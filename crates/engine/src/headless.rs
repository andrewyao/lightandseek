//! Native offscreen rendering: a device with no surface, texture readback,
//! and PNG output. Tests and examples draw through this.

use std::path::Path;

use crate::camera::View;
use crate::draw::SceneDraw;
use crate::render::{Output, Renderer};
use crate::{InitError, limits};

/// sRGB, so the PNG bytes are what a browser canvas would show.
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A renderer drawing into a texture it can read back.
#[derive(Debug)]
pub struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Renderer,
    target: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Offscreen {
    pub fn new(size: [u32; 2]) -> Result<Offscreen, InitError> {
        let (device, queue) = pollster::block_on(async {
            let instance =
                wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                })
                .await
                .map_err(InitError::Adapter)?;
            adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some("lightseek headless"),
                    required_limits: limits(&adapter),
                    ..Default::default()
                })
                .await
                .map_err(InitError::Device)
        })?;
        let renderer = Renderer::new(&device, TARGET_FORMAT);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen target"),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        Ok(Offscreen {
            device,
            queue,
            renderer,
            target,
            view,
        })
    }

    pub fn size(&self) -> [u32; 2] {
        [self.target.width(), self.target.height()]
    }

    pub fn aspect(&self) -> f32 {
        self.target.width() as f32 / self.target.height() as f32
    }

    /// Draws one frame and reads it back: tightly packed RGBA8, top row
    /// first.
    pub fn render(&mut self, view: &View, scene: &SceneDraw<'_>) -> Vec<u8> {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let size = self.size();
        self.renderer.render(
            &self.device,
            &self.queue,
            &mut encoder,
            Output {
                view: &self.view,
                size,
            },
            view,
            scene,
        );
        self.queue.submit([encoder.finish()]);
        self.read()
    }

    fn read(&self) -> Vec<u8> {
        let [w, h] = self.size();
        let row = w * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            self.target.size(),
        );
        self.queue.submit([encoder.finish()]);
        buffer.map_async(wgpu::MapMode::Read, .., |r| {
            r.expect("readback map failed");
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device lost while waiting");
        let mapped = buffer.get_mapped_range(..).expect("mapped above");
        let mut out = Vec::with_capacity((row * h) as usize);
        for y in 0..h as usize {
            let start = y * padded as usize;
            out.extend_from_slice(&mapped[start..start + row as usize]);
        }
        out
    }
}

pub fn write_png(path: &Path, size: [u32; 2], rgba: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, size[0], size[1]);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(rgba))
        .map_err(std::io::Error::other)
}
