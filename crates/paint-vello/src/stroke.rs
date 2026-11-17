//! CSS borders: an even-odd ring between the outer and inner rounded rects, and
//! per-side colors as the ring clipped to each side's wedge (as Chrome draws them).
use crate::color;
use crate::geometry::{self, Radii};
use crate::painter::Painter;
use fagent_layout::{SceneNode, SceneStroke};
use vello_common::kurbo::{BezPath, Rect};

pub(crate) struct Edges {
    pub outer: Rect,
    pub inner: Rect,
    pub outer_r: Radii,
    pub inner_r: Radii,
    /// Per corner [horizontal width, vertical width].
    pub k: [[f64; 2]; 4],
}

pub(crate) fn edges(n: &SceneNode) -> Option<Edges> {
    let s = n.stroke.as_ref()?;
    if !s.width.iter().any(|w| *w > 0.0) {
        return None;
    }
    let [t, r, b, l] = s.width.map(f64::from);
    let f = match s.align.as_str() {
        "outside" => 1.0,
        "center" => 0.5,
        _ => 0.0,
    };
    let k = [[l, t], [r, t], [r, b], [l, b]];
    let radii = geometry::corners(n);
    let bounds = geometry::rect(&n.bounds);
    let outer = Rect::new(
        bounds.x0 - l * f,
        bounds.y0 - t * f,
        bounds.x1 + r * f,
        bounds.y1 + b * f,
    );
    let outer_r: Radii = std::array::from_fn(|i| {
        if radii[i][0] > 0.0 {
            [radii[i][0] + k[i][0] * f, radii[i][1] + k[i][1] * f]
        } else {
            [0.0; 2]
        }
    });
    let inner = Rect::new(outer.x0 + l, outer.y0 + t, outer.x1 - r, outer.y1 - b);
    let inner_r = std::array::from_fn(|i| {
        [
            (outer_r[i][0] - k[i][0]).max(0.0),
            (outer_r[i][1] - k[i][1]).max(0.0),
        ]
    });
    (outer.width() > 0.0 && outer.height() > 0.0).then_some(Edges {
        outer,
        inner,
        outer_r,
        inner_r,
        k,
    })
}

/// Outer outline plus the inner outline in the opposite direction, filled non-zero.
/// A zero-width side makes both outlines share that edge; opposite windings cancel
/// its antialiased coverage exactly, whereas even-odd turns two half-covered pixels
/// into a full one (a hairline on sides that must stay unpainted).
fn ring_path(e: &Edges) -> BezPath {
    let mut p = geometry::rounded(e.outer, e.outer_r);
    if e.inner.width() > 0.0 && e.inner.height() > 0.0 {
        // Overlapping inner ellipses are reduced again here; tiny-skia keeps them
        // and clips instead. Visible only for very wide borders on small boxes.
        let inner = geometry::rounded(e.inner, geometry::reduced(e.inner, e.inner_r));
        p.extend(inner.reverse_subpaths());
    }
    p
}

fn ring<P: Painter>(p: &mut P, e: &Edges, s: &str, alpha: f32) {
    let Some(c) = color::solid(s, alpha) else {
        return;
    };
    p.set_paint(c.into());
    p.fill_path(&ring_path(e));
}

/// Painted after descendants, outside the node's own clip.
pub(crate) fn paint<P: Painter>(p: &mut P, n: &SceneNode, alpha: f32, device_scale: f64) {
    let Some(e) = edges(n) else {
        return;
    };
    let s = n.stroke.as_ref().expect("edges require a stroke");
    let colors: Vec<_> = (0..4)
        .filter(|i| s.width[*i] > 0.0)
        .map(|i| &s.color[i])
        .collect();
    if colors.iter().all(|c| *c == colors[0]) {
        ring(p, &e, colors[0], alpha);
    } else {
        sides(p, &e, s, alpha, device_scale);
    }
}

fn wedges(e: &Edges) -> [[[f64; 2]; 4]; 4] {
    let corners = |r: Rect| [[r.x0, r.y0], [r.x1, r.y0], [r.x1, r.y1], [r.x0, r.y1]];
    let o = corners(e.outer);
    let mut inner = corners(e.inner);
    for i in 0..4 {
        let [h, v] = e.k[i];
        let [rx, ry] = e.inner_r[i];
        let t = if rx > 0.0 && ry > 0.0 && h + v > 0.0 {
            1.0 / (h / rx + v / ry)
        } else {
            0.0
        };
        inner[i][0] += (inner[i][0] - o[i][0]) * t;
        inner[i][1] += (inner[i][1] - o[i][1]) * t;
    }
    std::array::from_fn(|n| {
        let m = (n + 1) % 4;
        [o[n], o[m], inner[m], inner[n]]
    })
}

const NORMAL: [[f64; 2]; 4] = [[0.0, -1.0], [1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]];

/// Transparent sides first; opaque neighbours push the seam one device pixel so
/// antialiased wedge edges do not leave a hairline gap.
fn sides<P: Painter>(p: &mut P, e: &Edges, s: &SceneStroke, alpha: f32, device_scale: f64) {
    let a = s
        .color
        .each_ref()
        .map(|c| color::parse(c).map_or(0.0, color::alpha));
    let mut order: Vec<usize> = (0..4).filter(|n| s.width[*n] > 0.0).collect();
    order.sort_by(|x, y| {
        a[*x]
            .total_cmp(&a[*y])
            .then((x % 2).cmp(&(y % 2)))
            .then(x.cmp(y))
    });
    let mut rank = [4; 4];
    for (i, n) in order.iter().enumerate() {
        rank[*n] = i;
    }
    let d = if order.iter().all(|n| a[*n] == 1.0) {
        1.0 / device_scale
    } else {
        0.0
    };
    let parts = wedges(e);
    for n in order {
        let shift = |pt: [f64; 2], m: usize, corner: usize, out: f64| {
            let same = s.width[m] == 0.0 || s.color[m] == s.color[n];
            let square = e.outer_r[corner][0] == 0.0 || e.outer_r[corner][1] == 0.0;
            let s = if same || (square && rank[m] > rank[n]) {
                d
            } else {
                0.0
            };
            std::array::from_fn(|i| pt[i] + s * (NORMAL[m][i] - NORMAL[n][i] + out * NORMAL[n][i]))
        };
        let [a, b, c, f] = parts[n];
        let (prev, next) = ((n + 3) % 4, (n + 1) % 4);
        let wedge = geometry::polygon(&[
            shift(a, prev, n, 2.0),
            shift(b, next, next, 2.0),
            shift(c, next, next, 0.0),
            shift(f, prev, n, 0.0),
        ]);
        p.push_clip_layer(&wedge);
        ring(p, e, &s.color[n], alpha);
        p.pop_layer();
    }
}
