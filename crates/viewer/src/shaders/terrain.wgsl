// Terrain vertex/fragment shader
// Renders heightmap terrain with clipmap LOD, per-pixel normals,
// slope-based material blending, and atmospheric fog.

struct CameraUniforms {
    view_proj: mat4x4f,
    camera_pos: vec3f,
    _pad0: f32,
}

struct TerrainUniforms {
    // Grid dimensions for this LOD ring
    grid_width: u32,
    grid_height: u32,
    // World-space offset of this ring's origin
    origin_x: f32,
    origin_z: f32,
    // Spacing between vertices in world units
    cell_size: f32,
    raster_origin_x: f32,
    raster_origin_z: f32,
    raster_cell_size: f32,
    _pad0: f32,
    // Heightmap texture dimensions
    map_width: f32,
    map_height: f32,
    // Texel size in UV space (1.0 / map_width)
    texel_uv: f32,
    // Sun direction (normalized)
    sun_dir: vec3f,
    // Fog parameters
    fog_density: f32,
    fog_color: vec3f,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(1) @binding(0) var<uniform> terrain: TerrainUniforms;
@group(1) @binding(1) var heightmap: texture_2d<f32>;
@group(1) @binding(2) var material_map: texture_2d<u32>;
@group(1) @binding(3) var terrain_sampler: sampler;

struct VertexOutput {
    @builtin(position) clip_pos: vec4f,
    @location(0) world_pos: vec3f,
    @location(1) uv: vec2f,
    @location(2) view_dist: f32,
}

@vertex
fn vs_terrain(@builtin(vertex_index) vid: u32) -> VertexOutput {
    // Convert vertex index to grid position
    // Each quad = 2 triangles = 6 vertices (non-indexed for simplicity in B1)
    let quad_idx = vid / 6u;
    let vert_in_quad = vid % 6u;

    let quads_per_row = terrain.grid_width - 1u;
    let quad_row = quad_idx / quads_per_row;
    let quad_col = quad_idx % quads_per_row;

    // Triangle vertex offsets within quad (two triangles: 0-1-2, 2-1-3)
    var col_off: u32;
    var row_off: u32;
    switch vert_in_quad {
        case 0u: { col_off = 0u; row_off = 0u; }
        case 1u: { col_off = 1u; row_off = 0u; }
        case 2u: { col_off = 0u; row_off = 1u; }
        case 3u: { col_off = 0u; row_off = 1u; }
        case 4u: { col_off = 1u; row_off = 0u; }
        default: { col_off = 1u; row_off = 1u; }
    }

    let grid_col = quad_col + col_off;
    let grid_row = quad_row + row_off;

    // World-space XZ position
    let world_x = terrain.origin_x + f32(grid_col) * terrain.cell_size;
    let world_z = terrain.origin_z + f32(grid_row) * terrain.cell_size;

    // UV for heightmap sampling
    let uv = vec2f(
        (world_x - terrain.raster_origin_x) / (terrain.map_width * terrain.raster_cell_size),
        (world_z - terrain.raster_origin_z) / (terrain.map_height * terrain.raster_cell_size),
    );

    // Sample height with bilinear filtering (hardware sampler)
    let height = textureSampleLevel(heightmap, terrain_sampler, uv, 0.0).r;

    let world_pos = vec3f(world_x, height, world_z);

    var out: VertexOutput;
    out.clip_pos = camera.view_proj * vec4f(world_pos, 1.0);
    out.world_pos = world_pos;
    out.uv = uv;
    out.view_dist = length(world_pos - camera.camera_pos);
    return out;
}

// Material colors (indexed by material map value)
fn material_color(mat_idx: u32) -> vec3f {
    switch mat_idx {
        case 0u: { return vec3f(0.35, 0.55, 0.25); } // grass
        case 1u: { return vec3f(0.55, 0.45, 0.30); } // dirt
        case 2u: { return vec3f(0.50, 0.48, 0.45); } // rock
        case 3u: { return vec3f(0.75, 0.72, 0.60); } // sand
        case 4u: { return vec3f(0.25, 0.35, 0.15); } // forest
        case 5u: { return vec3f(0.40, 0.38, 0.35); } // gravel
        case 6u: { return vec3f(0.60, 0.55, 0.45); } // packed earth
        case 7u: { return vec3f(0.45, 0.50, 0.55); } // stone
        default: { return vec3f(0.35, 0.55, 0.25); }  // fallback: grass
    }
}

@fragment
fn fs_terrain(in: VertexOutput) -> @location(0) vec4f {
    let texel = terrain.texel_uv;

    // Per-pixel normal from heightmap gradient (4 adjacent samples)
    let h_l = textureSampleLevel(heightmap, terrain_sampler, in.uv + vec2f(-texel, 0.0), 0.0).r;
    let h_r = textureSampleLevel(heightmap, terrain_sampler, in.uv + vec2f( texel, 0.0), 0.0).r;
    let h_d = textureSampleLevel(heightmap, terrain_sampler, in.uv + vec2f(0.0, -texel), 0.0).r;
    let h_u = textureSampleLevel(heightmap, terrain_sampler, in.uv + vec2f(0.0,  texel), 0.0).r;

    // World-space texel size for correct normal scale
    let texel_world = terrain.raster_cell_size;
    let normal = normalize(vec3f(h_l - h_r, 2.0 * texel_world, h_d - h_u));

    // Material from material map
    let map_coord = vec2i(
        clamp(
            in.uv * vec2f(terrain.map_width, terrain.map_height),
            vec2f(0.0, 0.0),
            vec2f(terrain.map_width - 1.0, terrain.map_height - 1.0),
        )
    );
    let mat_idx = textureLoad(material_map, map_coord, 0).r;

    // Base material color
    var base_color = material_color(mat_idx);

    // Slope-based blending: steep slopes get rock color regardless of material
    let slope = 1.0 - normal.y; // 0 = flat, 1 = vertical
    let rock_color = material_color(2u); // rock
    // Smooth blend: starts at slope 0.3, fully rock at 0.7
    let slope_blend = smoothstep(0.3, 0.7, slope);
    base_color = mix(base_color, rock_color, slope_blend);

    // Simple procedural noise for variation (based on world position)
    let noise = fract(sin(dot(in.world_pos.xz * 0.1, vec2f(12.9898, 78.233))) * 43758.5453);
    base_color = base_color * (0.92 + 0.16 * noise);

    // Directional sun lighting (diffuse + ambient)
    let ndotl = max(dot(normal, terrain.sun_dir), 0.0);
    let light = ndotl * 0.65 + 0.35; // 65% diffuse + 35% ambient

    // Atmospheric fog
    let fog_factor = exp(-in.view_dist * terrain.fog_density);

    let lit_color = base_color * light;
    let final_color = mix(terrain.fog_color, lit_color, fog_factor);

    return vec4f(final_color, 1.0);
}
