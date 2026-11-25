//! The renderer. It knows nothing about the game: callers describe a frame
//! as lit shapes and ground wedges (`draw`) seen through a `View`. It builds
//! for wasm32 (WebGPU, else WebGL2) and natively for headless PNGs.
//!
//! - `draw`    the draw model, plus the avatar figure.
//! - `camera`  follow and orbit cameras.
//! - `mesh`    unit meshes for each shape.
//! - `render`  the GPU passes. Draws into any texture view.

use std::fmt;

pub mod camera;
pub mod draw;
mod mesh;
mod render;

#[cfg(not(target_arch = "wasm32"))]
mod headless;
#[cfg(not(target_arch = "wasm32"))]
pub use headless::{Offscreen, TARGET_FORMAT, write_png};

pub use camera::{FollowCamera, OrbitCamera, View};
pub use draw::{
    AVATAR_HEIGHT, Environment, Instance, SceneDraw, Shape, Wedge, avatar_instances, facing,
};
pub use render::{Output, Renderer};

/// A configured device and surface, ready to draw a frame.
#[derive(Debug)]
pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

#[derive(Debug)]
pub enum InitError {
    Surface(wgpu::CreateSurfaceError),
    Adapter(wgpu::RequestAdapterError),
    Device(wgpu::RequestDeviceError),
    UnsupportedSurface,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitError::Surface(e) => write!(f, "surface: {e}"),
            InitError::Adapter(e) => write!(f, "adapter: {e}"),
            InitError::Device(e) => write!(f, "device: {e}"),
            InitError::UnsupportedSurface => f.write_str("surface not supported by adapter"),
        }
    }
}

impl std::error::Error for InitError {}

/// WebGL2 cannot meet wgpu's default limits.
fn limits(adapter: &wgpu::Adapter) -> wgpu::Limits {
    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
}

impl Gpu {
    /// Picks WebGPU when the browser exposes it and WebGL2 otherwise.
    pub async fn new(
        target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<Gpu, InitError> {
        let instance = wgpu::util::new_instance_with_webgpu_detection(
            wgpu::InstanceDescriptor::new_without_display_handle(),
        )
        .await;
        let surface = instance
            .create_surface(target)
            .map_err(InitError::Surface)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(InitError::Adapter)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("lightseek"),
                required_limits: limits(&adapter),
                ..Default::default()
            })
            .await
            .map_err(InitError::Device)?;
        let config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .ok_or(InitError::UnsupportedSurface)?;
        surface.configure(&device, &config);
        let renderer = Renderer::new(&device, config.format);
        Ok(Gpu {
            surface,
            device,
            queue,
            config,
            renderer,
        })
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas(canvas: web_sys::HtmlCanvasElement) -> Result<Gpu, InitError> {
        let (w, h) = (canvas.width(), canvas.height());
        Gpu::new(wgpu::SurfaceTarget::Canvas(canvas), w, h).await
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    /// Width over height, for a camera's `view`.
    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }

    /// The next surface texture. None skips the frame: the surface is not
    /// ready, or it was stale and has been reconfigured.
    fn acquire(&self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            _ => None,
        }
    }

    /// Draws `scene` as `view` sees it and presents it.
    pub fn draw(&mut self, view: &View, scene: &SceneDraw<'_>) {
        let Some(frame) = self.acquire() else { return };
        let target = frame.texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        self.renderer.render(
            &self.device,
            &self.queue,
            &mut encoder,
            Output {
                view: &target,
                size: self.size(),
            },
            view,
            scene,
        );
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
    }
}
