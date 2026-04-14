// Entity interpolation + LOD assignment compute shader.
// Lerps entity positions between prev and curr tick states,
// assigns LOD tier based on screen-space hex size.

struct EntityTickData {
    pos: vec3f,
    facing: f32,
    owner: u32,
    entity_kind: u32,
    health_frac: f32,
    stamina_frac: f32,
    flags: u32,
    _pad: vec3f,
}

struct EntityGpuData {
    pos: vec3f,
    facing: f32,
    owner: u32,
    lod_tier: u32,
    entity_kind: u32,
    health_frac: f32,
    stamina_frac: f32,
    flags: u32,
    _pad: vec2f,
}

struct InterpolationUniforms {
    t: f32,
    entity_count: u32,
    camera_pos: vec3f,
    viewport_height: f32,
    // hex_world_radius / tan(fov/2) — precomputed for LOD calculation
    lod_scale: f32,
    _pad: vec3f,
}

@group(0) @binding(0) var<storage, read> prev_state: array<EntityTickData>;
@group(0) @binding(1) var<storage, read> curr_state: array<EntityTickData>;
@group(0) @binding(2) var<storage, read_write> render_state: array<EntityGpuData>;
@group(0) @binding(3) var<uniform> interp: InterpolationUniforms;

// LOD tier thresholds (in screen pixels of hex size)
const LOD_CLOSE_THRESHOLD: f32 = 60.0;
const LOD_MID_THRESHOLD: f32 = 20.0;

/// Lerp an angle, handling wrap-around at 2π.
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    var diff = b - a;
    // Normalize diff to [-π, π]
    diff = diff - floor((diff + 3.14159265) / 6.28318530) * 6.28318530;
    return a + diff * t;
}

@compute @workgroup_size(256)
fn interpolate(@builtin(global_invocation_id) id: vec3u) {
    let i = id.x;
    if i >= interp.entity_count {
        return;
    }

    let prev = prev_state[i];
    let curr = curr_state[i];
    let t = interp.t;

    // Interpolated position
    let pos = mix(prev.pos, curr.pos, vec3f(t));
    let facing = lerp_angle(prev.facing, curr.facing, t);

    // LOD tier based on distance from camera
    let dist = length(pos - interp.camera_pos);
    // Screen-space hex size ≈ (hex_radius / dist) * viewport_height * fov_factor
    let screen_hex = interp.lod_scale / max(dist, 1.0) * interp.viewport_height;

    var lod_tier: u32 = 2u; // far (heatmap)
    if screen_hex >= LOD_CLOSE_THRESHOLD {
        lod_tier = 0u; // close (body model later)
    } else if screen_hex >= LOD_MID_THRESHOLD {
        lod_tier = 1u; // mid (dot + facing)
    }

    render_state[i].pos = pos;
    render_state[i].facing = facing;
    render_state[i].owner = curr.owner;
    render_state[i].lod_tier = lod_tier;
    render_state[i].entity_kind = curr.entity_kind;
    render_state[i].health_frac = curr.health_frac;
    render_state[i].stamina_frac = curr.stamina_frac;
    render_state[i].flags = curr.flags;
    render_state[i]._pad = vec2f(0.0);
}
