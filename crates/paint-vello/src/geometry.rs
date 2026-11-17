//! CSS corner overlap reduction and elliptical rounded-rect paths (same curve
//! constants as the Canvas2D and tiny-skia painters).
use fagent_layout::{Bounds, SceneNode};
use vello_common::kurbo::{BezPath, Rect};

/// Per corner [rx, ry], clockwise from top-left.
pub(crate) type Radii = [[f64; 2]; 4];
const K: f64 = 0.552_284_8;

pub(crate) fn rect(b: &Bounds) -> Rect {
    let (x, y) = (f64::from(b.x), f64::from(b.y));
    Rect::new(x, y, x + f64::from(b.width), y + f64::from(b.height))
}

/// CSS: all radii shrink by one factor so adjacent corners never overlap.
pub(crate) fn corners(n: &SceneNode) -> Radii {
    let b = &n.bounds;
    let r = n.radius.each().map(f64::from);
    let (w, h) = (f64::from(b.width), f64::from(b.height));
    let mut f = 1.0_f64;
    for (side, sum) in [
        (w, r[0] + r[1]),
        (w, r[2] + r[3]),
        (h, r[0] + r[3]),
        (h, r[1] + r[2]),
    ] {
        if sum > 0.0 {
            f = f.min(side / sum);
        }
    }
    r.map(|r| [r * f.max(0.0); 2])
}

/// Canvas roundRect overlap reduction for arbitrary elliptical radii.
pub(crate) fn reduced(b: Rect, r: Radii) -> Radii {
    let mut f = 1.0_f64;
    for (side, sum) in [
        (b.width(), r[0][0] + r[1][0]),
        (b.width(), r[2][0] + r[3][0]),
        (b.height(), r[0][1] + r[3][1]),
        (b.height(), r[1][1] + r[2][1]),
    ] {
        if sum > 0.0 {
            f = f.min(side / sum);
        }
    }
    r.map(|r| [r[0] * f.max(0.0), r[1] * f.max(0.0)])
}

/// Appends a closed rounded rect; nothing for an empty rect. Radii are used as given.
pub(crate) fn append(p: &mut BezPath, b: Rect, r: Radii) {
    if b.width() <= 0.0 || b.height() <= 0.0 {
        return;
    }
    let [a, c, d, e] = r;
    let (x, y, right, bottom) = (b.x0, b.y0, b.x1, b.y1);
    p.move_to((x + a[0], y));
    p.line_to((right - c[0], y));
    p.curve_to(
        (right - c[0] + c[0] * K, y),
        (right, y + c[1] - c[1] * K),
        (right, y + c[1]),
    );
    p.line_to((right, bottom - d[1]));
    p.curve_to(
        (right, bottom - d[1] + d[1] * K),
        (right - d[0] + d[0] * K, bottom),
        (right - d[0], bottom),
    );
    p.line_to((x + e[0], bottom));
    p.curve_to(
        (x + e[0] - e[0] * K, bottom),
        (x, bottom - e[1] + e[1] * K),
        (x, bottom - e[1]),
    );
    p.line_to((x, y + a[1]));
    p.curve_to(
        (x, y + a[1] - a[1] * K),
        (x + a[0] - a[0] * K, y),
        (x + a[0], y),
    );
    p.close_path();
}

pub(crate) fn rounded(b: Rect, r: Radii) -> BezPath {
    let mut p = BezPath::new();
    append(&mut p, b, r);
    p
}

pub(crate) fn node_path(n: &SceneNode) -> BezPath {
    rounded(rect(&n.bounds), corners(n))
}

pub(crate) fn polygon(points: &[[f64; 2]]) -> BezPath {
    let mut p = BezPath::new();
    for (i, [x, y]) in points.iter().enumerate() {
        if i == 0 {
            p.move_to((*x, *y));
        } else {
            p.line_to((*x, *y));
        }
    }
    p.close_path();
    p
}

/// The circular radius Vello's blurred rounded rect accepts: exact when every
/// corner matches, otherwise the mean (documented P0 approximation).
pub(crate) fn single_radius(r: Radii) -> f32 {
    (r.iter().map(|c| c[0] + c[1]).sum::<f64>() / 8.0) as f32
}
