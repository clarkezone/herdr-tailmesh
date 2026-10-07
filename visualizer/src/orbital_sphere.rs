use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::orb_viewport::Viewport;

const SPHERE_RADIUS: f32 = 2.2;
const SOURCE_PARTICLE_COUNT: usize = 15_000;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SceneUniforms {
    view_projection: [[f32; 4]; 4],
    group_model: [[f32; 4]; 4],
    viewport_brightness: [f32; 4],
    camera_depth: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct Particle {
    pub position_size: [f32; 4],
    pub color_softness: [f32; 4],
}

impl Particle {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![1 => Float32x4, 2 => Float32x4];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    corner: [f32; 2],
}

impl QuadVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct LineVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// Adjacent geometry vertices are one segment instance. Expand the segment
/// into a screen-space quad: native LineList rasterization is always one
/// physical pixel wide, regardless of display scaling.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LineSegment {
    endpoints: [LineVertex; 2],
}

impl LineSegment {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        1 => Float32x3, 2 => Float32x4, 3 => Float32x3, 4 => Float32x4
    ];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

pub struct OrbitalSphereScene {
    mesh_particle_front: wgpu::RenderPipeline,
    mesh_particle_back: wgpu::RenderPipeline,
    mesh_line_front: wgpu::RenderPipeline,
    mesh_line_back: wgpu::RenderPipeline,
    veil_pipeline: wgpu::RenderPipeline,
    quad_buffer: wgpu::Buffer,
    quad_index_buffer: wgpu::Buffer,
    atmosphere_buffer: wgpu::Buffer,
    atmosphere_count: u32,
    node_buffer: wgpu::Buffer,
    orbit_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    node_count: u32,
    line_count: u32,
}

