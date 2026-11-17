//! The WebGPU surface: wgpu device and canvas context for `vello_gpu`, with the health
//! signals the worker uses to leave WebGPU for the session: device loss, uncaptured GPU
//! errors (validation, out of memory, internal) and, through the present probe, a canvas
//! that shows nothing. Debug faults simulate those failures in browsers where WebGPU works.
use super::js_err;
use fagent_paint_vello::vello_common::kurbo::Rect;
use fagent_paint_vello::vello_common::paint::{ImageSource, TextureRegion};
use fagent_paint_vello::vello_common::peniko::Color;
use fagent_paint_vello::vello_common::TextureId;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use wasm_bindgen::JsError;
use web_sys::OffscreenCanvas;

/// Debug faults (`?gpuFault=` or `fagentEngine.gpuFault`), never set in normal use.
#[derive(Default)]
struct Faults {
    /// Every present leaves the canvas untouched (the silent blank frame).
    blank: bool,
    /// The next present fails like a broken render/present call.
    throw_next: bool,
}

pub(crate) struct WebGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: vello_gpu::Renderer,
    pub(crate) resources: vello_gpu::Resources,
    bindings: vello_gpu::TextureBindings,
    lost: Arc<AtomicBool>,
    idle: Arc<AtomicBool>,
    /// First uncaptured GPU error. wgpu's web backend installs no handler of its own, so
    /// without this one such errors only reach the console and frames silently go blank.
    error: Arc<Mutex<Option<String>>>,
    faults: Faults,
}

impl WebGpu {
    /// Adapter, device, configured canvas context and Vello renderer on `canvas`.
    pub(crate) async fn new(canvas: &OffscreenCanvas) -> Result<Self, JsError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas.clone()))
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
        let lost = Arc::new(AtomicBool::new(false));
        let flag = lost.clone();
        device.set_device_lost_callback(move |_, _| flag.store(true, Ordering::SeqCst));
        let error = Arc::new(Mutex::new(None));
        let slot = error.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            if let Ok(mut first) = slot.lock() {
                first.get_or_insert_with(|| e.to_string());
            }
        }));
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let caps = surface.get_capabilities(&adapter);
        // Vello writes sRGB-encoded values: a non-sRGB view format, probe-readable. wgpu
        // lists the browser's preferred canvas format first.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .ok_or_else(|| JsError::new("no 8-bit non-sRGB surface format"))?;
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
        Ok(Self {
            device,
            queue,
            surface,
            config,
            renderer,
            resources,
            bindings: vello_gpu::TextureBindings::new(),
            lost,
            idle: Arc::new(AtomicBool::new(true)),
            error,
            faults: Faults::default(),
        })
    }

    pub(crate) fn renderer(&mut self) -> &mut vello_gpu::Renderer {
        &mut self.renderer
    }

    pub(crate) fn lost(&self) -> bool {
        self.lost.load(Ordering::SeqCst)
    }

    pub(crate) fn idle(&self) -> bool {
        self.idle.load(Ordering::SeqCst)
    }

    pub(crate) fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }

    pub(crate) fn destroy(&self) {
        self.device.destroy();
    }

    /// The device's `maxTextureDimension2D`: the largest canvas this surface can present.
    pub(crate) fn max_texture_dimension(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    /// Reconfigures the canvas context when its size changed.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        if self.config.width != width || self.config.height != height {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Premultiplied RGBA8 as a sampled texture bound to `id`.
    pub(crate) fn upload(
        &mut self,
        id: TextureId,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> ImageSource {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
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
        self.queue.write_texture(
            texture.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: None,
            },
            texture.size(),
        );
        self.bindings
            .insert(id, texture.create_view(&Default::default()));
        ImageSource::external_texture(id, TextureRegion::Full, true)
    }

    /// Renders `scene` into the canvas's current texture and submits it.
    pub(crate) fn present(&mut self, scene: &vello_gpu::Scene) -> Result<(), String> {
        if self.lost() {
            return Err("WebGPU device lost".into());
        }
        if std::mem::take(&mut self.faults.throw_next) {
            return Err("render: simulated WebGPU render failure".into());
        }
        if self.faults.blank {
            return Ok(());
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => return Err(format!("surface: {other:?}")),
        };
        let target = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.renderer
            .render(
                scene,
                &mut self.resources,
                &mut encoder,
                vello_gpu::RenderTarget {
                    view: &target,
                    depth_view: None,
                },
                &self.bindings,
                vello_gpu::RenderOptions::default(),
            )
            .map_err(|e| format!("render: {e:?}"))?;
        self.idle.store(false, Ordering::SeqCst);
        self.queue.submit([encoder.finish()]);
        let idle = self.idle.clone();
        self.queue
            .on_submitted_work_done(move || idle.store(true, Ordering::SeqCst));
        self.queue.present(frame);
        Ok(())
    }

    /// Present probe: fills the whole canvas with opaque `rgb` through the same path as
    /// a board render. The worker reads it back from the canvas's `ImageBitmap`.
    pub(crate) fn present_fill(
        &mut self,
        scene: &mut vello_gpu::Scene,
        rgb: u32,
    ) -> Result<(), String> {
        let (w, h) = (self.config.width as u16, self.config.height as u16);
        scene.reset_and_resize(w, h);
        let [_, r, g, b] = rgb.to_be_bytes();
        scene.set_paint(Color::from_rgb8(r, g, b));
        scene.fill_rect(&Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        self.present(scene)
    }

    /// Debug fault: "blank" (presents leave the canvas empty), "throw" (the next
    /// present fails) or "error" (a real WebGPU validation error, reported uncaptured).
    pub(crate) fn simulate(&mut self, fault: &str) -> Result<(), String> {
        match fault {
            "blank" => self.faults.blank = true,
            "throw" => self.faults.throw_next = true,
            // MAP_READ combines only with COPY_DST: the browser rejects this buffer.
            "error" => drop(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("simulated GPU fault"),
                size: 4,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })),
            other => return Err(format!("unknown GPU fault {other}")),
        }
        Ok(())
    }
}
