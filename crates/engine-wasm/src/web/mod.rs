//! wasm-bindgen API of the engine, owned by exactly one dedicated Worker. JSON crosses
//! the boundary only for small values (summaries, keys, bounds, stats); the snapshot is
//! parsed once per revision and scenes stay in Rust.
mod backend;
mod webgpu;

use crate::draw::View;
use crate::frame::Placement;
use crate::model::Model;
use backend::{premultiply, Surface};
use fagent_paint_vello::GpuText;
use fagent_scene_wasm::StorageError;
use serde::Serialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

pub(crate) fn now() -> f64 {
    js_sys::Reflect::get(&js_sys::global(), &"performance".into())
        .ok()
        .map(|p| p.unchecked_into::<web_sys::Performance>())
        .map_or(0.0, |p| p.now())
}

pub(crate) fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// `{code, message}` as a plain object: survives structured clone to the page.
fn storage_err(error: StorageError) -> JsValue {
    serde_json::to_string(&error)
        .ok()
        .and_then(|json| js_sys::JSON::parse(&json).ok())
        .unwrap_or_else(|| JsValue::from_str("Engine error"))
}

fn json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

/// Decoded image kept so a backend switch can re-upload it.
struct RawImage {
    hash: String,
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenderReport {
    #[serde(flatten)]
    stats: crate::draw::DrawStats,
    encode_ms: f64,
    submit_ms: f64,
    width: f64,
    height: f64,
    scale: f64,
}

#[wasm_bindgen]
pub struct Engine {
    model: Model,
    surface: Option<Surface>,
    scene: vello_gpu::Scene,
    view: View,
    text: GpuText,
    images: HashMap<String, RawImage>,
    last: Option<String>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        // A panic aborts the module; log its message so a browser console shows why.
        std::panic::set_hook(Box::new(|info| {
            web_sys::console::error_1(&format!("fagent engine panic: {info}").into());
        }));
        Self {
            model: Model::new(),
            surface: None,
            scene: vello_gpu::Scene::new(1, 1),
            view: View::default(),
            text: GpuText::Outlines,
            images: HashMap::new(),
            last: None,
        }
    }

    /// Binds a drawing surface (replacing any previous one) and re-uploads every image.
    pub fn attach(&mut self, mut surface: Surface) -> Result<(), JsError> {
        surface.resize(self.view.width as u32, self.view.height as u32);
        for (id, image) in &self.images {
            let uploaded = surface
                .upload(&image.rgba, image.width, image.height)
                .map_err(|e| JsError::new(&e))?;
            self.model.add_image(id, uploaded);
        }
        self.surface = Some(surface);
        Ok(())
    }

    pub fn detach(&mut self) {
        self.surface = None;
    }

    /// "webgpu" | "webgl2" | "cpu" | "" (no surface yet).
    pub fn backend(&self) -> String {
        self.surface
            .as_ref()
            .map_or_else(String::new, Surface::kind)
    }

    pub fn start_probe(&mut self) -> Result<(), JsError> {
        let surface = self
            .surface
            .as_mut()
            .ok_or_else(|| JsError::new("no surface"))?;
        surface.start_probe().map_err(|e| JsError::new(&e))
    }

    /// "pending", "ok" or "failed: <reason>".
    pub fn probe_status(&mut self) -> String {
        match self.surface.as_mut().map(Surface::poll_probe) {
            None => "failed: no surface".into(),
            Some(None) => "pending".into(),
            Some(Some(Ok(()))) => "ok".into(),
            Some(Some(Err(e))) => format!("failed: {e}"),
        }
    }

    pub fn device_lost(&self) -> bool {
        self.surface.as_ref().is_some_and(Surface::device_lost)
    }

    /// Largest canvas side the attached backend can draw, device px; 0 when unknown (no
    /// surface, vello_cpu). The worker sizes its overscan margin to fit it.
    pub fn max_texture_size(&self) -> u32 {
        self.surface
            .as_ref()
            .map_or(0, Surface::max_texture_dimension)
    }

    /// Test hook: destroys the WebGPU device.
    pub fn lose_device(&self) {
        if let Some(s) = &self.surface {
            s.lose_device();
        }
    }

