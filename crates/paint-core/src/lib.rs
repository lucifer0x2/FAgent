//! Backend-neutral scene-3/web-4 paint contract of the native export (`fagent-paint-vello` on
//! `vello_cpu`), kept apart from any rasterizer: the wire scene,
//! validation, strict resource preparation (hash, decode with EXIF + ICC→sRGB, readiness,
//! `MISSING_GLYPH`), SVG resource inspection and straight-alpha PNG encoding.
//! Nothing here rasterizes geometry, touches the filesystem, fetches URLs or spawns threads.
mod api;
pub mod color;
mod encode;
mod icc;
mod images;
mod resources;
mod svg_resources;
mod tree;
mod validation;
mod wire;

pub use api::{DrawStats, PaintScene, Raster, RenderError, RenderOptions, ResourceReadiness};
pub use encode::encode_png;
pub use images::{decode_image, DecodedImage};
pub use resources::{Prepared, ResourceBytes, Resources};
pub use svg_resources::inspect_svg_resources;
pub use tree::SceneIndex;
pub use validation::validate;

/// Decode/inspect strict resources without allocating a target (e.g. an SVG export).
/// No geometry is painted; observations on a strict failure live in RenderError.readiness.
pub fn inspect_resources(
    scene: &PaintScene,
    resources: &Resources<'_>,
    options: &RenderOptions<'_>,
) -> Result<Vec<ResourceReadiness>, RenderError> {
    validation::resource_identities(scene, resources, options)?;
    Ok(resources.prepare(scene, options)?.readiness)
}
