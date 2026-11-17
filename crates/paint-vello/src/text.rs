//! Text lines from the scene drawn as glyph outlines positioned by the shared
//! shaping run (the scene carries only line strings and origins). Underline and
//! strike-through are rects, like the other painters.
use crate::color;
use crate::painter::{GlyphRun, Painter};
use crate::resources::Font;
use fagent_layout::SceneNode;
use vello_common::kurbo::{Affine, Rect};

/// Returns the number of glyphs drawn.
pub(crate) fn paint<P: Painter>(
    p: &mut P,
    n: &SceneNode,
    font: &mut Font,
    alpha: f32,
    camera: Affine,
) -> usize {
    let Some(c) = color::solid(n.fill.as_solid(), alpha) else {
        return 0;
    };
    let Some(scale) = font.scale(n.font_size) else {
        return 0;
    };
    p.set_paint(c.into());
    let (bx, by) = (f64::from(n.bounds.x), f64::from(n.bounds.y));
    let mut drawn = 0;
    for l in &n.lines {
        let (lx, ly) = (bx + f64::from(l.x), by + f64::from(l.y));
        if let Some(run) = font.run(&l.text, n.font_size, n.letter_spacing) {
            let coords = font.coords(n.font_size).unwrap_or_default();
            let line = GlyphRun {
                font: &font.data,
                size: n.font_size,
                coords: &coords,
                transform: camera * Affine::translate((lx, ly)),
                glyphs: &run,
            };
            if p.fill_glyphs(&line) {
                drawn += run.len();
            } else {
                for g in run.iter() {
                    let Some(path) = font.outline(g.id, n.font_size) else {
                        continue;
                    };
                    // Font units are y-up.
                    p.set_transform(
                        camera
                            * Affine::translate((lx + f64::from(g.x), ly + f64::from(g.y)))
                            * Affine::scale_non_uniform(scale, -scale),
                    );
                    p.fill_path(&path);
                    drawn += 1;
                }
            }
        }
        if let Some(d) = &n.decoration {
            p.set_transform(camera);
            // JS Math.round ties toward +infinity, as in the Canvas2D painter.
            let y = (ly + f64::from(d.offset) + 0.5).floor();
            p.fill_rect(&Rect::new(
                lx,
                y,
                lx + f64::from(l.width),
                y + f64::from(d.thickness),
            ));
        }
    }
    p.set_transform(camera);
    drawn
}

/// Level-of-detail stand-in for text too small to read: one bar per line over the
/// x-height band, at 40% of the text alpha. Returns the number of lines drawn.
pub(crate) fn greek<P: Painter>(p: &mut P, n: &SceneNode, alpha: f32) -> usize {
    let Some(c) = color::solid(n.fill.as_solid(), alpha * 0.4) else {
        return 0;
    };
    p.set_paint(c.into());
    let (bx, by, size) = (
        f64::from(n.bounds.x),
        f64::from(n.bounds.y),
        f64::from(n.font_size),
    );
    for l in &n.lines {
        let (x, baseline) = (bx + f64::from(l.x), by + f64::from(l.y));
        p.fill_rect(&Rect::new(
            x,
            baseline - 0.55 * size,
            x + f64::from(l.width),
            baseline,
        ));
    }
    n.lines.len()
}