    /// The first uncaptured WebGPU error since this surface was attached, if any.
    pub fn gpu_error(&self) -> Option<String> {
        self.surface.as_ref().and_then(Surface::gpu_error)
    }

    /// WebGPU present probe frame: the canvas, `size` px square, filled with opaque
    /// `rgb` (0xRRGGBB). `end_present_probe` restores the camera size.
    pub fn present_probe(&mut self, size: u32, rgb: u32) -> Result<(), JsError> {
        let surface = self
            .surface
            .as_mut()
            .ok_or_else(|| JsError::new("no surface"))?;
        surface
            .present_probe(size, rgb, &mut self.scene)
            .map_err(|e| JsError::new(&e))
    }

    pub fn end_present_probe(&mut self) {
        if let Some(surface) = &mut self.surface {
            surface.resize(self.view.width as u32, self.view.height as u32);
        }
    }

    /// Debug hook: "blank", "throw" or "error" on the WebGPU backend.
    pub fn simulate_gpu_fault(&mut self, fault: &str) -> Result<(), JsError> {
        let surface = self
            .surface
            .as_mut()
            .ok_or_else(|| JsError::new("no surface"))?;
        surface.simulate_fault(fault).map_err(|e| JsError::new(&e))
    }

    /// "outlines" (default, same glyphs as vello_cpu) or "atlas" (experimental glyph atlas).
    pub fn set_text_mode(&mut self, mode: &str) -> Result<(), JsError> {
        self.text = match mode {
            "outlines" => GpuText::Outlines,
            "atlas" => GpuText::Atlas,
            other => return Err(JsError::new(&format!("unknown text mode {other}"))),
        };
        Ok(())
    }

    pub fn load_snapshot(&mut self, snapshot_json: &str) -> Result<(), JsValue> {
        self.model.load_snapshot(snapshot_json).map_err(storage_err)
    }

    pub fn frame_keys(&self, revision: f64, frames_json: &str) -> Result<String, JsValue> {
        self.model
            .frame_keys(revision as u64, frames_json)
            .map_err(storage_err)
    }

    pub fn required_fonts(
        &self,
        revision: f64,
        frame_id: &str,
        width: f64,
        height: f64,
    ) -> Result<String, JsValue> {
        self.model
            .required_fonts(revision as u64, frame_id, width, height)
            .map_err(storage_err)
    }

    pub fn has_font(&self, id: &str, hash: &str) -> bool {
        self.model.has_font(id, hash)
    }

    pub fn load_font(&mut self, id: &str, hash: &str, bytes: &[u8]) -> Result<(), JsValue> {
        self.model.load_font(id, hash, bytes).map_err(storage_err)
    }

    /// Computes into staging; returns the summary JSON. `commit` shows it.
    pub fn compute(
        &mut self,
        revision: f64,
        frame_id: &str,
        width: f64,
        height: f64,
        digest: &str,
    ) -> Result<String, JsValue> {
        self.model
            .compute(revision as u64, frame_id, width, height, digest)
            .map(|s| json(&s))
            .map_err(storage_err)
    }

    /// Stages a precomputed scene-3 JSON (parity/benchmark harnesses); `commit` shows it.
    pub fn insert_scene(&mut self, scene_json: &str) -> Result<String, JsValue> {
        self.model
            .insert_scene(scene_json)
            .map(|s| json(&s))
            .map_err(storage_err)
    }

    /// Level of detail and backdrop: text below `greek_below_px` device px as bars (0 =
    /// never), analytic shadows below device scale `analytic_shadows_below` (0 = never),
    /// and the viewer's white frame box (off for exact scene captures).
    pub fn set_draw_options(
        &mut self,
        greek_below_px: f64,
        analytic_shadows_below: f64,
        backdrop: bool,
    ) {
        self.view.greek_below_px = greek_below_px;
        self.view.analytic_shadows_below = analytic_shadows_below;
        self.view.backdrop = backdrop;
    }

    pub fn has_image(&self, id: &str, hash: &str) -> bool {
        self.images.get(id).is_some_and(|i| i.hash == hash)
    }

