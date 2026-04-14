use crate::gpu::GpuState;
use wgpu::util::DeviceExt;
use wgpu::*;

/// Hex geometry constants matching the engine.
/// Flat-top hex: HEX_SIZE = 150m (flat-to-flat diameter).
const HEX_RADIUS: f32 = 86.602_54; // 150 / sqrt(3), center to corner
const SQRT3: f32 = 1.732_050_8;

/// Player colors for territory fill (up to 8 players).
const PLAYER_COLORS: [[f32; 3]; 8] = [
    [0.2, 0.4, 0.9], // blue
    [0.9, 0.2, 0.2], // red
    [0.2, 0.8, 0.3], // green
    [0.9, 0.7, 0.1], // yellow
    [0.7, 0.3, 0.8], // purple
    [0.1, 0.8, 0.8], // cyan
    [0.9, 0.5, 0.2], // orange
    [0.6, 0.6, 0.6], // gray
];

/// Per-vertex data for the hex overlay. Must match shader VertexInput.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct HexVertex {
    world_xz: [f32; 2],
    color: [f32; 4],
}

/// GPU-uploadable overlay uniforms. Must match hex_overlay.wgsl.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct HexOverlayUniforms {
    map_width: f32,
    map_height: f32,
    alpha: f32,
    _pad0: f32,
}

/// Hex overlay renderer: territory fill + borders draped on terrain.
pub struct HexOverlayRenderer {
    border_pipeline: RenderPipeline,
    fill_pipeline: RenderPipeline,
    bind_group: BindGroup,
    uniform_buf: Buffer,
    border_vertex_buf: Buffer,
    border_vertex_count: u32,
    fill_vertex_buf: Buffer,
    fill_vertex_count: u32,
}

/// Convert axial hex (q, r) to viewer horizontal plane (x, z).
fn hex_center(q: i32, r: i32) -> [f32; 2] {
    let fq = q as f32;
    let fr = r as f32;
    [
        SQRT3 * HEX_RADIUS * (fq + fr / 2.0),
        1.5 * HEX_RADIUS * fr,
    ]
}

/// World position of flat-top hex corner i (0..5) for hex (q, r).
fn hex_corner(q: i32, r: i32, i: usize) -> [f32; 2] {
    let c = hex_center(q, r);
    let angle = std::f32::consts::FRAC_PI_3 * i as f32;
    [c[0] + HEX_RADIUS * angle.cos(), c[1] + HEX_RADIUS * angle.sin()]
}

/// Generate border (line list) and fill (triangle list) vertex data.
fn generate_hex_geometry(
    hex_ownership: &[Option<u8>],
    grid_width: u32,
    grid_height: u32,
) -> (Vec<HexVertex>, Vec<HexVertex>) {
    let mut border_verts = Vec::new();
    let mut fill_verts = Vec::new();
    let border_color = [0.3, 0.3, 0.3, 0.4f32];

    for r in 0..grid_height as i32 {
        for q in 0..grid_width as i32 {
            let idx = (r as u32 * grid_width + q as u32) as usize;
            let center = hex_center(q, r);

            // Borders: 6 line segments
            for i in 0..6 {
                let c0 = hex_corner(q, r, i);
                let c1 = hex_corner(q, r, (i + 1) % 6);
                border_verts.push(HexVertex { world_xz: c0, color: border_color });
                border_verts.push(HexVertex { world_xz: c1, color: border_color });
            }

            // Territory fill: 6 triangles from center to adjacent corners
            if let Some(Some(owner)) = hex_ownership.get(idx) {
                let pc = PLAYER_COLORS[(*owner as usize) % PLAYER_COLORS.len()];
                let fill_color = [pc[0], pc[1], pc[2], 0.25];

                for i in 0..6 {
                    let c0 = hex_corner(q, r, i);
                    let c1 = hex_corner(q, r, (i + 1) % 6);
                    fill_verts.push(HexVertex { world_xz: center, color: fill_color });
                    fill_verts.push(HexVertex { world_xz: c0, color: fill_color });
                    fill_verts.push(HexVertex { world_xz: c1, color: fill_color });
                }
            }
        }
    }

    (border_verts, fill_verts)
}

fn vertex_buffer_layout() -> VertexBufferLayout<'static> {
    VertexBufferLayout {
        array_stride: std::mem::size_of::<HexVertex>() as u64,
        step_mode: VertexStepMode::Vertex,
        attributes: &[
            VertexAttribute {
                format: VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: 8,
                shader_location: 1,
            },
        ],
    }
}

