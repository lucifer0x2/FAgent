//! The three drawing surfaces on one `OffscreenCanvas` (a canvas keeps its first context
//! type, so each fallback step gets a fresh canvas from the page): `vello_gpu` through
//! WebGPU (wgpu) or WebGL2, and `vello_cpu` whose pixels reach the canvas as an
//! `ImageBitmap`. Each GPU backend runs Vello's renderer probe before it is trusted;
//! WebGPU also runs the present probe (see `webgpu.rs`).
use super::webgpu::WebGpu;
use super::{js_err, now};
use crate::draw::{draw, DrawStats, View};
use crate::model::Model;
use fagent_paint_vello::vello_common::paint::{ImageSource, TextureRegion};
use fagent_paint_vello::vello_common::peniko::ImageAlphaType;
use fagent_paint_vello::vello_common::pixmap::Pixmap;
use fagent_paint_vello::vello_common::TextureId;
use fagent_paint_vello::{pixmap_source, GpuPainter, GpuText, Image};
use vello_gpu::ProbeFeature;
use wasm_bindgen::prelude::*;
use wasm_bindgen::Clamped;
use web_sys::{
    HtmlCanvasElement, ImageBitmapRenderingContext, ImageData, OffscreenCanvas,
    OffscreenCanvasRenderingContext2d, WebGl2RenderingContext as Gl,
};

/// What scene-3 needs from the renderer (no sweep gradients or bicubic images).
const PROBE: &[ProbeFeature] = &[
    ProbeFeature::SolidRect,
    ProbeFeature::AlphaBlending,
    ProbeFeature::LinearGradient,
    ProbeFeature::RadialGradient,
    ProbeFeature::ImageBilinear,
    ProbeFeature::OpacityLayer,
    ProbeFeature::Filter,
    ProbeFeature::Transformed,
    ProbeFeature::BlurredRoundedRect,
];

pub(crate) struct WebGl {
    renderer: vello_gpu::WebGlRenderer,
    resources: vello_gpu::Resources,
    bindings: vello_gpu::WebGlTextureBindings,
}

pub(crate) struct Cpu {
    output: ImageBitmapRenderingContext,
    staging: OffscreenCanvas,
    staging_ctx: OffscreenCanvasRenderingContext2d,
    resources: vello_cpu::Resources,
}

pub(crate) enum Backend {
    WebGpu(Box<WebGpu>),
    WebGl(Box<WebGl>),
    Cpu(Box<Cpu>),
}

enum PendingProbe {
    WebGpu(vello_gpu::WgpuPendingProbe),
    WebGl(vello_gpu::WebGlPendingProbe),
}

/// A drawing surface bound to one canvas, created before it is handed to the engine.
#[wasm_bindgen]
pub struct Surface {
    pub(crate) backend: Backend,
    canvas: OffscreenCanvas,
    next_texture: u64,
    probe: Option<PendingProbe>,
    probe_result: Option<Result<(), String>>,
    init_ms: f64,
}

#[wasm_bindgen]
impl Surface {
    /// WebGPU via wgpu; rejects without an adapter or device.
    pub async fn webgpu(canvas: OffscreenCanvas) -> Result<Surface, JsError> {
        let start = now();
        let gpu = WebGpu::new(&canvas).await?;
        Ok(Self::with(Backend::WebGpu(Box::new(gpu)), canvas, start))
    }

    /// WebGL2 with Vello's GLSL shaders (`failIfMajorPerformanceCaveat`: no software GL).
    pub fn webgl2(canvas: OffscreenCanvas) -> Result<Surface, JsError> {
        let start = now();
        // vello_gpu only calls `getContext` on it, which OffscreenCanvas implements.
        let element: &HtmlCanvasElement = canvas.unchecked_ref();
        let (renderer, resources) = vello_gpu::WebGlRenderer::new(element).map_err(js_err)?;
        let backend = Backend::WebGl(Box::new(WebGl {
            renderer,
            resources,
            bindings: vello_gpu::WebGlTextureBindings::new(),
        }));
        Ok(Self::with(backend, canvas, start))
    }

