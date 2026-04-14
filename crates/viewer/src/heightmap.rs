use crate::camera::CameraUniforms;
use crate::gpu::GpuState;
use wgpu::*;

/// GPU-uploadable terrain uniforms. Must match terrain.wgsl TerrainUniforms.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TerrainUniforms {
    pub grid_width: u32,
    pub grid_height: u32,
    pub origin_x: f32,
    pub origin_z: f32,
    pub cell_size: f32,
    pub raster_origin_x: f32,
    pub raster_origin_z: f32,
    pub raster_cell_size: f32,
    pub _pad0: f32,
    pub map_width: f32,
    pub map_height: f32,
    pub texel_uv: f32,
    pub sun_dir: [f32; 3],
    pub fog_density: f32,
    pub fog_color: [f32; 3],
    pub _pad1: f32,
}

/// A single clipmap LOD ring.
struct ClipmapRing {
    cell_size: f32,
    grid_width: u32,
    grid_height: u32,
    vertex_count: u32,
}

/// Chunk size for dirty tracking (texels per side).
pub const CHUNK_SIZE: u32 = 64;

/// Heightmap terrain renderer with clipmap LOD and chunk dirty tracking.
pub struct HeightmapRenderer {
    pipeline: RenderPipeline,
    camera_bind_group_layout: BindGroupLayout,
    terrain_bind_group_layout: BindGroupLayout,
    terrain_bind_group: BindGroup,
    camera_uniform_buf: Buffer,
    camera_bind_group: BindGroup,
    terrain_uniform_buf: Buffer,
    heightmap_texture: Texture,
    heightmap_view: TextureView,
    material_texture: Texture,
    sampler: Sampler,
    rings: Vec<ClipmapRing>,
    raster_origin_x: f32,
    raster_origin_z: f32,
    raster_cell_size: f32,
    map_width: u32,
    map_height: u32,
    // Chunk dirty tracking
    chunks_x: u32,
    chunks_y: u32,
    dirty_chunks: Vec<bool>,
    /// CPU-side copy of heightmap for chunk re-uploads.
    height_data: Vec<f32>,
    /// CPU-side copy of material data for chunk re-uploads.
    material_data: Vec<u32>,
}

