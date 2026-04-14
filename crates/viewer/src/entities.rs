use crate::gpu::GpuState;
use wgpu::*;

/// Per-entity tick data received from the server. Must match interpolate.wgsl.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EntityTickData {
    pub pos: [f32; 3],
    pub facing: f32,
    pub owner: u32,
    pub entity_kind: u32,
    pub health_frac: f32,
    pub stamina_frac: f32,
    pub flags: u32,
    pub _pad: [f32; 3],
}

/// Per-entity GPU render data (written by compute shader). Must match entity.wgsl.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EntityGpuData {
    pub pos: [f32; 3],
    pub facing: f32,
    pub owner: u32,
    pub lod_tier: u32,
    pub entity_kind: u32,
    pub health_frac: f32,
    pub stamina_frac: f32,
    pub flags: u32,
    pub _pad: [f32; 2],
}

/// Interpolation uniforms. Must match interpolate.wgsl.
#[repr(C, align(16))]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct InterpolationUniforms {
    t: f32,
    entity_count: u32,
    _pad0: [u32; 2],
    camera_pos: [f32; 3],
    viewport_height: f32,
    lod_scale: f32,
    _pad: [f32; 3],
}

/// Entity render uniforms. Must match entity.wgsl.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct EntityUniforms {
    target_lod: u32,
    entity_count: u32,
    _pad: [f32; 2],
}

const MAX_ENTITIES: u32 = 16384;
const CLOSE_VERTS_PER_ENTITY: u32 = 24; // 8 triangles × 3 verts
const MID_VERTS_PER_ENTITY: u32 = 12; // diamond + facing indicator
const INTERPOLATION_UNIFORM_BINDING_SIZE: u64 = 64;

/// Entity renderer with compute interpolation and LOD-based drawing.
pub struct EntityRenderer {
    compute_pipeline: ComputePipeline,
    compute_bind_group: BindGroup,
    interp_uniform_buf: Buffer,

    prev_buf: Buffer,
    curr_buf: Buffer,

    render_pipeline: RenderPipeline,
    // Two bind groups — one per LOD tier (pre-filled uniforms)
    close_bind_group: BindGroup,
    mid_bind_group: BindGroup,
    close_uniform_buf: Buffer,
    mid_uniform_buf: Buffer,

    entity_count: u32,
    last_tick: Vec<EntityTickData>,
}

