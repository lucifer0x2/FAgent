//! Strict native PNG export with `vello_cpu`. The scene contract (validation, hash-checked
//! fonts, shared image decode with EXIF + ICC→sRGB, readiness, `MISSING_GLYPH`) comes from
//! `fagent-paint-core` (the contract the removed tiny-skia painter also used); drawing is the viewer's encoder
//! with export options fixed: glyph outlines, Gaussian filter shadows, no greeking, no culling.
use crate::encode::{encode_nodes, EncodeOptions, Prepared};
use crate::{pixmap_source, Resources, Shadows};
use fagent_paint_core::{
    validate, DrawStats, PaintScene, Raster, RenderError, RenderOptions, Resources as Bytes,
    SceneIndex,
};
use vello_common::kurbo::Affine;
use vello_common::peniko::ImageAlphaType;
use vello_cpu::{Pixmap, RenderContext, RenderSettings};

/// Provenance `engineVersion` of PNGs from [`render`].
pub const ENGINE_VERSION: &str = "fagent-paint-vello-0.1.0/vello_cpu-0.4.0";

/// Fixed for export: never the viewer's level-of-detail shortcuts.
const OPTIONS: EncodeOptions = EncodeOptions {
    shadows: Shadows::Filter,
    viewport: None,
    greek_below_px: 0.0,
};

/// Renders an already-computed scene with `vello_cpu`: the `fagent-paint-core` validation,
/// error codes, readiness and diagnostics, then the viewer's encoder.
/// Cancellation is checked between preparation, each text line, encoding and rasterization;
/// one rasterization pass is not interruptible.
pub fn render(
    scene: &PaintScene,
    resources: &Bytes<'_>,
    options: &RenderOptions<'_>,
) -> Result<Raster, RenderError> {
    let (width, height) = validate(scene, resources, options)?;
    // vello_cpu targets are u16-sized; the shared budget allows a 16M-pixel strip.
    let (w, h) = match (u16::try_from(width), u16::try_from(height)) {
        (Ok(w), Ok(h)) => (w, h),
        _ => return Err(RenderError::budget()),
    };
    options.check()?;
    let prepared = resources.prepare(scene, options)?;
    SceneIndex::new(&scene.nodes)?;
    let mut paint = Resources::new();
    for (id, image) in prepared.images {
        options.check()?;
        let image = pixmap_source(image.rgba, image.width, image.height).map_err(|_| {
            RenderError::new("RESOURCE_LIMIT", "Image exceeds the vello_cpu size limit")
        })?;
        paint.add_image(id, image);
    }
    for font in &scene.fonts {
        if let Some(face) = prepared.fonts.get(&font.id) {
            paint.insert_font(font.id.clone(), face.clone());
        }
    }
    check_text(scene, &mut paint, options)?;

    let mut ctx = RenderContext::new_with(w, h, RenderSettings::default());
    let stats = encode_nodes(
        &mut ctx,
        &scene.nodes,
        &Prepared::from_nodes(&scene.nodes),
        &mut paint,
        Affine::scale(options.scale),
        OPTIONS,
    );
    options.check()?;
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render(&mut pixmap, &mut vello_cpu::Resources::new());
    options.check()?;
    Ok(Raster {
        width,
        height,
        rgba: pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied),
        draw: DrawStats {
            drawn_lines: scene.nodes.iter().map(|n| n.lines.len()).sum(),
            opacity_layers: stats.opacity_layers,
            opacity_layer_fallbacks: 0,
        },
        source_scene_key: scene.provenance.source_scene_key.clone(),
        diagnostics: prepared.diagnostics,
        readiness: prepared.readiness,
    })
}

/// Every drawable text line shapes and outlines, or the export fails (`TEXT_SHAPING_FAILED` / `UNSUPPORTED_FEATURE`).
fn check_text(
    scene: &PaintScene,
    paint: &mut Resources,
    options: &RenderOptions<'_>,
) -> Result<(), RenderError> {
    for n in scene.nodes.iter().filter(|n| n.kind == "text") {
        let Some(font) = n.font_id.as_ref().and_then(|id| paint.fonts.get_mut(id)) else {
            continue; // a placeholder (non-strict) or already a readiness failure
        };
        for line in &n.lines {
            options.check()?;
            font.check_line(&line.text, n.font_size, n.letter_spacing)
                .map_err(|(code, message)| RenderError::new(code, message))?;
        }
    }
    Ok(())
}
