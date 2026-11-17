//! The drawing surface the encoder writes to. Both Vello backends expose the same
//! sparse-strips recording API, so one encoder serves `vello_cpu` and `vello_gpu`.
use vello_common::filter_effects::Filter;
use vello_common::kurbo::{Affine, BezPath, Rect};
use vello_common::paint::PaintType;
use vello_common::peniko::{Fill, FontData};

/// One shaped line for a backend glyph pipeline: `transform` maps the line origin
/// (CSS px, y down) to device px; glyph positions are CSS px from that origin.
pub struct GlyphRun<'a> {
    pub font: &'a FontData,
    pub size: f32,
    /// Normalized variation coordinates (F2Dot14 bits) of the sized instance.
    pub coords: &'a [i16],
    pub transform: Affine,
    pub glyphs: &'a [Glyph],
}

/// One shaped glyph, in CSS px relative to the line origin (y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

pub trait Painter {
    fn set_transform(&mut self, transform: Affine);
    fn set_paint(&mut self, paint: PaintType);
    /// Paint space relative to the current transform (gradients, images).
    fn set_paint_transform(&mut self, transform: Affine);
    fn reset_paint_transform(&mut self);
    fn set_fill_rule(&mut self, rule: Fill);
    fn fill_path(&mut self, path: &BezPath);
    fn fill_rect(&mut self, rect: &Rect);
    /// A solid paint with the coverage of a blurred rounded rect, limited to `path`;
    /// `invert` paints `1 - coverage` (inset shadows).
    fn fill_blurred_rounded_rect_in(
        &mut self,
        path: &BezPath,
        rect: &Rect,
        radius: f32,
        std_dev: f32,
        invert: bool,
    );
    fn push_clip_layer(&mut self, path: &BezPath);
    fn push_opacity_layer(&mut self, opacity: f32);
    /// Contents until the matching `pop_layer` go through `filter` (shadows: Gaussian blur).
    fn push_filter_layer(&mut self, filter: Filter);
    fn pop_layer(&mut self);
    /// Draws `run` with the current paint through the backend's glyph pipeline.
    /// `false` means unsupported: the caller fills glyph outlines itself.
    fn fill_glyphs(&mut self, _run: &GlyphRun<'_>) -> bool {
        false
    }
}

macro_rules! vello_painter {
    ($ty:ty) => {
        impl Painter for $ty {
            fn set_transform(&mut self, transform: Affine) {
                <$ty>::set_transform(self, transform);
            }
            fn set_paint(&mut self, paint: PaintType) {
                <$ty>::set_paint(self, paint);
            }
            fn set_paint_transform(&mut self, transform: Affine) {
                <$ty>::set_paint_transform(self, transform);
            }
            fn reset_paint_transform(&mut self) {
                <$ty>::reset_paint_transform(self);
            }
            fn set_fill_rule(&mut self, rule: Fill) {
                <$ty>::set_fill_rule(self, rule);
            }
            fn fill_path(&mut self, path: &BezPath) {
                <$ty>::fill_path(self, path);
            }
            fn fill_rect(&mut self, rect: &Rect) {
                <$ty>::fill_rect(self, rect);
            }
            fn fill_blurred_rounded_rect_in(
                &mut self,
                path: &BezPath,
                rect: &Rect,
                radius: f32,
                std_dev: f32,
                invert: bool,
            ) {
                <$ty>::fill_blurred_rounded_rect_in(self, path, rect, radius, std_dev, invert);
            }
            fn push_clip_layer(&mut self, path: &BezPath) {
                <$ty>::push_clip_layer(self, path);
            }
            fn push_opacity_layer(&mut self, opacity: f32) {
                <$ty>::push_opacity_layer(self, opacity);
            }
            fn push_filter_layer(&mut self, filter: Filter) {
                <$ty>::push_filter_layer(self, filter);
            }
            fn pop_layer(&mut self) {
                <$ty>::pop_layer(self);
            }
        }
    };
}

#[cfg(feature = "cpu")]
vello_painter!(vello_cpu::RenderContext);
#[cfg(feature = "gpu")]
vello_painter!(vello_gpu::Scene);
