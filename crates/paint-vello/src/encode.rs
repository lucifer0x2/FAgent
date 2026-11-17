//! scene-3 → Vello drawing calls, in the Canvas2D painter's order: outer shadows,
//! fill, text/image, inset shadows, children (clipped when `clip`), then stroke.
use crate::color;
use crate::geometry;
use crate::painter::Painter;
use crate::resources::{Image, Resources};
use crate::shadow::{self, Shadows};
use crate::{cull, stroke, text};
use fagent_layout::{Asset, Font, SceneNode};
use serde::Deserialize;
use std::collections::HashMap;
use vello_common::kurbo::{Affine, Rect};
use vello_common::paint::{self, PaintType};
use vello_common::peniko::{Extend, ImageQuality, ImageSampler};

/// The parts of a scene-3 document the painter reads; other fields are ignored.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaintScene {
    #[serde(default)]
    pub provenance: Option<Provenance>,
    pub nodes: Vec<SceneNode>,
    #[serde(default)]
    pub fonts: Vec<Font>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// Only the viewport is read: raster size = ceil(viewport × scale).
#[derive(Clone, Debug, Deserialize)]
pub struct Provenance {
    pub viewport: Viewport,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Viewport {
    pub width: f64,
    pub height: f64,
}

impl PaintScene {
    /// Device size for `scale`, from the scene viewport.
    pub fn device_size(&self, scale: f64) -> Option<(u32, u32)> {
        let v = self.provenance.as_ref()?.viewport;
        let (w, h) = ((v.width * scale).ceil(), (v.height * scale).ceil());
        (w >= 1.0 && h >= 1.0 && w <= 65535.0 && h <= 65535.0).then_some((w as u32, h as u32))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EncodeOptions {
    pub shadows: Shadows,
    /// Device-px rect being drawn; subtrees whose extent misses it are skipped.
    /// `None` encodes everything.
    pub viewport: Option<Rect>,
    /// Level of detail: text whose font size is below this many device px is drawn as
    /// one translucent bar per line ("greeking"); 0 always draws glyphs.
    pub greek_below_px: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EncodeStats {
    pub nodes: usize,
    pub glyphs: usize,
    pub layers: usize,
    /// Of `layers`, the isolated group-opacity layers.
    pub opacity_layers: usize,
    pub placeholders: usize,
    /// Subtrees skipped by viewport culling (counted at their root).
    pub culled: usize,
    /// Text lines drawn as bars by `greek_below_px`.
    pub greeked: usize,
}

const MAX_DEPTH: usize = 128;

/// Scene structure the encoder walks: children, roots and (lazily) culling extents.
/// Build it once per scene and reuse it across frames ([`encode_prepared`]).
#[derive(Clone, Debug, Default)]
pub struct Prepared {
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
    extents: Vec<Rect>,
}

impl Prepared {
    pub fn new(scene: &PaintScene) -> Self {
        Self::from_nodes(&scene.nodes)
    }

    /// [`Prepared::new`] for a scene's node list.
    pub fn from_nodes(nodes: &[SceneNode]) -> Self {
        let index: HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let mut children = vec![Vec::new(); nodes.len()];
        let mut roots = Vec::new();
        for (i, n) in nodes.iter().enumerate() {
            match n.parent_id.as_deref().and_then(|id| index.get(id)) {
                Some(&parent) if parent != i => children[parent].push(i),
                _ => roots.push(i),
            }
        }
        let extents = cull::extents(nodes, &children, &roots);
        Self {
            children,
            roots,
            extents,
        }
    }

    /// Scene-px extent of everything the scene can paint (culling bounds of its roots).
    pub fn extent(&self) -> Option<Rect> {
        self.roots
            .iter()
            .map(|&i| self.extents[i])
            .reduce(|a, b| a.union(b))
    }
}

/// Records `scene` into `p`; `camera` maps scene px to device px.
pub fn encode<P: Painter>(
    p: &mut P,
    scene: &PaintScene,
    resources: &mut Resources,
    camera: Affine,
    options: EncodeOptions,
) -> EncodeStats {
    encode_prepared(p, scene, &Prepared::new(scene), resources, camera, options)
}

/// [`encode`] with a [`Prepared`] built from this same `scene`.
pub fn encode_prepared<P: Painter>(
    p: &mut P,
    scene: &PaintScene,
    prepared: &Prepared,
    resources: &mut Resources,
    camera: Affine,
    options: EncodeOptions,
) -> EncodeStats {
    encode_nodes(p, &scene.nodes, prepared, resources, camera, options)
}

/// [`encode_prepared`] for a node list and a [`Prepared::from_nodes`] of that same list.
pub fn encode_nodes<P: Painter>(
    p: &mut P,
    nodes: &[SceneNode],
    prepared: &Prepared,
    resources: &mut Resources,
    camera: Affine,
    options: EncodeOptions,
) -> EncodeStats {
    let [a, b, c, d, _, _] = camera.as_coeffs();
    let mut e = Encoder {
        nodes,
        children: &prepared.children,
        extents: &prepared.extents,
        resources,
        camera,
        options,
        device_scale: (a * d - b * c).abs().sqrt().max(1e-6),
        stats: EncodeStats::default(),
    };
    p.set_transform(camera);
    for &i in &prepared.roots {
        e.draw(p, i, 1.0, 0);
    }
    e.stats
}

struct Encoder<'a> {
    nodes: &'a [SceneNode],
    children: &'a [Vec<usize>],
    extents: &'a [Rect],
    resources: &'a mut Resources,
    camera: Affine,
    options: EncodeOptions,
    device_scale: f64,
    stats: EncodeStats,
}

impl Encoder<'_> {
    fn draw<P: Painter>(&mut self, p: &mut P, i: usize, alpha: f32, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        if let Some(view) = self.options.viewport {
            if !cull::visible(self.extents[i], self.camera, view) {
                self.stats.culled += 1;
                return;
            }
        }
        let nodes = self.nodes;
        let n = &nodes[i];
        let a = alpha * n.opacity;
        if a <= 0.0 {
            return;
        }
        self.stats.nodes += 1;
        // Group opacity needs an isolated layer only when parts can overlap.
        if a < 1.0 && self.units(i) >= 2 {
            self.stats.layers += 1;
            self.stats.opacity_layers += 1;
            p.push_opacity_layer(a);
            self.contents(p, i, 1.0, depth);
            p.pop_layer();
        } else {
            self.contents(p, i, a, depth);
        }
    }

    fn units(&self, i: usize) -> usize {
        let n = &self.nodes[i];
        let own = n.kind == "text" || n.kind == "image" || color::visible(&n.fill);
        usize::from(own)
            + usize::from(n.kind == "text" && n.lines.len() > 1)
            + usize::from(n.decoration.is_some())
            + usize::from(stroke::edges(n).is_some())
            + n.shadow.len()
            + self.children[i].len()
    }

    fn contents<P: Painter>(&mut self, p: &mut P, i: usize, alpha: f32, depth: usize) {
        let (nodes, children) = (self.nodes, self.children);
        let n = &nodes[i];
        shadow::paint(p, n, alpha, false, self.options.shadows);
        if n.kind != "text" {
            let b = geometry::rect(&n.bounds);
            if let Some((paint, at)) = color::fill(&n.fill, b, alpha) {
                p.set_paint(paint);
                p.set_paint_transform(at);
                p.fill_path(&geometry::node_path(n));
                p.reset_paint_transform();
            }
        }
        match n.kind.as_str() {
            "text" => match n
                .font_id
                .as_ref()
                .and_then(|f| self.resources.fonts.get_mut(f))
            {
                Some(_)
                    if f64::from(n.font_size) * self.device_scale < self.options.greek_below_px =>
                {
                    self.stats.greeked += text::greek(p, n, alpha);
                }
                Some(font) => self.stats.glyphs += text::paint(p, n, font, alpha, self.camera),
                None => self.placeholder(p, n, alpha),
            },
            "image" => match n
                .asset_id
                .as_ref()
                .and_then(|a| self.resources.images.get(a))
            {
                Some(image) => draw_image(p, n, image, alpha),
                None => self.placeholder(p, n, alpha),
            },
            _ => {}
        }
        shadow::paint(p, n, alpha, true, self.options.shadows);
        let kids = &children[i];
        if !kids.is_empty() {
            let b = &n.bounds;
            if !n.clip {
                for &c in kids {
                    self.draw(p, c, alpha, depth + 1);
                }
            } else if b.width > 0.0 && b.height > 0.0 {
                self.stats.layers += 1;
                p.push_clip_layer(&geometry::node_path(n));
                for &c in kids {
                    self.draw(p, c, alpha, depth + 1);
                }
                p.pop_layer();
            }
        }
        // Deliberately after descendants, outside this node's own clip.
        stroke::paint(p, n, alpha, self.device_scale);
    }

    /// "MISSING FONT/IMAGE" in a 3x5 bitmap alphabet, as the other painters draw it.
    fn placeholder<P: Painter>(&mut self, p: &mut P, n: &SceneNode, alpha: f32) {
        self.stats.placeholders += 1;
        let image = n.kind == "image";
        let b = geometry::rect(&n.bounds);
        if image {
            if let Some(c) = color::solid("#ffe4e6", alpha) {
                p.set_paint(c.into());
                p.fill_rect(&b);
            }
        }
        let (label, ink) = if image {
            ("MISSING IMAGE", "#9f1239")
        } else {
            ("MISSING FONT", "#be123c")
        };
        let Some(c) = color::solid(ink, alpha) else {
            return;
        };
        p.set_paint(c.into());
        for (i, ch) in label.chars().enumerate() {
            for (y, row) in bits(ch).iter().enumerate() {
                for x in 0..3 {
                    if row & (1 << (2 - x)) != 0 {
                        let x0 = b.x0 + 4.0 + i as f64 * 8.0 + f64::from(x) * 2.0;
                        let y0 = b.y0 + 4.0 + y as f64 * 2.0;
                        p.fill_rect(&Rect::new(x0, y0, x0 + 2.0, y0 + 2.0));
                    }
                }
            }
        }
    }
}

fn bits(c: char) -> [u8; 5] {
    match c {
        'M' => [5, 7, 7, 5, 5],
        'I' => [7, 2, 2, 2, 7],
        'S' => [7, 4, 7, 1, 7],
        'N' => [5, 7, 7, 7, 5],
        'G' => [7, 4, 5, 5, 7],
        'F' => [7, 4, 6, 4, 4],
        'O' => [7, 5, 5, 5, 7],
        'T' => [7, 2, 2, 2, 2],
        'A' => [2, 5, 7, 5, 5],
        'E' => [7, 4, 6, 4, 7],
        _ => [0; 5],
    }
}

/// `fit`/`fill` stretch, `contain` letterboxes, `cover` crops; always centered and
/// clipped to the node's rounded box. `crop` selects a normalized source rect first.
fn draw_image<P: Painter>(p: &mut P, n: &SceneNode, image: &Image, alpha: f32) {
    let b = geometry::rect(&n.bounds);
    let [cx, cy, cw, ch] = n
        .crop
        .as_ref()
        .map_or([0.0, 0.0, 1.0, 1.0], |c| [c.x, c.y, c.width, c.height])
        .map(f64::from);
    let (iw, ih) = (f64::from(image.width), f64::from(image.height));
    let (sw, sh) = (iw * cw, ih * ch);
    if sw <= 0.0 || sh <= 0.0 || b.width() <= 0.0 || b.height() <= 0.0 {
        return;
    }
    let (w, h) = match n.fit.as_str() {
        "cover" | "contain" => {
            let (fx, fy) = (b.width() / sw, b.height() / sh);
            let r = if n.fit == "cover" {
                fx.max(fy)
            } else {
                fx.min(fy)
            };
            (sw * r, sh * r)
        }
        _ => (b.width(), b.height()),
    };
    let x = b.x0 + (b.width() - w) / 2.0;
    let y = b.y0 + (b.height() - h) / 2.0;
    let brush = paint::Image {
        image: image.source.clone(),
        sampler: ImageSampler {
            x_extend: Extend::Pad,
            y_extend: Extend::Pad,
            quality: ImageQuality::Medium,
            alpha,
        },
    };
    p.push_clip_layer(&geometry::node_path(n));
    p.set_paint(PaintType::Image(brush));
    p.set_paint_transform(
        Affine::translate((x, y))
            * Affine::scale_non_uniform(w / sw, h / sh)
            * Affine::translate((-iw * cx, -ih * cy)),
    );
    p.fill_rect(&Rect::new(x, y, x + w, y + h));
    p.reset_paint_transform();
    p.pop_layer();
}
