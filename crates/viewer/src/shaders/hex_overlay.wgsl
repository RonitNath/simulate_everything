// Hex overlay shader — renders hex borders and territory fill
// draped onto terrain by sampling heightmap for vertex Y.

struct CameraUniforms {
    view_proj: mat4x4f,
    camera_pos: vec3f,
    _pad0: f32,
}

struct HexOverlayUniforms {
    raster_origin_x: f32,
    raster_origin_z: f32,
    raster_cell_size: f32,
    _pad0: f32,
    map_width: f32,
    map_height: f32,
    alpha: f32,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(1) @binding(0) var<uniform> overlay: HexOverlayUniforms;
@group(1) @binding(1) var heightmap: texture_2d<f32>;
@group(1) @binding(2) var terrain_sampler: sampler;

// Per-vertex data from the vertex buffer
struct VertexInput {
    // World XZ position of this vertex (hex corner or center)
    @location(0) world_xz: vec2f,
    // RGBA color (territory color with alpha)
    @location(1) color: vec4f,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4f,
    @location(0) color: vec4f,
}

@vertex
fn vs_hex_overlay(in: VertexInput) -> VertexOutput {
    // Sample heightmap to drape vertex onto terrain
    let uv = vec2f(
        (in.world_xz.x - overlay.raster_origin_x) / (overlay.map_width * overlay.raster_cell_size),
        (in.world_xz.y - overlay.raster_origin_z) / (overlay.map_height * overlay.raster_cell_size),
    );
    let height = textureSampleLevel(heightmap, terrain_sampler, uv, 0.0).r;

    // Small Y offset to avoid z-fighting with terrain
    let world_pos = vec3f(in.world_xz.x, height + 0.15, in.world_xz.y);

    var out: VertexOutput;
    out.clip_pos = camera.view_proj * vec4f(world_pos, 1.0);
    out.color = vec4f(in.color.rgb, in.color.a * overlay.alpha);
    return out;
}

@fragment
fn fs_hex_overlay(in: VertexOutput) -> @location(0) vec4f {
    return in.color;
}
