//! Task 29 P0 web demo: one scene encoder (`fagent-paint-vello`) drawn by
//! `vello_gpu` through WebGPU (primary) or WebGL2 (fallback) on a canvas.
//! Main-thread only for P0; the engine Worker is Task 30.
#![cfg(target_arch = "wasm32")]

use fagent_paint_vello::vello_common::kurbo::{Affine, Rect};
use fagent_paint_vello::vello_common::paint::{ImageSource, TextureRegion};
use fagent_paint_vello::vello_common::TextureId;
use fagent_paint_vello::{
    encode, EncodeOptions, GpuPainter, GpuText, Image, PaintScene, Resources, Shadows,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

struct WebGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: vello_gpu::Renderer,
    resources: vello_gpu::Resources,
    bindings: vello_gpu::TextureBindings,
}

struct WebGl {
    renderer: vello_gpu::WebGlRenderer,
    resources: vello_gpu::Resources,
    bindings: vello_gpu::WebGlTextureBindings,
}

enum Backend {
    WebGpu(Box<WebGpu>),
    WebGl(Box<WebGl>),
}

#[wasm_bindgen]
pub struct Demo {
    backend: Backend,
    canvas: HtmlCanvasElement,
    scene: Option<PaintScene>,
    paint: Resources,
    gpu: vello_gpu::Scene,
    next_texture: u64,
    idle: Arc<AtomicBool>,
    text: GpuText,
    greek_below_px: f64,
    /// Device + shader initialization time, ms.
    init_ms: f64,
}

