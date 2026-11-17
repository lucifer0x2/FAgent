//! Snapshot → resolved scene. No storage, network, paint, clock or browser APIs.
#[cfg(all(target_arch = "wasm32", feature = "bindings"))]
mod bindings;
mod content_key;
mod engine;

pub use content_key::{
    frame_keys, frame_keys_json, frame_keys_resolved, FrameKey, FrameKeyRequest,
};
pub use engine::{ResourceUrls, SceneEngine};
pub use fagent_scene_input::{ResolvedNodes, SceneOptions, Snapshot, StorageError};
