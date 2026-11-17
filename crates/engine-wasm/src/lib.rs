//! FAgent viewer engine (Task 30): one module, owned by one Worker, that keeps the
//! accepted snapshot, computes frame scenes with the shared Rust core (`scene-wasm`)
//! and paints them with Vello (`paint-vello`) on an `OffscreenCanvas`: WebGPU first,
//! then WebGL2, then `vello_cpu` into an `ImageBitmap`. Scenes never leave the engine;
//! the page receives summaries, hit-test results and node bounds.
mod draw;
mod frame;
mod model;
#[cfg(target_arch = "wasm32")]
mod web;

pub use draw::{draw, DrawStats, View};
pub use frame::{AssetRef, NodeBounds, Placement, Summary};
pub use model::{Model, Result};