#[wasm_bindgen]
impl Demo {
    /// WebGPU via wgpu; rejects when the browser has no adapter or device.
    pub async fn webgpu(canvas: HtmlCanvasElement) -> Result<Demo, JsError> {
        let start = now();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(js_err)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(js_err)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(js_err)?;
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let caps = surface.get_capabilities(&adapter);
        // Vello writes sRGB-encoded values: use a non-sRGB view format.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .ok_or_else(|| JsError::new("no non-sRGB surface format"))?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::default(),
        };
        surface.configure(&device, &config);
        let (renderer, resources) = vello_gpu::Renderer::new(
            &device,
            &queue,
            &vello_gpu::RenderTargetConfig {
                format,
                width: width as u16,
                height: height as u16,
            },
        );
        let backend = Backend::WebGpu(Box::new(WebGpu {
            device,
            queue,
            surface,
            config,
            renderer,
            resources,
            bindings: vello_gpu::TextureBindings::new(),
        }));
        Ok(Self::with(backend, canvas, now() - start))
    }

    /// WebGL2 with Vello's GLSL shaders (blocking shader compile, timed).
    pub fn webgl2(canvas: HtmlCanvasElement) -> Result<Demo, JsError> {
        let start = now();
        let (renderer, resources) = vello_gpu::WebGlRenderer::new(&canvas).map_err(js_err)?;
        let backend = Backend::WebGl(Box::new(WebGl {
            renderer,
            resources,
            bindings: vello_gpu::WebGlTextureBindings::new(),
        }));
        Ok(Self::with(backend, canvas, now() - start))
    }

    fn with(backend: Backend, canvas: HtmlCanvasElement, init_ms: f64) -> Self {
        let (w, h) = (canvas.width().max(1) as u16, canvas.height().max(1) as u16);
        Self {
            backend,
            canvas,
            scene: None,
            paint: Resources::new(),
            gpu: vello_gpu::Scene::new(w, h),
            next_texture: 1,
            idle: Arc::new(AtomicBool::new(true)),
            text: GpuText::default(),
            greek_below_px: 0.0,
            init_ms,
        }
    }

    pub fn backend(&self) -> String {
        match self.backend {
            Backend::WebGpu(_) => "webgpu".into(),
            Backend::WebGl(_) => "webgl2".into(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn init_ms(&self) -> f64 {
        self.init_ms
    }

    /// Text path: "outlines" (default, same as vello_cpu), "glyph_run" or "atlas".
    pub fn set_text_mode(&mut self, mode: &str) -> Result<(), JsError> {
        self.text = match mode {
            "outlines" => GpuText::Outlines,
            "glyph_run" => GpuText::GlyphRun,
            "atlas" => GpuText::Atlas,
            other => return Err(JsError::new(&format!("unknown text mode {other}"))),
        };
        Ok(())
    }

    /// Text below this device-px font size is drawn as bars (level of detail); 0 = off.
    pub fn set_greek_below(&mut self, px: f64) {
        self.greek_below_px = px;
    }

    /// Replaces the scene; returns the node count.
    pub fn set_scene(&mut self, json: &str) -> Result<usize, JsError> {
        let scene: PaintScene = serde_json::from_str(json).map_err(js_err)?;
        let n = scene.nodes.len();
        self.scene = Some(scene);
        Ok(n)
    }

    /// `manifest` is the scene's font entry as JSON; bytes are hash-checked.
    pub fn add_font(&mut self, manifest: &str, bytes: &[u8]) -> Result<(), JsError> {
        let manifest = serde_json::from_str(manifest).map_err(js_err)?;
        self.paint.add_font(&manifest, bytes).map_err(js_err)
    }

    /// Straight-alpha RGBA8 (from `getImageData`), uploaded as a premultiplied texture.
    pub fn add_image(
        &mut self,
        id: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), JsError> {
        let (w, h) = (
            u16::try_from(width).map_err(js_err)?,
            u16::try_from(height).map_err(js_err)?,
        );
        if rgba.len() != width as usize * height as usize * 4 || w == 0 || h == 0 {
            return Err(JsError::new("image size mismatch"));
        }
        let mut premul = rgba.to_vec();
        for px in premul.chunks_exact_mut(4) {
            let a = u16::from(px[3]);
            for c in &mut px[..3] {
                *c = ((u16::from(*c) * a + 127) / 255) as u8;
            }
        }
        let id_tex = TextureId(self.next_texture);
        self.next_texture += 1;
        match &mut self.backend {
            Backend::WebGpu(g) => {
                let texture = g.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(id),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                g.queue.write_texture(
                    texture.as_image_copy(),
                    &premul,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: None,
                    },
                    texture.size(),
                );
                g.bindings
                    .insert(id_tex, texture.create_view(&Default::default()));
            }
            Backend::WebGl(g) => {
                let gl = g.renderer.gl_context();
                let texture = gl
                    .create_texture()
                    .ok_or_else(|| JsError::new("createTexture"))?;
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
                    Some(&premul),
                )
                .map_err(|e| JsError::new(&format!("texImage2D {e:?}")))?;
                for (k, v) in [
                    (Gl::TEXTURE_MIN_FILTER, Gl::LINEAR),
                    (Gl::TEXTURE_MAG_FILTER, Gl::LINEAR),
                    (Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE),
                    (Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE),
                ] {
                    gl.tex_parameteri(Gl::TEXTURE_2D, k, v as i32);
                }
                gl.bind_texture(Gl::TEXTURE_2D, None);
                g.bindings.insert(id_tex, texture, w, h);
            }
        }
        self.paint.add_image(
            id,
            Image {
                source: ImageSource::external_texture(id_tex, TextureRegion::Full, true),
                width,
                height,
            },
        );
        Ok(())
    }

    /// Matches the drawing surface to the canvas size (device px).
    pub fn resize(&mut self) {
        let (w, h) = (self.canvas.width().max(1), self.canvas.height().max(1));
        if let Backend::WebGpu(g) = &mut self.backend {
            if g.config.width != w || g.config.height != h {
                g.config.width = w;
                g.config.height = h;
                g.surface.configure(&g.device, &g.config);
            }
        }
        self.gpu.reset_and_resize(w as u16, h as u16);
    }

    /// Draws the scene with camera `translate(tx, ty) * scale(scale)`; `cull` skips
    /// subtrees outside the canvas. Returns
    /// `[encodeMs, submitMs, nodes, glyphs, layers, culled, greeked]`.
    pub fn render(
        &mut self,
        scale: f64,
        tx: f64,
        ty: f64,
        analytic_shadows: bool,
        cull: bool,
    ) -> Result<Vec<f64>, JsError> {
        let scene = self
            .scene
            .as_ref()
            .ok_or_else(|| JsError::new("no scene"))?;
        let t0 = now();
        self.gpu.reset();
        let camera = Affine::translate((tx, ty)) * Affine::scale(scale);
        let options = EncodeOptions {
            shadows: if analytic_shadows {
                Shadows::Analytic
            } else {
                Shadows::Filter
            },
            viewport: cull.then(|| {
                Rect::new(
                    0.0,
                    0.0,
                    f64::from(self.gpu.width()),
                    f64::from(self.gpu.height()),
                )
            }),
            greek_below_px: self.greek_below_px,
        };
        let resources = match &mut self.backend {
            Backend::WebGpu(g) => &mut g.resources,
            Backend::WebGl(g) => &mut g.resources,
        };
        let mut painter = GpuPainter {
            scene: &mut self.gpu,
            resources,
            text: self.text,
        };
        let stats = encode(&mut painter, scene, &mut self.paint, camera, options);
        let t1 = now();
        self.idle.store(false, Ordering::SeqCst);
        match &mut self.backend {
            Backend::WebGpu(g) => {
                let frame = match g.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(f)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                    other => return Err(JsError::new(&format!("surface: {other:?}"))),
                };
                let view = frame.texture.create_view(&Default::default());
                let mut encoder = g.device.create_command_encoder(&Default::default());
                g.renderer
                    .render(
                        &self.gpu,
                        &mut g.resources,
                        &mut encoder,
                        vello_gpu::RenderTarget {
                            view: &view,
                            depth_view: None,
                        },
                        &g.bindings,
                        vello_gpu::RenderOptions::default(),
                    )
                    .map_err(|e| JsError::new(&format!("render: {e:?}")))?;
                g.queue.submit([encoder.finish()]);
                let idle = self.idle.clone();
                g.queue
                    .on_submitted_work_done(move || idle.store(true, Ordering::SeqCst));
                g.queue.present(frame);
            }
            Backend::WebGl(g) => {
                g.renderer
                    .render(
                        &self.gpu,
                        &mut g.resources,
                        vello_gpu::WebGlRenderTarget::DrawingBuffer,
                        &g.bindings,
                        vello_gpu::WebGlRenderOptions::default(),
                    )
                    .map_err(|e| JsError::new(&format!("render: {e:?}")))?;
                self.idle.store(true, Ordering::SeqCst);
            }
        }
        let t2 = now();
        Ok(vec![
            t1 - t0,
            t2 - t1,
            stats.nodes as f64,
            stats.glyphs as f64,
            stats.layers as f64,
            stats.culled as f64,
            stats.greeked as f64,
        ])
    }

    /// WebGPU: whether the last submission finished on the GPU. WebGL2: blocks on a
    /// 1×1 `readPixels` (a synchronous GPU round trip; `finish()` alone returns early
    /// in Chromium) and returns true.
    pub fn gpu_idle(&self) -> bool {
        if let Backend::WebGl(g) = &self.backend {
            let gl = g.renderer.gl_context();
            let mut px = [0u8; 4];
            let _ = gl.read_pixels_with_opt_u8_array(
                0,
                0,
                1,
                1,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut px),
            );
            return true;
        }
        self.idle.load(Ordering::SeqCst)
    }
}