    /// Straight RGBA8 (from `getImageData`), premultiplied and uploaded to the surface.
    pub fn add_image(
        &mut self,
        id: &str,
        hash: &str,
        mut rgba: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<(), JsError> {
        if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
            return Err(JsError::new("image size mismatch"));
        }
        premultiply(&mut rgba);
        if let Some(surface) = &mut self.surface {
            let image = surface
                .upload(&rgba, width, height)
                .map_err(|e| JsError::new(&e))?;
            self.model.add_image(id, image);
        }
        self.images.insert(
            id.to_owned(),
            RawImage {
                hash: hash.to_owned(),
                rgba,
                width,
                height,
            },
        );
        Ok(())
    }

    pub fn commit(&mut self, frame_id: &str) -> bool {
        self.model.commit(frame_id)
    }

    pub fn discard(&mut self, frame_id: &str) {
        self.model.discard(frame_id);
    }

    pub fn reuse(&mut self, frame_id: &str, digest: &str, revision: f64) -> bool {
        self.model.reuse(frame_id, digest, revision as u64)
    }

    /// `[{id, x, y, width, height}]` in board order; unplaced frames are released.
    pub fn set_layout(&mut self, layout_json: &str) -> Result<(), JsError> {
        let layout: Vec<Placement> = serde_json::from_str(layout_json).map_err(js_err)?;
        self.model.set_layout(layout);
        Ok(())
    }

    /// Camera in device px: `translate(tx, ty) · scale(scale)`, canvas `width` × `height`.
    pub fn set_camera(&mut self, scale: f64, tx: f64, ty: f64, width: f64, height: f64) {
        self.view = View {
            scale,
            tx,
            ty,
            width: width.max(1.0).round(),
            height: height.max(1.0).round(),
            ..self.view
        };
        if let Some(surface) = &mut self.surface {
            surface.resize(self.view.width as u32, self.view.height as u32);
        }
    }

    /// Draws the board; returns stats JSON (encodeMs, submitMs, frames, nodes, …).
    pub fn render(&mut self) -> Result<String, JsError> {
        let surface = self
            .surface
            .as_mut()
            .ok_or_else(|| JsError::new("no surface"))?;
        let (stats, encode_ms, submit_ms) = surface
            .render(&mut self.model, &self.view, self.text, &mut self.scene)
            .map_err(|e| JsError::new(&e))?;
        let report = json(&RenderReport {
            stats,
            encode_ms,
            submit_ms,
            width: self.view.width,
            height: self.view.height,
            scale: self.view.scale,
        });
        self.last = Some(report.clone());
        Ok(report)
    }

    pub fn gpu_idle(&self) -> bool {
        self.surface.as_ref().is_none_or(Surface::idle)
    }

    /// The node under board point (`x`, `y`) among frames current at `revision`, with
    /// its frame-local bounds (`{frameId, id, kind, x, y, width, height, lines}`), or `null`.
    pub fn hit_test(&self, x: f64, y: f64, revision: f64) -> String {
        json(&self.model.hit_test(x, y, revision as u64))
    }

    /// Frame-local bounds of `ids` (JSON array) in `frame_id` ("" = every placed frame);
    /// a negative `revision` accepts whatever each frame shows (selection overlay).
    pub fn bounds(&self, frame_id: &str, revision: f64, ids_json: &str) -> String {
        let ids: Vec<String> = serde_json::from_str(ids_json).unwrap_or_default();
        let revision = (revision >= 0.0).then_some(revision as u64);
        json(&self.model.bounds(frame_id, revision, &ids))
    }

    /// `{backend, frames: [{id, revision, nodes}], lastRender}` for diagnostics.
    pub fn frame_status(&self) -> String {
        let frames: Vec<_> = self
            .model
            .placed()
            .map(|f| {
                serde_json::json!({
                    "id": f.id(), "revision": f.revision, "nodes": f.summary.node_count
                })
            })
            .collect();
        let last: serde_json::Value = self
            .last
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Null);
        json(&serde_json::json!({
            "backend": self.backend(),
            "frames": frames,
            "lastRender": last,
        }))
    }
}
