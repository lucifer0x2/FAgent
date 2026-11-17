//! `vello_gpu` painter with access to the renderer resources, so text can go
//! through Vello's glyph pipeline (`Scene::glyph_run`, optionally its experimental
//! glyph atlas) instead of one filled outline path per glyph.
use crate::painter::{GlyphRun, Painter};
use vello_common::filter_effects::Filter;
use vello_common::kurbo::{Affine, BezPath, Rect};
use vello_common::paint::PaintType;
use vello_common::peniko::Fill;

/// How text glyphs reach the GPU scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GpuText {
    /// Outline paths from the shared shaping run (same code as `vello_cpu`).
    #[default]
    Outlines,
    /// `glyph_run` with outline caching in `vello_gpu::Resources`, no atlas.
    GlyphRun,
    /// `glyph_run` with the glyph atlas: glyphs rasterized once, then drawn as images.
    Atlas,
}

pub struct GpuPainter<'a> {
    pub scene: &'a mut vello_gpu::Scene,
    pub resources: &'a mut vello_gpu::Resources,
    pub text: GpuText,
}

impl Painter for GpuPainter<'_> {
    fn set_transform(&mut self, transform: Affine) {
        self.scene.set_transform(transform);
    }
    fn set_paint(&mut self, paint: PaintType) {
        self.scene.set_paint(paint);
    }
    fn set_paint_transform(&mut self, transform: Affine) {
        self.scene.set_paint_transform(transform);
    }
    fn reset_paint_transform(&mut self) {
        self.scene.reset_paint_transform();
    }
    fn set_fill_rule(&mut self, rule: Fill) {
        self.scene.set_fill_rule(rule);
    }
    fn fill_path(&mut self, path: &BezPath) {
        self.scene.fill_path(path);
    }
    fn fill_rect(&mut self, rect: &Rect) {
        self.scene.fill_rect(rect);
    }
    fn fill_blurred_rounded_rect_in(
        &mut self,
        path: &BezPath,
        rect: &Rect,
        radius: f32,
        std_dev: f32,
        invert: bool,
    ) {
        Painter::fill_blurred_rounded_rect_in(self.scene, path, rect, radius, std_dev, invert);
    }
    fn push_clip_layer(&mut self, path: &BezPath) {
        self.scene.push_clip_layer(path);
    }
    fn push_opacity_layer(&mut self, opacity: f32) {
        self.scene.push_opacity_layer(opacity);
    }
    fn push_filter_layer(&mut self, filter: Filter) {
        self.scene.push_filter_layer(filter);
    }
    fn pop_layer(&mut self) {
        self.scene.pop_layer();
    }
    fn fill_glyphs(&mut self, run: &GlyphRun<'_>) -> bool {
        if self.text == GpuText::Outlines {
            return false;
        }
        self.scene.set_transform(run.transform);
        let glyphs = run.glyphs.iter().map(|g| glifo::Glyph {
            id: g.id,
            x: g.x,
            y: g.y,
        });
        // Errors only report glyphs that rendered blank; the rest are drawn.
        let _ = self
            .scene
            .glyph_run(self.resources, run.font)
            .font_size(run.size)
            .hint(false)
            .normalized_coords(run.coords)
            .atlas_cache(self.text == GpuText::Atlas)
            .fill_glyphs(glyphs);
        true
    }
}
