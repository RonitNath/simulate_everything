use crate::gpu::GpuState;
use wgpu::*;

pub const BODY_POINT_COUNT: usize = 16;
const MAX_BODIES: u32 = 2048;
const INSTANCE_SLOTS_PER_BODY: u32 = 14;
const QUAD_VERTS: u32 = 6;

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BodyTickData {
    pub points: [[f32; 4]; BODY_POINT_COUNT],
    pub weapon_a: [f32; 4],
    pub weapon_b: [f32; 4],
    pub shield_center: [f32; 4],
    pub shield_normal: [f32; 4],
    pub owner: u32,
    pub wound_mask: u32,
    pub _pad: [u32; 2],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BodyInterpolationUniforms {
    t: f32,
    body_count: u32,
    _pad: [u32; 2],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BodyRenderUniforms {
    body_count: u32,
    _pad: [u32; 3],
}

pub struct BodyRenderer {
    compute_pipeline: ComputePipeline,
    compute_bind_group: BindGroup,
    interp_uniform_buf: Buffer,
    prev_buf: Buffer,
    curr_buf: Buffer,
    render_pipeline: RenderPipeline,
    render_bind_group: BindGroup,
    render_uniform_buf: Buffer,
    body_count: u32,
    last_tick: Vec<BodyTickData>,
}

impl BodyRenderer {
    pub fn new(gpu: &GpuState, camera_bgl: &BindGroupLayout) -> Self {
        let device = &gpu.device;
        let tick_size = (MAX_BODIES as usize) * std::mem::size_of::<BodyTickData>();

        let prev_buf = device.create_buffer(&BufferDescriptor {
            label: Some("body prev tick"),
            size: tick_size as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let curr_buf = device.create_buffer(&BufferDescriptor {
            label: Some("body curr tick"),
            size: tick_size as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let render_buf = device.create_buffer(&BufferDescriptor {
            label: Some("body render state"),
            size: tick_size as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let interp_uniform_buf = device.create_buffer(&BufferDescriptor {
            label: Some("body interp uniforms"),
            size: std::mem::size_of::<BodyInterpolationUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let render_uniform_buf = device.create_buffer(&BufferDescriptor {
            label: Some("body render uniforms"),
            size: std::mem::size_of::<BodyRenderUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let compute_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("body interpolate compute"),
            source: ShaderSource::Wgsl(include_str!("shaders/body_interpolate.wgsl").into()),
        });
        let render_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("body render"),
            source: ShaderSource::Wgsl(include_str!("shaders/body.wgsl").into()),
        });

        let compute_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("body interp bgl"),
            entries: &[
                storage_layout_entry(0, ShaderStages::COMPUTE, true),
                storage_layout_entry(1, ShaderStages::COMPUTE, true),
                storage_layout_entry(2, ShaderStages::COMPUTE, false),
                uniform_layout_entry(3, ShaderStages::COMPUTE),
            ],
        });
        let compute_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("body interp bg"),
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
            label: Some("body interp pl"),
            bind_group_layouts: &[Some(&compute_bgl)],
            immediate_size: 0,
        });
        let compute_pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("body interp pipeline"),
            layout: Some(&compute_pl),
            module: &compute_shader,
            entry_point: Some("interpolate_body"),
            compilation_options: Default::default(),
            cache: None,
        });

        let render_bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("body render bgl"),
            entries: &[
                storage_layout_entry(0, ShaderStages::VERTEX, true),
                uniform_layout_entry(1, ShaderStages::VERTEX),
            ],
        });
        let render_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("body render bg"),
            layout: &render_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: render_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: render_uniform_buf.as_entire_binding(),
                },
            ],
        });
        let render_pl = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("body render pl"),
            bind_group_layouts: &[Some(camera_bgl), Some(&render_bgl)],
            immediate_size: 0,
        });
        let render_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("body pipeline"),
            layout: Some(&render_pl),
            vertex: VertexState {
                module: &render_shader,
                entry_point: Some("vs_body"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &render_shader,
                entry_point: Some("fs_body"),
                targets: &[Some(ColorTargetState {
                    format: gpu.format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                cull_mode: None,
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
            render_bind_group,
            render_uniform_buf,
            body_count: 0,
            last_tick: Vec::new(),
        }
    }

    pub fn push_tick(&mut self, queue: &Queue, bodies: &[BodyTickData]) {
        self.body_count = bodies.len().min(MAX_BODIES as usize) as u32;
        let curr_slice = &bodies[..self.body_count as usize];
        let curr_bytes = bytemuck::cast_slice(curr_slice);

        if self.last_tick.len() == self.body_count as usize {
            queue.write_buffer(
                &self.prev_buf,
                0,
                bytemuck::cast_slice(self.last_tick.as_slice()),
            );
        } else {
            queue.write_buffer(&self.prev_buf, 0, curr_bytes);
        }
        queue.write_buffer(&self.curr_buf, 0, curr_bytes);
        self.last_tick.clear();
        self.last_tick.extend_from_slice(curr_slice);

        let uniforms = BodyRenderUniforms {
            body_count: self.body_count,
            _pad: [0; 3],
        };
        queue.write_buffer(&self.render_uniform_buf, 0, bytemuck::bytes_of(&uniforms));
    }

    pub fn interpolate(&self, encoder: &mut CommandEncoder, queue: &Queue, t: f32) {
        if self.body_count == 0 {
            return;
        }

        let uniforms = BodyInterpolationUniforms {
            t,
            body_count: self.body_count,
            _pad: [0; 2],
        };
        queue.write_buffer(&self.interp_uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("body interpolation"),
            ..Default::default()
        });
        pass.set_pipeline(&self.compute_pipeline);
        pass.set_bind_group(0, &self.compute_bind_group, &[]);
        pass.dispatch_workgroups(self.body_count.div_ceil(64), 1, 1);
    }

    pub fn render(
        &self,
        encoder: &mut CommandEncoder,
        color_view: &TextureView,
        depth_view: &TextureView,
        camera_bg: &BindGroup,
    ) {
        if self.body_count == 0 {
            return;
        }

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("body pass"),
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
        pass.set_bind_group(1, &self.render_bind_group, &[]);
        pass.draw(
            0..QUAD_VERTS,
            0..(self.body_count * INSTANCE_SLOTS_PER_BODY),
        );
    }

    pub fn body_count(&self) -> u32 {
        self.body_count
    }
}

fn storage_layout_entry(
    binding: u32,
    visibility: ShaderStages,
    read_only: bool,
) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_layout_entry(binding: u32, visibility: ShaderStages) -> BindGroupLayoutEntry {
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