    /// `vello_cpu` into a staging canvas, shown with `transferFromImageBitmap`.
    pub fn cpu(canvas: OffscreenCanvas) -> Result<Surface, JsError> {
        let start = now();
        let output = canvas
            .get_context("bitmaprenderer")
            .map_err(|e| JsError::new(&format!("bitmaprenderer: {e:?}")))?
            .ok_or_else(|| JsError::new("bitmaprenderer unavailable"))?
            .unchecked_into::<ImageBitmapRenderingContext>();
        let staging = OffscreenCanvas::new(canvas.width().max(1), canvas.height().max(1))
            .map_err(|e| JsError::new(&format!("OffscreenCanvas: {e:?}")))?;
        let staging_ctx = staging
            .get_context("2d")
            .map_err(|e| JsError::new(&format!("2d: {e:?}")))?
            .ok_or_else(|| JsError::new("2d context unavailable"))?
            .unchecked_into::<OffscreenCanvasRenderingContext2d>();
        let backend = Backend::Cpu(Box::new(Cpu {
            output,
            staging,
            staging_ctx,
            resources: vello_cpu::Resources::new(),
        }));
        let mut surface = Self::with(backend, canvas, start);
        surface.probe_result = Some(Ok(()));
        Ok(surface)
    }

    fn with(backend: Backend, canvas: OffscreenCanvas, start: f64) -> Self {
        Self {
            backend,
            canvas,
            next_texture: 1,
            probe: None,
            probe_result: None,
            init_ms: now() - start,
        }
    }

    pub fn kind(&self) -> String {
        match self.backend {
            Backend::WebGpu(_) => "webgpu",
            Backend::WebGl(_) => "webgl2",
            Backend::Cpu(_) => "cpu",
        }
        .into()
    }

    #[wasm_bindgen(getter)]
    pub fn init_ms(&self) -> f64 {
        self.init_ms
    }
}

impl Surface {
    /// Starts Vello's renderer probe (draws reference cells and reads them back).
    pub(crate) fn start_probe(&mut self) -> Result<(), String> {
        if self.probe_result.is_some() || self.probe.is_some() {
            return Ok(());
        }
        self.probe = Some(match &mut self.backend {
            Backend::WebGpu(g) => PendingProbe::WebGpu(
                g.renderer()
                    .probe(PROBE, false)
                    .map_err(|e| format!("probe: {e}"))?,
            ),
            Backend::WebGl(g) => {
                PendingProbe::WebGl(g.renderer.probe(PROBE).map_err(|e| format!("probe: {e}"))?)
            }
            Backend::Cpu(_) => return Ok(()),
        });
        Ok(())
    }

    /// `None` while the readback is pending.
    pub(crate) fn poll_probe(&mut self) -> Option<Result<(), String>> {
        if let Some(result) = &self.probe_result {
            return Some(result.clone());
        }
        let done = match self.probe.take()? {
            PendingProbe::WebGpu(p) => match p.try_finish() {
                Ok(vello_gpu::WgpuProbeStatus::Pending(p)) => {
                    self.probe = Some(PendingProbe::WebGpu(p));
                    return None;
                }
                Ok(vello_gpu::WgpuProbeStatus::Complete(probe)) => probe.is_success(),
                Err(e) => return Some(self.finish_probe(Err(format!("probe: {e}")))),
            },
            PendingProbe::WebGl(p) => match p.try_finish() {
                Ok(vello_gpu::WebGlProbeStatus::Pending(p)) => {
                    self.probe = Some(PendingProbe::WebGl(p));
                    return None;
                }
                Ok(vello_gpu::WebGlProbeStatus::Complete(probe)) => probe.is_success(),
                Err(e) => return Some(self.finish_probe(Err(format!("probe: {e}")))),
            },
        };
        let result = if done {
            Ok(())
        } else {
            Err("renderer probe output does not match the reference".into())
        };
        Some(self.finish_probe(result))
    }

    fn finish_probe(&mut self, result: Result<(), String>) -> Result<(), String> {
        self.probe_result = Some(result.clone());
        result
    }

    pub(crate) fn device_lost(&self) -> bool {
        matches!(&self.backend, Backend::WebGpu(g) if g.lost())
    }