impl OrbitalSphereScene {
    pub fn new(
        device: &wgpu::Device,
        color_format: wgpu::TextureFormat,
        depth_format: wgpu::TextureFormat,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("orbital_sphere.wgsl"));
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("orbital sphere uniforms"),
            contents: bytemuck::bytes_of(&SceneUniforms {
                view_projection: Mat4::IDENTITY.to_cols_array_2d(),
                group_model: Mat4::IDENTITY.to_cols_array_2d(),
                viewport_brightness: [1.0, 1.0, 1.45, 0.0],
                camera_depth: [0.0; 4],
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("orbital sphere bind group layout"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("orbital sphere bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("orbital sphere pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let additive_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let create_particles = |entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("orbital sphere particle pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("particle_vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(QuadVertex::layout()), Some(Particle::layout())],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(additive_blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let create_lines = |entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("orbital sphere line pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("line_vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(QuadVertex::layout()), Some(LineSegment::layout())],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(additive_blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth_format,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        let mesh_particle_front = create_particles("front_particle_fs");
        let mesh_particle_back = create_particles("rear_particle_fs");
        let mesh_line_front = create_lines("front_line_fs");
        let mesh_line_back = create_lines("rear_line_fs");
        let veil_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh translucent sphere veil"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("veil_vs"),
                compilation_options: Default::default(),
                buffers: &[Some(QuadVertex::layout())],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("veil_fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        let quad_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("orbital sphere particle quad"),
            contents: bytemuck::cast_slice(&[
                QuadVertex {
                    corner: [-1.0, -1.0],
                },
                QuadVertex {
                    corner: [1.0, -1.0],
                },
                QuadVertex { corner: [1.0, 1.0] },
                QuadVertex {
                    corner: [-1.0, 1.0],
                },
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let quad_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("orbital sphere particle indices"),
            contents: bytemuck::cast_slice(&[0_u16, 1, 2, 2, 3, 0]),
            usage: wgpu::BufferUsages::INDEX,
        });
        let particles = create_sphere_particles();
        let atmosphere: Vec<Particle> = particles
            .iter()
            .step_by(2)
            .flat_map(|p| {
                let mut core = *p;
                core.position_size[3] = 0.9;
                core.color_softness = [0.015, 0.006, 0.03, 0.9];
                let mut halo = core;
                halo.position_size[3] = 2.4;
                halo.color_softness = [0.003, 0.001, 0.006, 0.02];
                [halo, core]
            })
            .collect();
        let atmosphere_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh orbital atmosphere"),
            contents: bytemuck::cast_slice(&atmosphere),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let node_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("orbital sphere nodes"),
            size: (std::mem::size_of::<Particle>() * crate::mesh_orb::MAX_PARTICLES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let orbit_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("orbital sphere orbit lines"),
            size: (std::mem::size_of::<LineVertex>() * crate::mesh_orb::MAX_LINES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            mesh_particle_front,
            mesh_particle_back,
            mesh_line_front,
            mesh_line_back,
            veil_pipeline,
            quad_buffer,
            quad_index_buffer,
            atmosphere_buffer,
            atmosphere_count: atmosphere.len() as u32,
            node_buffer,
            orbit_buffer,
            uniform_buffer,
            bind_group,
            node_count: 0,
            line_count: 0,
        }
    }

    pub fn update_mesh(
        &mut self,
        queue: &wgpu::Queue,
        sim: &crate::mesh_model::Simulation,
        viewport: Viewport,
    ) -> Result<(), String> {
        let (view_projection, group_model, scale) = crate::mesh_orb::camera(sim.time, viewport);
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&SceneUniforms {
                view_projection: view_projection.to_cols_array_2d(),
                group_model: group_model.to_cols_array_2d(),
                viewport_brightness: [viewport.width, viewport.height, 1.45, scale],
                camera_depth: [
                    crate::mesh_orb::camera_distance(viewport),
                    SPHERE_RADIUS * 1.1,
                    1.0,
                    viewport.pixels_per_point,
                ],
            }),
        );
        let geometry = crate::mesh_orb::geometry(sim);
        if geometry.particles.len() > crate::mesh_orb::MAX_PARTICLES
            || geometry.lines.len() > crate::mesh_orb::MAX_LINES
        {
            return Err("Orb geometry exceeded fixed GPU capacity".into());
        }
        self.node_count = geometry.particles.len() as u32;
        if !geometry.lines.len().is_multiple_of(2) {
            return Err("Orb line geometry contains an incomplete segment".into());
        }
        self.line_count = (geometry.lines.len() / 2) as u32;
        if !geometry.particles.is_empty() {
            queue.write_buffer(
                &self.node_buffer,
                0,
                bytemuck::cast_slice(&geometry.particles),
            );
        }
        queue.write_buffer(&self.orbit_buffer, 0, bytemuck::cast_slice(&geometry.lines));
        Ok(())
    }

    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        viewport: Option<Viewport>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("orbital sphere pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.001,
                        g: 0.0,
                        b: 0.004,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = viewport {
            pass.set_viewport(
                viewport.x,
                viewport.y,
                viewport.width,
                viewport.height,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(
                viewport.x as u32,
                viewport.y as u32,
                viewport.width.max(1.0) as u32,
                viewport.height.max(1.0) as u32,
            );
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        self.draw_mesh_layer(&mut pass, &self.mesh_line_back, &self.mesh_particle_back);
        pass.set_pipeline(&self.veil_pipeline);
        pass.set_vertex_buffer(0, self.quad_buffer.slice(..));
        pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
        self.draw_mesh_layer(&mut pass, &self.mesh_line_front, &self.mesh_particle_front);
    }
    fn draw_mesh_layer<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        lines: &'a wgpu::RenderPipeline,
        particles: &'a wgpu::RenderPipeline,
    ) {
        pass.set_pipeline(lines);
        pass.set_vertex_buffer(0, self.quad_buffer.slice(..));
        pass.set_vertex_buffer(1, self.orbit_buffer.slice(..));
        pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..self.line_count);
        pass.set_pipeline(particles);
        pass.set_vertex_buffer(0, self.quad_buffer.slice(..));
        pass.set_index_buffer(self.quad_index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.set_vertex_buffer(1, self.atmosphere_buffer.slice(..));
        pass.draw_indexed(0..6, 0, 0..self.atmosphere_count);
        pass.set_vertex_buffer(1, self.node_buffer.slice(..));
        pass.draw_indexed(0..6, 0, 0..self.node_count);
    }
}

fn create_sphere_particles() -> Vec<Particle> {
    let bright = srgb_to_linear(Vec3::new(0xa7 as f32, 0x8b as f32, 0xfa as f32) / 255.0);
    let dim = srgb_to_linear(Vec3::new(0x70 as f32, 0x1a as f32, 0x75 as f32) / 255.0);
    let mut particles = Vec::with_capacity(SOURCE_PARTICLE_COUNT);
    for index in 0..SOURCE_PARTICLE_COUNT {
        let phi = (-1.0 + (2.0 * index as f32) / SOURCE_PARTICLE_COUNT as f32).acos();
        let theta = (SOURCE_PARTICLE_COUNT as f32 * std::f32::consts::PI).sqrt() * phi;
        let x = SPHERE_RADIUS * theta.cos() * phi.sin();
        let y = SPHERE_RADIUS * theta.sin() * phi.sin();
        let z = SPHERE_RADIUS * phi.cos();
        let noise = (x * 3.5).sin() * (y * 3.5).cos() * (z * 3.5).sin() + (x * 6.0).cos() * 0.4;
        if noise <= -0.1 {
            continue;
        }
        let distortion = 1.0 + noise * 0.1;
        let color = dim.lerp(bright, if noise > 0.5 { 1.0 } else { 0.3 });
        let position = [x * distortion, y * distortion, z * distortion];
        particles.push(Particle {
            position_size: [position[0], position[1], position[2], 2.5],
            color_softness: [color.x, color.y, color.z, 0.95],
        });
    }
    particles
}

fn srgb_to_linear(color: Vec3) -> Vec3 {
    color.map(|channel| {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a native GPU adapter; run with --ignored"]
    fn gpu_marker_area_and_branch_width_follow_display_scale() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut scene = OrbitalSphereScene::new(&device, format, wgpu::TextureFormat::Depth32Float);
        // Isolated probes use the actual production pipelines, buffers and
        // shader. Red marker and green branch let us measure them independently.
        scene.atmosphere_count = 0;
        scene.node_count = 1;
        scene.line_count = 1;
        queue.write_buffer(
            &scene.node_buffer,
            0,
            bytemuck::bytes_of(&Particle {
                position_size: [-0.5, 0.0, 1.0, 8.0],
                color_softness: [1.0, 0.0, 0.0, 0.92],
            }),
        );
        queue.write_buffer(
            &scene.orbit_buffer,
            0,
            bytemuck::cast_slice(&[
                LineVertex {
                    position: [0.0, 0.0, 1.0],
                    color: [0.0, 0.7, 0.0, 1.0],
                },
                LineVertex {
                    position: [0.75, 0.0, 1.0],
                    color: [0.0, 0.7, 0.0, 1.0],
                },
            ]),
        );
        let mut base_marker_area = 0.0;
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0, 4.0] {
            let width = (128.0 * scale) as u32;
            let size = wgpu::Extent3d {
                width,
                height: width,
                depth_or_array_layers: 1,
            };
            let make_texture = |format, usage| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("DPI render probe"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            };
            let color = make_texture(
                format,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let depth = make_texture(
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            queue.write_buffer(
                &scene.uniform_buffer,
                0,
                bytemuck::bytes_of(&SceneUniforms {
                    view_projection: Mat4::IDENTITY.to_cols_array_2d(),
                    group_model: Mat4::IDENTITY.to_cols_array_2d(),
                    viewport_brightness: [width as f32, width as f32, 1.0, scale],
                    camera_depth: [10.0, 2.0, 1.0, scale],
                }),
            );
            let stride = (width * 4).div_ceil(256) * 256;
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("DPI readback"),
                size: (stride * width) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            scene.render(
                &mut encoder,
                &color.create_view(&Default::default()),
                &depth.create_view(&Default::default()),
                None,
            );
            encoder.copy_texture_to_buffer(
                color.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
                        rows_per_image: Some(width),
                    },
                },
                size,
            );
            let submission = queue.submit([encoder.finish()]);
            let (sender, receiver) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    sender.send(result).unwrap()
                });
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(std::time::Duration::from_secs(10)),
                })
                .unwrap();
            receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
                .unwrap();
            let pixels = readback.slice(..).get_mapped_range().unwrap();
            let mut marker_area = 0.0;
            let mut branch_width = 0.0;
            for y in 0..width {
                for x in 0..width {
                    let index = (y * stride + x * 4) as usize;
                    if x < width / 2 && pixels[index] > 20 {
                        marker_area += f32::from(pixels[index]) / 255.0 / scale.powi(2);
                    }
                    // Center column excludes endpoints and the isolated marker.
                    if x == width * 3 / 4 {
                        branch_width += f32::from(pixels[index + 1]) / 255.0 / 0.7 / scale;
                    }
                }
            }
            if scale == 1.0 {
                base_marker_area = marker_area;
            }
            assert!(base_marker_area > 50.0);
            assert!(
                (marker_area / base_marker_area - 1.0).abs() < 0.06,
                "{scale}x marker area: {marker_area}, base: {base_marker_area}"
            );
            assert!(
                (branch_width - 1.0).abs() < 0.05,
                "{scale}x branch logical width: {branch_width}"
            );
        }
    }
}