fn create_pipeline(
    device: &Device,
    shader: &ShaderModule,
    pipeline_layout: &PipelineLayout,
    format: TextureFormat,
    topology: PrimitiveTopology,
    label: &str,
) -> RenderPipeline {
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(pipeline_layout),
        vertex: VertexState {
            module: shader,
            entry_point: Some("vs_hex_overlay"),
            buffers: &[vertex_buffer_layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(FragmentState {
            module: shader,
            entry_point: Some("fs_hex_overlay"),
            targets: &[Some(ColorTargetState {
                format,
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: PrimitiveState {
            topology,
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Depth24Plus,
            depth_write_enabled: Some(false),
            depth_compare: Some(CompareFunction::LessEqual),
            stencil: StencilState::default(),
            bias: DepthBiasState::default(),
        }),
        multisample: MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl HexOverlayRenderer {
    pub fn new(
        gpu: &GpuState,
        camera_bgl: &BindGroupLayout,
        heightmap_view: &TextureView,
        sampler: &Sampler,
        map_width: u32,
        map_height: u32,
        hex_ownership: &[Option<u8>],
        grid_width: u32,
        grid_height: u32,
    ) -> Self {
        let device = &gpu.device;

        let (border_verts, fill_verts) =
            generate_hex_geometry(hex_ownership, grid_width, grid_height);

        let border_vertex_buf = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("hex border vb"),
            contents: bytemuck::cast_slice(&border_verts),
            usage: BufferUsages::VERTEX,
        });

        let fill_vertex_buf = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("hex fill vb"),
            contents: bytemuck::cast_slice(&fill_verts),
            usage: BufferUsages::VERTEX,
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("hex overlay shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/hex_overlay.wgsl").into()),
        });

        let overlay_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("hex overlay bgl"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Texture {
                        multisampled: false,
                        view_dimension: TextureViewDimension::D2,
                        sample_type: TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniform_buf = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("hex overlay uniforms"),
            contents: bytemuck::bytes_of(&HexOverlayUniforms {
                map_width: map_width as f32,
                map_height: map_height as f32,
                alpha: 1.0,
                _pad0: 0.0,
            }),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("hex overlay bg"),
            layout: &overlay_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(heightmap_view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(sampler),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("hex overlay pl"),
            bind_group_layouts: &[Some(camera_bgl), Some(&overlay_bgl)],
            immediate_size: 0,
        });

        let border_pipeline = create_pipeline(
            device, &shader, &pipeline_layout, gpu.format,
            PrimitiveTopology::LineList, "hex border pipeline",
        );
        let fill_pipeline = create_pipeline(
            device, &shader, &pipeline_layout, gpu.format,
            PrimitiveTopology::TriangleList, "hex fill pipeline",
        );

        Self {
            border_pipeline,
            fill_pipeline,
            bind_group,
            uniform_buf,
            border_vertex_buf,
            border_vertex_count: border_verts.len() as u32,
            fill_vertex_buf,
            fill_vertex_count: fill_verts.len() as u32,
        }
    }

    /// Update territory fill when hex ownership changes.
    pub fn update_ownership(
        &mut self,
        device: &Device,
        hex_ownership: &[Option<u8>],
        grid_width: u32,
        grid_height: u32,
    ) {
        let (_, fill_verts) = generate_hex_geometry(hex_ownership, grid_width, grid_height);
        self.fill_vertex_buf = device.create_buffer_init(&util::BufferInitDescriptor {
            label: Some("hex fill vb"),
            contents: bytemuck::cast_slice(&fill_verts),
            usage: BufferUsages::VERTEX,
        });
        self.fill_vertex_count = fill_verts.len() as u32;
    }

    /// Render hex overlay: fill first (alpha-blended), then borders on top.
    pub fn render(
        &self,
        encoder: &mut CommandEncoder,
        color_view: &TextureView,
        depth_view: &TextureView,
        camera_bg: &BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("hex overlay pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });

        pass.set_bind_group(0, camera_bg, &[]);
        pass.set_bind_group(1, &self.bind_group, &[]);

        // Territory fill (triangles, alpha-blended)
        if self.fill_vertex_count > 0 {
            pass.set_pipeline(&self.fill_pipeline);
            pass.set_vertex_buffer(0, self.fill_vertex_buf.slice(..));
            pass.draw(0..self.fill_vertex_count, 0..1);
        }

        // Hex borders (lines)
        if self.border_vertex_count > 0 {
            pass.set_pipeline(&self.border_pipeline);
            pass.set_vertex_buffer(0, self.border_vertex_buf.slice(..));
            pass.draw(0..self.border_vertex_count, 0..1);
        }
    }
}
