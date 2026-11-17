//! Scene colors and gradients as Vello paints. Gradients interpolate in
//! premultiplied sRGB (peniko's default), which is what CSS specifies.
use crate::geometry::Radii;
use fagent_layout::{Gradient, Paint, Radius};
use vello_common::color::DynamicColor;
use vello_common::kurbo::{Affine, Point, Rect};
use vello_common::paint::PaintType;
use vello_common::peniko::{self, Color, ColorStop};

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa` or `transparent`; anything else is `None`.
pub(crate) fn parse(s: &str) -> Option<Color> {
    if s == "transparent" {
        return Some(Color::TRANSPARENT);
    }
    let h = s.strip_prefix('#')?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let h: String = match h.len() {
        3 | 4 => h.chars().flat_map(|c| [c, c]).collect(),
        6 | 8 => h.to_owned(),
        _ => return None,
    };
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    let a = if h.len() == 8 { byte(6)? } else { 255 };
    Some(Color::from_rgba8(byte(0)?, byte(2)?, byte(4)?, a))
}

pub(crate) fn alpha(c: Color) -> f32 {
    c.components[3]
}

pub(crate) fn solid(s: &str, alpha: f32) -> Option<Color> {
    let c = parse(s)?.multiply_alpha(alpha);
    (self::alpha(c) > 0.0).then_some(c)
}

/// Whether a fill fully hides what is under its shape (outer shadows may skip the cut).
pub(crate) fn opaque(fill: &Paint) -> bool {
    matches!(fill, Paint::Solid(s) if parse(s).is_some_and(|c| alpha(c) == 1.0))
}

pub(crate) fn visible(fill: &Paint) -> bool {
    match fill {
        Paint::Solid(s) => parse(s).is_some_and(|c| alpha(c) > 0.0),
        Paint::Gradient(_) => true,
    }
}

/// The paint for a box fill and the paint transform it needs (radial ellipses).
pub(crate) fn fill(fill: &Paint, b: Rect, alpha: f32) -> Option<(PaintType, Affine)> {
    match fill {
        Paint::Solid(s) => solid(s, alpha).map(|c| (c.into(), Affine::IDENTITY)),
        Paint::Gradient(g) => gradient(g, b, alpha),
    }
}

fn stops(source: &[(f32, String)], alpha: f32) -> Option<Vec<ColorStop>> {
    source
        .iter()
        .map(|(offset, s)| {
            let c = parse(s)?.multiply_alpha(alpha);
            Some(ColorStop {
                offset: *offset,
                color: DynamicColor::from_alpha_color(c),
            })
        })
        .collect()
}

fn gradient(g: &Gradient, b: Rect, alpha: f32) -> Option<(PaintType, Affine)> {
    match g {
        Gradient::Linear { angle, stops: s } => {
            // CSS: the gradient line passes through the center at `angle` (0 = up) and
            // is long enough that the corners get the end colors.
            let a = f64::from(*angle).to_radians();
            let (dx, dy) = (a.sin(), -a.cos());
            let len = (b.width() * dx).abs() + (b.height() * dy).abs();
            let c = b.center();
            let half = (dx * len / 2.0, dy * len / 2.0);
            let start = Point::new(c.x - half.0, c.y - half.1);
            let end = Point::new(c.x + half.0, c.y + half.1);
            let g = peniko::Gradient::new_linear(start, end).with_stops(&stops(s, alpha)?[..]);
            Some((PaintType::Gradient(g), Affine::IDENTITY))
        }
        Gradient::Radial {
            center: [cx, cy],
            radius,
            stops: s,
        } => {
            let (cx, cy) = (f64::from(*cx), f64::from(*cy));
            let [rx, ry] = match radius {
                Some(Radius::Circle(r)) => [f64::from(*r); 2],
                Some(Radius::Ellipse([x, y])) => [f64::from(*x), f64::from(*y)],
                // CSS `farthest-corner` for an ellipse with the box's aspect ratio.
                None => [
                    2f64.sqrt()
                        * (cx * b.width())
                            .abs()
                            .max((b.width() - cx * b.width()).abs()),
                    2f64.sqrt()
                        * (cy * b.height())
                            .abs()
                            .max((b.height() - cy * b.height()).abs()),
                ],
            };
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            let g = peniko::Gradient::new_radial((0.0, 0.0), 1.0).with_stops(&stops(s, alpha)?[..]);
            let at = Affine::translate((b.x0 + cx * b.width(), b.y0 + cy * b.height()))
                * Affine::scale_non_uniform(rx, ry);
            Some((PaintType::Gradient(g), at))
        }
    }
}

/// Spread changes corner radii like Chrome's box-shadow (the cubic falloff for
/// radii smaller than the spread).
pub(crate) fn spread_radii(r: Radii, s: f64) -> Radii {
    r.map(|c| c.map(|r| spread_radius(r, s)))
}

fn spread_radius(r: f64, s: f64) -> f64 {
    if r == 0.0 {
        0.0
    } else if s <= 0.0 {
        (r + s).max(0.0)
    } else if r >= s {
        r + s
    } else {
        r + s * (1.0 + (r / s - 1.0).powi(3))
    }
}
