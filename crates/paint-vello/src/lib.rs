//! scene-3 recorded for Vello (Task 29 P0, Task 34 export). One encoder serves both
//! `vello_cpu` (export, parity reference, no-GPU fallback) and `vello_gpu`
//! (WebGPU with WebGL2 fallback), through the small [`Painter`] trait.
mod color;
mod cull;
mod encode;
#[cfg(feature = "export")]
pub mod export;
mod geometry;
#[cfg(feature = "gpu")]
mod gpu;
mod painter;
mod resources;
pub mod shadow;
mod stroke;
mod text;

pub use encode::{
    encode, encode_nodes, encode_prepared, EncodeOptions, EncodeStats, PaintScene, Prepared,
    Provenance, Viewport,
};
#[cfg(feature = "gpu")]
pub use gpu::{GpuPainter, GpuText};
pub use painter::{Glyph, GlyphRun, Painter};
#[cfg(feature = "cpu")]
pub use resources::pixmap_source;
pub use resources::{Image, Resources};
pub use shadow::Shadows;
pub use vello_common;
#[cfg(all(test, feature = "export"))]
mod export_tests;
#[cfg(all(test, feature = "cpu"))]
mod tests;