impl EntityRenderer {
    pub fn new(gpu: &GpuState, camera_bgl: &BindGroupLayout) -> Self {
        let device = &gpu.device;

        let tick_size = (MAX_ENTITIES as usize) * std::mem::size_of::<EntityTickData>();
        let render_size = (MAX_ENTITIES as usize) * std::mem::size_of::<EntityGpuData>();

        let prev_buf = device.create_buffer(&BufferDescriptor {
            label: Some("entity prev tick"),
            size: tick_size as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let curr_buf = device.create_buffer(&BufferDescriptor {
            label: Some("entity curr tick"),
            size: tick_size as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let render_buf = device.create_buffer(&BufferDescriptor {
            label: Some("entity render state"),
            size: render_size as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let interp_uniform_buf = device.create_buffer(&BufferDescriptor {
            label: Some("interp uniforms"),
            size: INTERPOLATION_UNIFORM_BINDING_SIZE,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Compute pipeline ---
        let compute_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("interpolate compute"),
            source: ShaderSource::Wgsl(include_str!("shaders/interpolate.wgsl").into()),
        });

        let compute_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("interp bgl"),
            entries: &[
                bgl_storage_ro(0),
                bgl_storage_ro(1),
                bgl_storage_rw(2),
                bgl_uniform(3, ShaderStages::COMPUTE),
            ],
        });

        let compute_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("interp bg"),
            layout: &compute_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: prev_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: curr_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: render_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: interp_uniform_buf.as_entire_binding(),
                },
            ],
        });

        let compute_pl = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("interp pl"),
            bind_group_layouts: &[Some(&compute_bgl)],
            immediate_size: 0,
        });

        let compute_pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("interpolate pipeline"),
            layout: Some(&compute_pl),
            module: &compute_shader,
            entry_point: Some("interpolate"),
            compilation_options: Default::default(),
            cache: None,
        });

        // --- Render pipeline ---
        let render_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("entity render"),
            source: ShaderSource::Wgsl(include_str!("shaders/entity.wgsl").into()),
        });

        let entity_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("entity render bgl"),
            entries: &[bgl_storage_ro_vert(0), bgl_uniform(1, ShaderStages::VERTEX)],
        });

        // Two uniform buffers for the two LOD tiers
        let close_uniform_buf = create_entity_uniform_buf(device, "close", 0);
        let mid_uniform_buf = create_entity_uniform_buf(device, "mid", 1);

        let close_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("entity close bg"),
            layout: &entity_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: render_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: close_uniform_buf.as_entire_binding(),
                },
            ],
        });

        let mid_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("entity mid bg"),
            layout: &entity_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: render_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: mid_uniform_buf.as_entire_binding(),
                },
            ],
        });

        let render_pl = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("entity render pl"),
            bind_group_layouts: &[Some(camera_bgl), Some(&entity_bgl)],
            immediate_size: 0,
        });

        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("entity pipeline"),
            layout: Some(&render_pl),
            vertex: VertexState {
                module: &render_shader,
                entry_point: Some("vs_entity"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &render_shader,
                entry_point: Some("fs_entity"),
                targets: &[Some(ColorTargetState {
                    format: gpu.format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(DepthStencilState {
                format: TextureFormat::Depth24Plus,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Less),
                stencil: StencilState::default(),
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            compute_pipeline,
            compute_bind_group,
            interp_uniform_buf,
            prev_buf,
            curr_buf,
            render_pipeline,
            close_bind_group,
            mid_bind_group,
            close_uniform_buf,
            mid_uniform_buf,
            entity_count: 0,
            last_tick: Vec::new(),
        }
    }

    /// Upload a new tick, preserving the previous tick for interpolation.
    pub fn push_tick(&mut self, queue: &Queue, entities: &[EntityTickData]) {
        self.entity_count = entities.len().min(MAX_ENTITIES as usize) as u32;
        let curr_slice = &entities[..self.entity_count as usize];
        let curr_bytes = bytemuck::cast_slice(curr_slice);

        if self.last_tick.len() == self.entity_count as usize {
            let prev_bytes = bytemuck::cast_slice(self.last_tick.as_slice());
            queue.write_buffer(&self.prev_buf, 0, prev_bytes);
        } else {
            queue.write_buffer(&self.prev_buf, 0, curr_bytes);
        }
        queue.write_buffer(&self.curr_buf, 0, curr_bytes);
        self.last_tick.clear();
        self.last_tick.extend_from_slice(curr_slice);

        // Update entity count in LOD uniform buffers
        let close = EntityUniforms {
            target_lod: 0,
            entity_count: self.entity_count,
            _pad: [0.0; 2],
        };
        let mid = EntityUniforms {
            target_lod: 1,
            entity_count: self.entity_count,
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.close_uniform_buf, 0, bytemuck::bytes_of(&close));
        queue.write_buffer(&self.mid_uniform_buf, 0, bytemuck::bytes_of(&mid));
    }

    /// Dispatch interpolation compute shader.
    pub fn interpolate(
        &self,
        encoder: &mut CommandEncoder,
        queue: &Queue,
        t: f32,
        camera_pos: [f32; 3],
        viewport_height: f32,
    ) {
        if self.entity_count == 0 {
            return;
        }

        let uniforms = InterpolationUniforms {
            t,
            entity_count: self.entity_count,
            _pad0: [0; 2],
            camera_pos,
            viewport_height,
            lod_scale: 86.6 / std::f32::consts::FRAC_PI_4.tan(),
            _pad: [0.0; 3],
        };
        queue.write_buffer(&self.interp_uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let workgroups = (self.entity_count + 255) / 256;
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("interpolation"),
            ..Default::default()
        });
        pass.set_pipeline(&self.compute_pipeline);
        pass.set_bind_group(0, &self.compute_bind_group, &[]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }

    /// Render entities at close and mid LOD tiers.
    pub fn render(
        &self,
        encoder: &mut CommandEncoder,
        color_view: &TextureView,
        depth_view: &TextureView,
        camera_bg: &BindGroup,
    ) {
        if self.entity_count == 0 {
            return;
        }

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("entity pass"),
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

        pass.set_pipeline(&self.render_pipeline);
        pass.set_bind_group(0, camera_bg, &[]);

        // Close LOD (tier 0)
        pass.set_bind_group(1, &self.close_bind_group, &[]);
        pass.draw(0..CLOSE_VERTS_PER_ENTITY, 0..self.entity_count);

        // Mid LOD (tier 1)
        pass.set_bind_group(1, &self.mid_bind_group, &[]);
        pass.draw(0..MID_VERTS_PER_ENTITY, 0..self.entity_count);
    }

    pub fn entity_count(&self) -> u32 {
        self.entity_count
    }
}

// --- Helpers ---

fn bgl_storage_ro(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bgl_storage_rw(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bgl_storage_ro_vert(binding: u32) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bgl_uniform(binding: u32, visibility: ShaderStages) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn create_entity_uniform_buf(device: &Device, label: &str, lod: u32) -> Buffer {
    use wgpu::util::DeviceExt;
    let data = EntityUniforms {
        target_lod: lod,
        entity_count: 0,
        _pad: [0.0; 2],
    };
    device.create_buffer_init(&util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(&data),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    })
}
