//! Viewport culling: a conservative scene-space extent per subtree (own paint
//! incl. shadows, outside strokes and text overflow, plus unclipped descendants),
//! so off-screen subtrees are skipped before any path reaches strip generation.
use crate::geometry;
use fagent_layout::SceneNode;
use vello_common::kurbo::{Affine, Rect, Vec2};

/// Own paint extent: bounds grown by shadows, strokes and text lines.
fn own(n: &SceneNode) -> Rect {
    let b = geometry::rect(&n.bounds);
    let mut r = b;
    for s in &n.shadow {
        if s.inset {
            continue;
        }
        // Gaussian sigma = blur / 2 → 3 sigma covers what is visible.
        let grow = f64::from(s.spread.max(0.0)) + 1.5 * f64::from(s.blur.max(0.0)) + 1.0;
        let moved = b + Vec2::new(f64::from(s.x), f64::from(s.y));
        r = r.union(moved.inflate(grow, grow));
    }
    if let Some(s) = &n.stroke {
        let w = s.width.iter().copied().fold(0.0_f32, f32::max);
        let w = f64::from(w) + 1.0;
        r = r.union(b.inflate(w, w));
    }
    if n.kind == "text" {
        let pad = f64::from(n.font_size.max(n.line_height).max(0.0));
        for l in &n.lines {
            let x = b.x0 + f64::from(l.x);
            let y = b.y0 + f64::from(l.y);
            r = r.union(Rect::new(
                x - pad,
                y - pad,
                x + f64::from(l.width) + pad,
                y + 2.0 * pad,
            ));
        }
    }
    r
}

/// Subtree extents in scene px, indexed like `nodes`; children clipped by `clip` parents.
pub(crate) fn extents(nodes: &[SceneNode], children: &[Vec<usize>], roots: &[usize]) -> Vec<Rect> {
    let mut out = vec![Rect::ZERO; nodes.len()];
    // Iterative post-order: parents are finished after all of their children.
    let mut stack: Vec<(usize, bool)> = roots.iter().map(|&i| (i, false)).collect();
    let mut seen = vec![false; nodes.len()];
    while let Some((i, done)) = stack.pop() {
        if done {
            let n = &nodes[i];
            let mut r = own(n);
            let b = geometry::rect(&n.bounds);
            for &c in &children[i] {
                let child = if n.clip { out[c].intersect(b) } else { out[c] };
                if child.width() > 0.0 && child.height() > 0.0 {
                    r = r.union(child);
                }
            }
            out[i] = r;
        } else if !seen[i] {
            seen[i] = true;
            stack.push((i, true));
            stack.extend(children[i].iter().map(|&c| (c, false)));
        }
    }
    out
}

/// Whether `extent` (scene px) can touch `viewport` (device px) under `camera`.
pub(crate) fn visible(extent: Rect, camera: Affine, viewport: Rect) -> bool {
    let d = camera.transform_rect_bbox(extent);
    d.x1 > viewport.x0 && d.x0 < viewport.x1 && d.y1 > viewport.y0 && d.y0 < viewport.y1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, parent: Option<&str>, x: f32, w: f32, clip: bool) -> SceneNode {
        let mut n: SceneNode = serde_json::from_value(serde_json::json!({
            "id": id, "parentId": parent, "kind": "frame",
            "bounds": {"x": x, "y": 0, "width": w, "height": 10},
            "lines": [], "fontId": null, "fontSize": 16, "lineHeight": 24, "assetId": null,
            "fit": "contain", "opacity": 1, "clip": clip, "radius": 0, "crop": null, "fill": "#fff"
        }))
        .unwrap();
        n.parent_id = parent.map(Into::into);
        n
    }

    #[test]
    fn unclipped_children_grow_the_parent_extent_and_clipped_ones_do_not() {
        let nodes = [
            node("a", None, 0.0, 10.0, false),
            node("b", Some("a"), 100.0, 10.0, false),
            node("c", None, 0.0, 10.0, true),
            node("d", Some("c"), 100.0, 10.0, false),
        ];
        let children = vec![vec![1], vec![], vec![3], vec![]];
        let e = extents(&nodes, &children, &[0, 2]);
        assert_eq!(e[0].x1, 110.0);
        assert_eq!(e[2].x1, 10.0);
        let view = Rect::new(0.0, 0.0, 50.0, 50.0);
        assert!(visible(e[0], Affine::IDENTITY, view));
        assert!(!visible(e[1], Affine::IDENTITY, view));
        assert!(visible(e[1], Affine::translate((-80.0, 0.0)), view));
    }
}