    /// Test hook: destroys the WebGPU device so the loss path runs.
    pub(crate) fn lose_device(&self) {
        if let Backend::WebGpu(g) = &self.backend {
            g.destroy();
        }
    }

    /// The first uncaptured WebGPU error (validation, out of memory, internal), if any.
    pub(crate) fn gpu_error(&self) -> Option<String> {
        match &self.backend {
            Backend::WebGpu(g) => g.error(),
            _ => None,
        }
    }

    /// WebGPU present probe: shrinks the canvas to `size` × `size` and fills it with
    /// opaque `rgb` (0xRRGGBB) through the board's present path. Other backends: no-op.
    pub(crate) fn present_probe(
        &mut self,
        size: u32,
        rgb: u32,
        scene: &mut vello_gpu::Scene,
    ) -> Result<(), String> {
        if !matches!(self.backend, Backend::WebGpu(_)) {
            return Ok(());
        }
        self.resize(size, size);
        match &mut self.backend {
            Backend::WebGpu(g) => g.present_fill(scene, rgb),
            _ => Ok(()),
        }
    }

    /// Debug fault on the WebGPU backend (see `WebGpu::simulate`).
    pub(crate) fn simulate_fault(&mut self, fault: &str) -> Result<(), String> {
        match &mut self.backend {
            Backend::WebGpu(g) => g.simulate(fault),
            _ => Err("GPU faults apply to WebGPU only".into()),
        }
    }

    /// Largest canvas side this backend can draw, device px; 0 when unknown (vello_cpu).
    /// WebGPU: the device's `maxTextureDimension2D`. WebGL2: `MAX_TEXTURE_SIZE`, capped by
    /// `MAX_RENDERBUFFER_SIZE` (the drawing buffer is one).
    pub(crate) fn max_texture_dimension(&self) -> u32 {
        match &self.backend {
            Backend::WebGpu(g) => g.max_texture_dimension(),
            Backend::WebGl(g) => {
                let gl = g.renderer.gl_context();
                let limit = |name| {
                    gl.get_parameter(name)
                        .ok()
                        .and_then(|v| v.as_f64())
                        .map_or(0, |v| v as u32)
                };
                [Gl::MAX_TEXTURE_SIZE, Gl::MAX_RENDERBUFFER_SIZE]
                    .map(limit)
                    .into_iter()
                    .filter(|&v| v > 0)
                    .min()
                    .unwrap_or(0)
            }
            Backend::Cpu(_) => 0,
        }
    }

