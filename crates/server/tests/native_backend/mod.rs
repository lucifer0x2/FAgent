//! The native backend these tests exercise: `vello` (native compute, `vello_cpu` PNG).
use fagent_server::RenderBackend;

pub fn native() -> RenderBackend {
    RenderBackend::Vello
}

/// The `RENDER_BACKEND` / `--render-backend` spelling of [`native`].
#[allow(dead_code)]
pub fn native_name() -> &'static str {
    native().name()
}
