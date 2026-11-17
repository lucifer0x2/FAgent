//! Board drawing for any Vello backend: per placed frame the viewer backdrop (white
//! box with the same soft drop shadow the Canvas2D board draws), then the scene through
//! the shared encoder with culling and zoom-dependent level of detail (Task 29 P0).
use crate::model::Model;
use fagent_paint_vello::vello_common::kurbo::{Affine, Rect, Shape, Vec2};
use fagent_paint_vello::vello_common::peniko::Color;
use fagent_paint_vello::{encode_prepared, EncodeOptions, Painter, Shadows};
use serde::Serialize;

/// Camera and level-of-detail policy for one frame of output, in device px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Device px per scene px (zoom × devicePixelRatio).
    pub scale: f64,
    pub tx: f64,
    pub ty: f64,
    pub width: f64,
    pub height: f64,
    /// Text whose font size is below this many device px is drawn as bars.
    pub greek_below_px: f64,
    /// Below this device scale shadows use Vello's analytic blurred rect instead of
    /// the filter layer (P0: indistinguishable when small, ~2–14 ms cheaper per board).
    pub analytic_shadows_below: f64,
    /// The viewer's white frame box with its drop shadow; off for exact scene captures
    /// (parity), which compare against a transparent Canvas2D reference.
    pub backdrop: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            scale: 1.0,
            tx: 0.0,
            ty: 0.0,
            width: 1.0,
            height: 1.0,
            greek_below_px: 4.0,
            analytic_shadows_below: 0.5,
            backdrop: true,
        }
    }
}

impl View {
    pub fn camera(&self) -> Affine {
        Affine::translate((self.tx, self.ty)) * Affine::scale(self.scale)
    }

    fn device(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, self.height)
    }

    pub fn shadows(&self) -> Shadows {
        if self.scale < self.analytic_shadows_below {
            Shadows::Analytic
        } else {
            Shadows::Filter
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DrawStats {
    pub frames: usize,
    pub culled_frames: usize,
    pub nodes: usize,
    pub glyphs: usize,
    pub layers: usize,
    pub culled: usize,
    pub greeked: usize,
    pub analytic_shadows: bool,
}

const BACKDROP_SHADOW: Color = Color::from_rgba8(29, 36, 51, 31); // #1d2433 at 0.12
const BACKDROP_BLUR_SIGMA: f64 = 10.0; // Konva shadowBlur 20
const BACKDROP_OFFSET_Y: f64 = 5.0;

/// Records every placed frame of `model` into `p`.
pub fn draw<P: Painter>(p: &mut P, model: &mut Model, view: &View) -> DrawStats {
    let camera = view.camera();
    let device = view.device();
    let options = EncodeOptions {
        shadows: view.shadows(),
        viewport: Some(device),
        greek_below_px: view.greek_below_px,
    };
    let mut stats = DrawStats {
        analytic_shadows: options.shadows == Shadows::Analytic,
        ..DrawStats::default()
    };
    let (frames, paint) = model.frames_and_paint();
    for frame in frames {
        let place = &frame.placement;
        let at = camera * Affine::translate((place.x, place.y));
        let backdrop = Rect::new(0.0, 0.0, place.width, place.height);
        let reach = 3.0 * BACKDROP_BLUR_SIGMA + 1.0;
        let mut extent = backdrop
            .inflate(reach, reach)
            .union(backdrop + Vec2::new(0.0, BACKDROP_OFFSET_Y));
        if let Some(scene) = frame.prepared.extent() {
            extent = extent.union(scene);
        }
        let shown = at.transform_rect_bbox(extent);
        if shown.x1 <= device.x0
            || shown.x0 >= device.x1
            || shown.y1 <= device.y0
            || shown.y0 >= device.y1
        {
            stats.culled_frames += 1;
            continue;
        }
        stats.frames += 1;
        if view.backdrop {
            p.set_transform(at);
            let source = backdrop + Vec2::new(0.0, BACKDROP_OFFSET_Y);
            p.set_paint(BACKDROP_SHADOW.into());
            p.fill_blurred_rounded_rect_in(
                &source.inflate(reach, reach).to_path(0.1),
                &source,
                0.0,
                BACKDROP_BLUR_SIGMA as f32,
                false,
            );
            p.set_paint(Color::WHITE.into());
            p.fill_rect(&backdrop);
        }
        let s = encode_prepared(p, &frame.scene, &frame.prepared, paint, at, options);
        stats.nodes += s.nodes;
        stats.glyphs += s.glyphs;
        stats.layers += s.layers;
        stats.culled += s.culled;
        stats.greeked += s.greeked;
    }
    stats
}
