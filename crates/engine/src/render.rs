//! The GPU passes: every opaque instance into the sun's shadow map, then the
//! ground and those instances, lit, shadowed, and fogged, then the wedges
//! blended over them. Draws into any texture view.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use glam::{Mat4, Vec3};

use crate::camera::View;
use crate::draw::{SceneDraw, Shape};
use crate::mesh;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const FAN_SEGMENTS: u32 = 24;
/// WebGL2 guarantees 2D textures this wide and no wider.
const SHADOW_RES: u32 = 2048;
/// Half the side of the square the shadow map covers, in world units.
const SHADOW_HALF: f32 = 40.0;
/// How far toward the sun the shadow volume reaches past its centre, and
/// away from it: tall enough for the city's towers.
const SHADOW_DEPTH: f32 = 150.0;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    sun_view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    sun: [f32; 4],
    sky: [f32; 4],
    ground: [f32; 4],
    fog: [f32; 4],
}

/// Model matrix columns, then linear colour.
type GpuInstance = [f32; 20];
/// Apex and yaw, half angle and radius, then linear colour with alpha.
type GpuWedge = [f32; 12];

/// The view a frame lands in, and its size in pixels.
#[derive(Copy, Clone, Debug)]
pub struct Output<'a> {
    pub view: &'a wgpu::TextureView,
    pub size: [u32; 2],
}

#[derive(Debug)]
struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
}

#[derive(Debug)]
pub struct Renderer {
    /// Whether the output stores sRGB itself; otherwise the shader encodes.
    srgb_target: bool,
    globals: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    shadow_group: wgpu::BindGroup,
    shadow_map: wgpu::TextureView,
    shadow: wgpu::RenderPipeline,
    ground: wgpu::RenderPipeline,
    solid: wgpu::RenderPipeline,
    wedge: wgpu::RenderPipeline,
    meshes: [Mesh; 4],
    fan: wgpu::Buffer,
    instances: wgpu::Buffer,
    wedges: wgpu::Buffer,
    staging: Vec<GpuInstance>,
    depth: Option<(wgpu::TextureView, [u32; 2])>,
}

