//! CSS box-shadow (sigma = blur / 2). Outer shadows are cut under non-opaque
//! fills; inset shadows are limited to the padding box. Two techniques:
//! Vello's analytic blurred rounded rect (one circular radius) and a Gaussian
//! blur filter layer over the exact shape (any per-corner radii).
use crate::color;
use crate::geometry::{self, Radii};
use crate::painter::Painter;
use crate::stroke;
use fagent_layout::{SceneNode, Shadow};
use vello_common::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_common::kurbo::{BezPath, Rect, Shape};
use vello_common::peniko::Fill;

pub(crate) fn paint<P: Painter>(p: &mut P, n: &SceneNode, alpha: f32, inset: bool, how: Shadows) {
    if !n.shadow.iter().any(|s| s.inset == inset) {
        return;
    }
    let bounds = geometry::rect(&n.bounds);
    let radius = geometry::corners(n);
    let edges = n
        .stroke
        .as_ref()
        .filter(|s| s.align == "inside")
        .and_then(|_| stroke::edges(n));
    let (padding, padding_r) = edges.map_or((bounds, radius), |e| (e.inner, e.inner_r));
    for s in n.shadow.iter().rev().filter(|s| s.inset == inset) {
        let Some(c) = color::solid(&s.color, alpha) else {
            continue;
        };
        p.set_paint(c.into());
        if inset {
            inner(p, s, padding, padding_r, how);
        } else {
            outer(p, n, s, bounds, radius, how);
        }
    }
}

/// How blurred shadows are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shadows {
    /// Gaussian blur filter layer over the exact shape: matches Chromium (default).
    #[default]
    Filter,
    /// Vello's analytic blurred rounded rect where every corner shares one radius
    /// (no offscreen layer, but a visibly sharper falloff); filter otherwise.
    Analytic,
}

fn analytic(how: Shadows, r: Radii) -> bool {
    let first = r[0][0];
    how == Shadows::Analytic
        && r.iter()
            .all(|c| (c[0] - first).abs() < 1e-3 && (c[1] - first).abs() < 1e-3)
}

fn std_dev(s: &Shadow) -> f64 {
    f64::from(s.blur.max(0.0)) / 2.0
}

fn blur(sigma: f64) -> Filter {
    Filter::from_primitive(FilterPrimitive::gaussian_blur(sigma as f32, EdgeMode::None))
}

fn outer<P: Painter>(
    p: &mut P,
    n: &SceneNode,
    s: &Shadow,
    bounds: Rect,
    radius: Radii,
    how: Shadows,
) {
    let (dx, dy, spread) = (f64::from(s.x), f64::from(s.y), f64::from(s.spread));
    let source = Rect::new(
        bounds.x0 + dx - spread,
        bounds.y0 + dy - spread,
        bounds.x1 + dx + spread,
        bounds.y1 + dy + spread,
    );
    if source.width() <= 0.0 || source.height() <= 0.0 {
        return;
    }
    let r = geometry::reduced(source, color::spread_radii(radius, spread));
    let sigma = std_dev(s);
    let reach = source.inflate(3.0 * sigma + 1.0, 3.0 * sigma + 1.0);
    // CSS never paints an outer shadow under its own box. Skipping the cut is
    // invisible when the box fill is opaque, and saves a layer.
    let cut = !color::opaque(&n.fill);
    if cut {
        let mut keep = reach.union(bounds).inflate(1.0, 1.0).to_path(0.1);
        keep.extend(geometry::node_path(n));
        p.set_fill_rule(Fill::EvenOdd);
        p.push_clip_layer(&keep);
        p.set_fill_rule(Fill::NonZero);
    }
    if sigma <= 0.0 {
        p.fill_path(&geometry::rounded(source, r));
    } else if analytic(how, r) {
        let radius = geometry::single_radius(r);
        p.fill_blurred_rounded_rect_in(&reach.to_path(0.1), &source, radius, sigma as f32, false);
    } else {
        p.push_filter_layer(blur(sigma));
        p.fill_path(&geometry::rounded(source, r));
        p.pop_layer();
    }
    if cut {
        p.pop_layer();
    }
}

fn inner<P: Painter>(p: &mut P, s: &Shadow, padding: Rect, padding_r: Radii, how: Shadows) {
    if padding.width() <= 0.0 || padding.height() <= 0.0 {
        return;
    }
    let (dx, dy, spread) = (f64::from(s.x), f64::from(s.y), f64::from(s.spread));
    let box_path = geometry::rounded(padding, geometry::reduced(padding, padding_r));
    let source = Rect::new(
        padding.x0 + dx + spread,
        padding.y0 + dy + spread,
        padding.x1 + dx - spread,
        padding.y1 + dy - spread,
    );
    if source.width() <= 0.0 || source.height() <= 0.0 {
        // The hole closed: the whole padding box is in shadow.
        p.fill_path(&box_path);
        return;
    }
    let r = geometry::reduced(source, padding_r.map(|c| c.map(|v| (v - spread).max(0.0))));
    let sigma = std_dev(s);
    if sigma > 0.0 && analytic(how, r) {
        let radius = geometry::single_radius(r);
        p.fill_blurred_rounded_rect_in(&box_path, &source, radius, sigma as f32, true);
        return;
    }
    // The padding box minus the (offset) hole, blurred when needed.
    let grow = 3.0 * sigma + 1.0;
    let mut ring: BezPath = padding.union(source).inflate(grow, grow).to_path(0.1);
    geometry::append(&mut ring, source, r);
    p.push_clip_layer(&box_path);
    if sigma > 0.0 {
        p.push_filter_layer(blur(sigma));
    }
    p.set_fill_rule(Fill::EvenOdd);
    p.fill_path(&ring);
    p.set_fill_rule(Fill::NonZero);
    if sigma > 0.0 {
        p.pop_layer();
    }
    p.pop_layer();
}