impl HeightmapRenderer {
    pub fn new(
        gpu: &GpuState,
        map_width: u32,
        map_height: u32,
        raster_origin_x: f32,
        raster_origin_z: f32,
        raster_cell_size: f32,
        height_data: &[f32],
        material_data: &[u32],
    ) -> Self {
        let device = &gpu.device;
        let queue = &gpu.queue;

        // --- Heightmap texture (R32Float) ---
        let heightmap_texture = device.create_texture(&TextureDescriptor {
            label: Some("heightmap"),
            size: Extent3d {
                width: map_width,
                height: map_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &heightmap_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            bytemuck::cast_slice(height_data),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(map_width * 4),
                rows_per_image: Some(map_height),
            },
            Extent3d {
                width: map_width,
                height: map_height,
                depth_or_array_layers: 1,
            },
        );

        // --- Material texture (R32Uint) ---
        let material_texture = device.create_texture(&TextureDescriptor {
            label: Some("material_map"),
            size: Extent3d {
                width: map_width,
                height: map_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Uint,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &material_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            bytemuck::cast_slice(material_data),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(map_width * 4),
                rows_per_image: Some(map_height),
            },
            Extent3d {
                width: map_width,
                height: map_height,
                depth_or_array_layers: 1,
            },
        );

        // --- Clipmap rings ---
        // 4 LOD levels: 1m, 4m, 16m, 64m cell size
        // Each ring covers a band around the camera
        let rings = vec![
            ClipmapRing {
                cell_size: 1.0,
                grid_width: 128,
                grid_height: 128,
                vertex_count: 127 * 127 * 6,
            },
            ClipmapRing {
                cell_size: 4.0,
                grid_width: 64,
                grid_height: 64,
                vertex_count: 63 * 63 * 6,
            },
            ClipmapRing {
                cell_size: 16.0,
                grid_width: 32,
                grid_height: 32,
                vertex_count: 31 * 31 * 6,
            },
            ClipmapRing {
                cell_size: 64.0,
                grid_width: 16,
                grid_height: 16,
                vertex_count: 15 * 15 * 6,
            },
        ];

        // --- Shader ---
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("terrain shader"),
            source: ShaderSource::Wgsl(include_str!("shaders/terrain.wgsl").into()),
        });

        // --- Bind group layouts ---
        let camera_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("camera bgl"),
                entries: &[BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let terrain_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: Some("terrain bgl"),
                entries: &[
                    // terrain uniforms
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                        ty: BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // heightmap texture
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            multisampled: false,
                            view_dimension: TextureViewDimension::D2,
                            sample_type: TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    // material map texture
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStages::FRAGMENT,
                        ty: BindingType::Texture {
                            multisampled: false,
                            view_dimension: TextureViewDimension::D2,
                            sample_type: TextureSampleType::Uint,
                        },
                        count: None,
                    },
                    // sampler
                    BindGroupLayoutEntry {
                        binding: 3,
                        visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                        ty: BindingType::Sampler(SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        // --- Pipeline ---
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("terrain pipeline layout"),
            bind_group_layouts: &[
                Some(&camera_bind_group_layout),
                Some(&terrain_bind_group_layout),
            ],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("terrain pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_terrain"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_terrain"),
                targets: &[Some(ColorTargetState {
                    format: gpu.format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
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

        // --- Uniform buffers ---
        let camera_uniform_buf = device.create_buffer(&BufferDescriptor {
            label: Some("camera uniforms"),
            size: std::mem::size_of::<CameraUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let camera_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("camera bg"),
            layout: &camera_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: camera_uniform_buf.as_entire_binding(),
            }],
        });

        let terrain_uniform_buf = device.create_buffer(&BufferDescriptor {
            label: Some("terrain uniforms"),
            size: std::mem::size_of::<TerrainUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Sampler ---
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("terrain sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });

        let heightmap_view = heightmap_texture.create_view(&TextureViewDescriptor::default());
        let material_view = material_texture.create_view(&TextureViewDescriptor::default());

        // Chunk dirty tracking
        let chunks_x = (map_width + CHUNK_SIZE - 1) / CHUNK_SIZE;
        let chunks_y = (map_height + CHUNK_SIZE - 1) / CHUNK_SIZE;
        let dirty_chunks = vec![false; (chunks_x * chunks_y) as usize];

        let terrain_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("terrain bg"),
            layout: &terrain_bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: terrain_uniform_buf.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&heightmap_view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&material_view),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            pipeline,
            camera_bind_group_layout,
            terrain_bind_group_layout,
            terrain_bind_group,
            camera_uniform_buf,
            camera_bind_group,
            terrain_uniform_buf,
            heightmap_texture,
            heightmap_view,
            material_texture,
            sampler,
            rings,
            raster_origin_x,
            raster_origin_z,
            raster_cell_size,
            map_width,
            map_height,
            chunks_x,
            chunks_y,
            dirty_chunks,
            height_data: height_data.to_vec(),
            material_data: material_data.to_vec(),
        }
    }

    /// Apply a heightmap mutation to a rectangular region.
    /// Updates the CPU-side copy and marks overlapping chunks dirty.
    pub fn mutate_heightmap(&mut self, x: u32, y: u32, width: u32, height: u32, data: &[f32]) {
        // Update CPU-side copy
        for dy in 0..height {
            for dx in 0..width {
                let tx = x + dx;
                let ty = y + dy;
                if tx < self.map_width && ty < self.map_height {
                    let idx = (ty * self.map_width + tx) as usize;
                    let src_idx = (dy * width + dx) as usize;
                    if let (Some(dst), Some(src)) =
                        (self.height_data.get_mut(idx), data.get(src_idx))
                    {
                        *dst = *src;
                    }
                }
            }
        }

        // Mark overlapping chunks dirty
        let chunk_x0 = x / CHUNK_SIZE;
        let chunk_y0 = y / CHUNK_SIZE;
        let chunk_x1 = ((x + width).min(self.map_width) - 1) / CHUNK_SIZE;
        let chunk_y1 = ((y + height).min(self.map_height) - 1) / CHUNK_SIZE;

        for cy in chunk_y0..=chunk_y1 {
            for cx in chunk_x0..=chunk_x1 {
                let ci = (cy * self.chunks_x + cx) as usize;
                if let Some(d) = self.dirty_chunks.get_mut(ci) {
                    *d = true;
                }
            }
        }
    }

    pub fn mutate_materialmap(&mut self, x: u32, y: u32, width: u32, height: u32, data: &[u32]) {
        for dy in 0..height {
            for dx in 0..width {
                let tx = x + dx;
                let ty = y + dy;
                if tx < self.map_width && ty < self.map_height {
                    let idx = (ty * self.map_width + tx) as usize;
                    let src_idx = (dy * width + dx) as usize;
                    if let (Some(dst), Some(src)) =
                        (self.material_data.get_mut(idx), data.get(src_idx))
                    {
                        *dst = *src;
                    }
                }
            }
        }

        let chunk_x0 = x / CHUNK_SIZE;
        let chunk_y0 = y / CHUNK_SIZE;
        let chunk_x1 = ((x + width).min(self.map_width) - 1) / CHUNK_SIZE;
        let chunk_y1 = ((y + height).min(self.map_height) - 1) / CHUNK_SIZE;

        for cy in chunk_y0..=chunk_y1 {
            for cx in chunk_x0..=chunk_x1 {
                let ci = (cy * self.chunks_x + cx) as usize;
                if let Some(d) = self.dirty_chunks.get_mut(ci) {
                    *d = true;
                }
            }
        }
    }

    pub fn mutate_terrain_patch(
        &mut self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        heights: &[f32],
        materials: &[u32],
    ) {
        self.mutate_heightmap(x, y, width, height, heights);
        self.mutate_materialmap(x, y, width, height, materials);
    }

    /// Upload all dirty chunks to the GPU, then clear dirty flags.
    pub fn flush_dirty_chunks(&mut self, queue: &Queue) {
        for ci in 0..(self.chunks_x * self.chunks_y) as usize {
            if !self.dirty_chunks[ci] {
                continue;
            }

            let cx = (ci as u32) % self.chunks_x;
            let cy = (ci as u32) / self.chunks_x;

            let x0 = cx * CHUNK_SIZE;
            let y0 = cy * CHUNK_SIZE;
            let w = CHUNK_SIZE.min(self.map_width - x0);
            let h = CHUNK_SIZE.min(self.map_height - y0);

            // Extract chunk data from CPU copy
            let mut chunk_data = Vec::with_capacity((w * h) as usize);
            let mut material_chunk = Vec::with_capacity((w * h) as usize);
            for row in y0..(y0 + h) {
                let start = (row * self.map_width + x0) as usize;
                let end = start + w as usize;
                chunk_data.extend_from_slice(&self.height_data[start..end]);
                material_chunk.extend_from_slice(&self.material_data[start..end]);
            }

            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &self.material_texture,
                    mip_level: 0,
                    origin: Origin3d { x: x0, y: y0, z: 0 },
                    aspect: TextureAspect::All,
                },
                bytemuck::cast_slice(&material_chunk),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );

            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &self.heightmap_texture,
                    mip_level: 0,
                    origin: Origin3d { x: x0, y: y0, z: 0 },
                    aspect: TextureAspect::All,
                },
                bytemuck::cast_slice(&chunk_data),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );

            self.dirty_chunks[ci] = false;
        }
    }

    /// Render terrain with clipmap rings centered on camera target.
    pub fn render(
        &self,
        encoder: &mut CommandEncoder,
        color_view: &TextureView,
        depth_view: &TextureView,
        queue: &Queue,
        camera_uniforms: &CameraUniforms,
        camera_target: [f32; 3],
    ) {
        // Upload camera uniforms
        queue.write_buffer(
            &self.camera_uniform_buf,
            0,
            bytemuck::bytes_of(camera_uniforms),
        );

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("terrain pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: 0.65,
                        g: 0.75,
                        b: 0.85,
                        a: 1.0,
                    }),
                    store: StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(1.0),
                    store: StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.camera_bind_group, &[]);

        // Draw each clipmap ring
        for ring in &self.rings {
            // Center ring on camera target, snapped to cell grid
            let snap_x = (camera_target[0] / ring.cell_size).floor() * ring.cell_size;
            let snap_z = (camera_target[2] / ring.cell_size).floor() * ring.cell_size;
            let origin_x = snap_x - (ring.grid_width as f32 * ring.cell_size) / 2.0;
            let origin_z = snap_z - (ring.grid_height as f32 * ring.cell_size) / 2.0;

            let uniforms = TerrainUniforms {
                grid_width: ring.grid_width,
                grid_height: ring.grid_height,
                origin_x,
                origin_z,
                cell_size: ring.cell_size,
                raster_origin_x: self.raster_origin_x,
                raster_origin_z: self.raster_origin_z,
                raster_cell_size: self.raster_cell_size,
                _pad0: 0.0,
                map_width: self.map_width as f32,
                map_height: self.map_height as f32,
                texel_uv: 1.0 / (self.map_width.max(self.map_height) as f32),
                sun_dir: [0.4, 0.8, 0.3],
                fog_density: 0.0003,
                fog_color: [0.65, 0.75, 0.85],
                _pad1: 0.0,
            };
            queue.write_buffer(&self.terrain_uniform_buf, 0, bytemuck::bytes_of(&uniforms));

            pass.set_bind_group(1, &self.terrain_bind_group, &[]);
            pass.draw(0..ring.vertex_count, 0..1);
        }
    }

    // --- Accessors for shared GPU resources ---

    pub fn camera_bind_group_layout(&self) -> &BindGroupLayout {
        &self.camera_bind_group_layout
    }

    pub fn camera_bind_group(&self) -> &BindGroup {
        &self.camera_bind_group
    }

    pub fn camera_uniform_buf(&self) -> &Buffer {
        &self.camera_uniform_buf
    }

    pub fn heightmap_view(&self) -> &TextureView {
        &self.heightmap_view
    }

    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }

    pub fn map_width(&self) -> u32 {
        self.map_width
    }

    pub fn map_height(&self) -> u32 {
        self.map_height
    }

    pub fn raster_origin_x(&self) -> f32 {
        self.raster_origin_x
    }

    pub fn raster_origin_z(&self) -> f32 {
        self.raster_origin_z
    }

    pub fn raster_cell_size(&self) -> f32 {
        self.raster_cell_size
    }
}