impl Renderer {
    /// `output` is the format of the views `render` will draw into.
    pub fn new(device: &wgpu::Device, output: wgpu::TextureFormat) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let shadow_map = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow map"),
                size: wgpu::Extent3d {
                    width: SHADOW_RES,
                    height: SHADOW_RES,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        // Linear filtering compares the four nearest texels and blends the
        // results: soft edges for free.
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow"),
            layout: &shadow_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&shadow_map),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&globals_layout), Some(&shadow_layout)],
            immediate_size: 0,
        });
        let caster_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });

        let pipeline = |label,
                        vs,
                        fs,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        cull,
                        opaque: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: cull,
                    ..Default::default()
                },
                // Reversed depth: nearer is greater.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(opaque),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: output,
                        blend: (!opaque).then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let solid_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let instance_attrs = wgpu::vertex_attr_array![
            2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4
        ];
        let solid_buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<mesh::Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &solid_attrs,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GpuInstance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &instance_attrs,
            }),
        ];
        let solid = pipeline(
            "solid",
            "vs_solid",
            "fs_lit",
            &solid_buffers,
            Some(wgpu::Face::Back),
            true,
        );
        // Depth only, both faces, so thin slabs and open shapes still cast.
        let shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&caster_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &solid_buffers,
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: 1.5,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let ground = pipeline("ground", "vs_ground", "fs_ground", &[], None, true);
        let fan_attrs = wgpu::vertex_attr_array![0 => Float32x2];
        let wedge_attrs =
            wgpu::vertex_attr_array![1 => Float32x4, 2 => Float32x4, 3 => Float32x4];
        let wedge = pipeline(
            "wedge",
            "vs_wedge",
            "fs_wedge",
            &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<[f32; 2]>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &fan_attrs,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GpuWedge>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wedge_attrs,
                }),
            ],
            None,
            false,
        );

        let meshes = Shape::ALL
            .map(|shape| {
                let data = mesh::build(shape);
                Mesh {
                    vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("mesh vertices"),
                        contents: bytemuck::cast_slice(&data.vertices),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                    indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("mesh indices"),
                        contents: bytemuck::cast_slice(&data.indices),
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    count: data.indices.len() as u32,
                }
            });
        let fan: Vec<[f32; 2]> = (0..FAN_SEGMENTS)
            .flat_map(|i| {
                let (a, b) = (
                    i as f32 / FAN_SEGMENTS as f32,
                    (i + 1) as f32 / FAN_SEGMENTS as f32,
                );
                [[0.5, 0.0], [a, 1.0], [b, 1.0]]
            })
            .collect();
        let fan = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fan"),
            contents: bytemuck::cast_slice(&fan),
            usage: wgpu::BufferUsages::VERTEX,
        });

        Renderer {
            srgb_target: output.is_srgb(),
            globals,
            globals_group,
            shadow_group,
            shadow_map,
            shadow,
            ground,
            solid,
            wedge,
            meshes,
            fan,
            instances: vertex_buffer(device, "instances", 256),
            wedges: vertex_buffer(device, "wedges", 256),
            staging: Vec::new(),
            depth: None,
        }
    }

    /// Records one frame of `scene` as `view` sees it into `encoder`.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        out: Output<'_>,
        view: &View,
        scene: &SceneDraw<'_>,
    ) {
        let env = &scene.env;
        let sky = env.sky.map(to_linear);
        let globals = Globals {
            view_proj: view.view_proj().to_cols_array_2d(),
            sun_view_proj: sun_view_proj(view, env.sun).to_cols_array_2d(),
            eye: view.eye.extend(1.0).into(),
            sun: env.sun.normalize_or(glam::Vec3::Y).extend(0.0).into(),
            sky: [sky[0], sky[1], sky[2], 1.0],
            ground: linear4(env.ground, 1.0),
            fog: [
                env.fog[0],
                env.fog[1].max(env.fog[0] + 1e-3),
                if self.srgb_target { 0.0 } else { 1.0 },
                0.0,
            ],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));

        // Grouped by shape, so each shape draws as one run from offset zero:
        // WebGL2 cannot start an instanced draw past the first instance.
        self.staging.clear();
        let mut runs = [0u32; 4];
        for (run, shape) in runs.iter_mut().zip(Shape::ALL) {
            let before = self.staging.len();
            self.staging.extend(
                scene
                    .instances
                    .iter()
                    .filter(|i| i.shape == shape)
                    .map(|i| {
                        let mut v = [0.0; 20];
                        v[..16].copy_from_slice(&i.model.to_cols_array());
                        v[16..].copy_from_slice(&linear4(i.color, 1.0));
                        v
                    }),
            );
            *run = (self.staging.len() - before) as u32;
        }
        upload(device, queue, &mut self.instances, "instances", &self.staging);
        let wedges: Vec<GpuWedge> = scene
            .wedges
            .iter()
            .map(|w| {
                let c = linear4([w.color[0], w.color[1], w.color[2]], w.color[3]);
                [
                    w.center.x,
                    w.center.y,
                    w.center.z,
                    w.yaw,
                    w.half_angle,
                    w.radius,
                    0.0,
                    0.0,
                    c[0],
                    c[1],
                    c[2],
                    c[3],
                ]
            })
            .collect();
        upload(device, queue, &mut self.wedges, "wedges", &wedges);

        if self.depth.as_ref().is_none_or(|(_, size)| *size != out.size) {
            self.depth = Some((depth_view(device, out.size), out.size));
        }
        let mut casters = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.shadow_map,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        casters.set_bind_group(0, &self.globals_group, &[]);
        casters.set_pipeline(&self.shadow);
        self.draw_instances(&mut casters, &runs);
        drop(casters);

        let (depth, _) = self.depth.as_ref().expect("made above");
        let clear = if self.srgb_target { sky } else { env.sky };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: out.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: f64::from(clear[0]),
                        g: f64::from(clear[1]),
                        b: f64::from(clear[2]),
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.globals_group, &[]);
        pass.set_bind_group(1, &self.shadow_group, &[]);
        pass.set_pipeline(&self.ground);
        pass.draw(0..6, 0..1);

        pass.set_pipeline(&self.solid);
        self.draw_instances(&mut pass, &runs);

        if !wedges.is_empty() {
            pass.set_pipeline(&self.wedge);
            pass.set_vertex_buffer(0, self.fan.slice(..));
            pass.set_vertex_buffer(1, self.wedges.slice(..));
            pass.draw(0..FAN_SEGMENTS * 3, 0..wedges.len() as u32);
        }
    }
}

impl Renderer {
    /// Each shape's run of the uploaded instances, `runs[i]` long.
    fn draw_instances(&self, pass: &mut wgpu::RenderPass<'_>, runs: &[u32; 4]) {
        let stride = std::mem::size_of::<GpuInstance>() as u64;
        let mut start = 0u64;
        for (mesh, &n) in self.meshes.iter().zip(runs) {
            if n > 0 {
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, self.instances.slice(start * stride..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..mesh.count, 0, 0..n);
            }
            start += u64::from(n);
        }
    }
}

/// Where the shadow map is centred: the ground the camera looks at, but no
/// further from the eye than the map's half width, so the near field is
/// always covered.
fn shadow_focus(view: &View) -> Vec3 {
    let eye = view.eye;
    let ahead = view.view.inverse().transform_vector3(Vec3::NEG_Z);
    let flat = Vec3::new(eye.x, 0.0, eye.z);
    let reach = if ahead.y < -1e-3 {
        (eye.y / -ahead.y).max(0.0)
    } else {
        f32::INFINITY
    };
    let along = Vec3::new(ahead.x, 0.0, ahead.z) * reach.min(1e6);
    flat + along.clamp_length_max(SHADOW_HALF)
}