    /// Matches the canvas (and WebGPU surface) to `width` × `height` device px.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        let (w, h) = (width.max(1), height.max(1));
        if self.canvas.width() != w {
            self.canvas.set_width(w);
        }
        if self.canvas.height() != h {
            self.canvas.set_height(h);
        }
        match &mut self.backend {
            Backend::WebGpu(g) => g.resize(w, h),
            Backend::Cpu(c) => {
                if c.staging.width() != w {
                    c.staging.set_width(w);
                }
                if c.staging.height() != h {
                    c.staging.set_height(h);
                }
            }
            _ => {}
        }
    }

    /// Uploads premultiplied RGBA8 as an image the encoder can draw on this backend.
    pub(crate) fn upload(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<Image, String> {
        let id = TextureId(self.next_texture);
        self.next_texture += 1;
        let source = match &mut self.backend {
            Backend::Cpu(_) => return pixmap_source(rgba.to_vec(), width, height),
            Backend::WebGpu(g) => g.upload(id, rgba, width, height),
            Backend::WebGl(g) => {
                let (w, h) = (
                    u16::try_from(width).map_err(|_| "image too wide")?,
                    u16::try_from(height).map_err(|_| "image too tall")?,
                );
                let gl = g.renderer.gl_context();
                let texture = gl.create_texture().ok_or("createTexture failed")?;
                gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
                gl.pixel_storei(Gl::UNPACK_ALIGNMENT, 1);
                gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                    Gl::TEXTURE_2D,
                    0,
                    Gl::RGBA8 as i32,
                    width as i32,
                    height as i32,
                    0,
                    Gl::RGBA,
                    Gl::UNSIGNED_BYTE,
                    Some(rgba),
                )
                .map_err(|e| format!("texImage2D {e:?}"))?;
                for (k, v) in [
                    (Gl::TEXTURE_MIN_FILTER, Gl::LINEAR),
                    (Gl::TEXTURE_MAG_FILTER, Gl::LINEAR),
                    (Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE),
                    (Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE),
                ] {
                    gl.tex_parameteri(Gl::TEXTURE_2D, k, v as i32);
                }
                gl.bind_texture(Gl::TEXTURE_2D, None);
                g.bindings.insert(id, texture, w, h);
                ImageSource::external_texture(id, TextureRegion::Full, true)
            }
        };
        Ok(Image {
            source,
            width,
            height,
        })
    }

    /// Encodes the board and submits it. Returns the stats with encode/submit ms.
    pub(crate) fn render(
        &mut self,
        model: &mut Model,
        view: &View,
        text: GpuText,
        scene: &mut vello_gpu::Scene,
    ) -> Result<(DrawStats, f64, f64), String> {
        let (w, h) = (self.canvas.width().max(1), self.canvas.height().max(1));
        let t0 = now();
        match &mut self.backend {
            Backend::Cpu(c) => {
                let (w16, h16) = (
                    u16::try_from(w).map_err(|_| "canvas too wide")?,
                    u16::try_from(h).map_err(|_| "canvas too tall")?,
                );
                let mut ctx = vello_cpu::RenderContext::new(w16, h16);
                let stats = draw(&mut ctx, model, view);
                let t1 = now();
                let mut pixmap = Pixmap::new(w16, h16);
                ctx.flush();
                ctx.render(&mut pixmap, &mut c.resources);
                let rgba = pixmap.take_rgba8(ImageAlphaType::Alpha);
                let data = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&rgba), w, h)
                    .map_err(|e| format!("ImageData: {e:?}"))?;
                c.staging_ctx
                    .put_image_data(&data, 0.0, 0.0)
                    .map_err(|e| format!("putImageData: {e:?}"))?;
                let bitmap = c
                    .staging
                    .transfer_to_image_bitmap()
                    .map_err(|e| format!("transferToImageBitmap: {e:?}"))?;
                c.output.transfer_from_image_bitmap(&bitmap);
                Ok((stats, t1 - t0, now() - t1))
            }
            Backend::WebGpu(g) => {
                if g.lost() {
                    return Err("WebGPU device lost".into());
                }
                scene.reset_and_resize(w as u16, h as u16);
                let mut painter = GpuPainter {
                    scene,
                    resources: &mut g.resources,
                    text,
                };
                let stats = draw(&mut painter, model, view);
                let t1 = now();
                g.present(scene)?;
                Ok((stats, t1 - t0, now() - t1))
            }
            Backend::WebGl(g) => {
                scene.reset_and_resize(w as u16, h as u16);
                let mut painter = GpuPainter {
                    scene,
                    resources: &mut g.resources,
                    text,
                };
                let stats = draw(&mut painter, model, view);
                let t1 = now();
                g.renderer
                    .render(
                        scene,
                        &mut g.resources,
                        vello_gpu::WebGlRenderTarget::DrawingBuffer,
                        &g.bindings,
                        vello_gpu::WebGlRenderOptions::default(),
                    )
                    .map_err(|e| format!("render: {e:?}"))?;
                Ok((stats, t1 - t0, now() - t1))
            }
        }
    }

    /// WebGPU: whether the last submission finished. WebGL2: a 1×1 `readPixels` round
    /// trip (blocking). CPU: always.
    pub(crate) fn idle(&self) -> bool {
        match &self.backend {
            Backend::WebGpu(g) => g.idle(),
            Backend::WebGl(g) => {
                let mut px = [0u8; 4];
                let _ = g.renderer.gl_context().read_pixels_with_opt_u8_array(
                    0,
                    0,
                    1,
                    1,
                    Gl::RGBA,
                    Gl::UNSIGNED_BYTE,
                    Some(&mut px),
                );
                true
            }
            Backend::Cpu(_) => true,
        }
    }
}

/// Premultiplies straight RGBA8 (from `getImageData`) in place.
pub(crate) fn premultiply(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = u16::from(px[3]);
        for c in &mut px[..3] {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
}