/// World to the shadow map's clip space: an orthographic box looking along
/// the sunlight, centred on `shadow_focus`. Its orientation never changes
/// for a given sun, and its centre snaps to whole texels, so a moving
/// camera slides the map without making shadow edges crawl.
fn sun_view_proj(view: &View, sun: Vec3) -> Mat4 {
    let toward = sun.normalize_or(Vec3::Y);
    let up = if toward.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let light = glam::camera::rh::view::look_to_mat4(Vec3::ZERO, -toward, up);
    let c = light.transform_point3(shadow_focus(view));
    let texel = 2.0 * SHADOW_HALF / SHADOW_RES as f32;
    let (x, y) = ((c.x / texel).round() * texel, (c.y / texel).round() * texel);
    let proj = glam::camera::rh::proj::directx::orthographic(
        x - SHADOW_HALF,
        x + SHADOW_HALF,
        y - SHADOW_HALF,
        y + SHADOW_HALF,
        -c.z - SHADOW_DEPTH,
        -c.z + SHADOW_DEPTH,
    );
    proj * light
}

fn vertex_buffer(device: &wgpu::Device, label: &str, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Writes `items` to the front of `buffer`, growing it first if needed.
fn upload<T: Pod>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &mut wgpu::Buffer,
    label: &str,
    items: &[T],
) {
    let bytes: &[u8] = bytemuck::cast_slice(items);
    if bytes.len() as u64 > buffer.size() {
        *buffer = vertex_buffer(device, label, (bytes.len() as u64).next_power_of_two());
    }
    if !bytes.is_empty() {
        queue.write_buffer(buffer, 0, bytes);
    }
}

fn depth_view(device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear4(rgb: [f32; 3], alpha: f32) -> [f32; 4] {
    let [r, g, b] = rgb.map(to_linear);
    [r, g, b, alpha]
}

#[cfg(test)]
mod tests {
    use glam::{Vec3, Vec3Swizzles, Vec4Swizzles};
    use naga::back::glsl;

    use super::{SHADOW_RES, sun_view_proj};
    use crate::camera::{FollowCamera, View};

    const SUN: Vec3 = Vec3::new(0.45, 1.0, 0.3);

    fn clip(view: &View, p: Vec3) -> Vec3 {
        let c = sun_view_proj(view, SUN) * p.extend(1.0);
        c.xyz() / c.w
    }

    #[test]
    fn the_shadow_box_holds_the_followed_avatar_and_the_tops_of_towers() {
        let target = Vec3::new(12.0, 0.0, -30.0);
        let view = FollowCamera::new(target, 0.7).view(1.6);
        for p in [target, target + Vec3::Y * 60.0, target + Vec3::X * 20.0] {
            let c = clip(&view, p);
            assert!(c.x.abs() < 1.0 && c.y.abs() < 1.0, "{p} lands at {c}");
            assert!((0.0..=1.0).contains(&c.z), "{p} depth {}", c.z);
        }
    }

    #[test]
    fn a_sub_texel_camera_move_moves_the_shadow_map_by_whole_texels() {
        let texel_ndc = 2.0 / SHADOW_RES as f32;
        let a = FollowCamera::new(Vec3::new(3.0, 0.0, 4.0), 0.3).view(1.6);
        let world = Vec3::new(5.0, 0.0, 9.0);
        for step in [0.001, 0.013, 0.37, 2.9] {
            let b = FollowCamera::new(Vec3::new(3.0 + step, 0.0, 4.0), 0.3).view(1.6);
            let shift = (clip(&b, world) - clip(&a, world)).xy() / texel_ndc;
            assert!(
                (shift - shift.round()).abs().max_element() < 1e-2,
                "step {step} shifted by {shift} texels"
            );
        }
    }

    /// The browser falls back to WebGL2, which runs these entry points as
    /// GLSL ES 3.00; the native test adapter never takes that path.
    #[test]
    fn every_entry_point_translates_to_glsl_es_300() {
        let module = naga::front::wgsl::parse_str(include_str!("shader.wgsl")).unwrap();
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap();
        let options = glsl::Options {
            version: glsl::Version::new_gles(300),
            ..Default::default()
        };
        assert_eq!(module.entry_points.len(), 7);
        for ep in &module.entry_points {
            let pipeline = glsl::PipelineOptions {
                shader_stage: ep.stage,
                entry_point: ep.name.clone(),
                multiview: None,
            };
            let mut out = String::new();
            glsl::Writer::new(
                &mut out,
                &module,
                &info,
                &options,
                &pipeline,
                naga::proc::BoundsCheckPolicies::default(),
            )
            .and_then(|mut w| w.write())
            .unwrap_or_else(|e| panic!("{}: {e}", ep.name));
        }
    }
}
