struct BodyTickData {
    points: array<vec4f, 16>,
    weapon_a: vec4f,
    weapon_b: vec4f,
    shield_center: vec4f,
    shield_normal: vec4f,
    owner: u32,
    wound_mask: u32,
    _pad0: u32,
    _pad1: u32,
}

struct InterpUniforms {
    t: f32,
    body_count: u32,
    _pad: vec2u,
}

@group(0) @binding(0) var<storage, read> prev_state: array<BodyTickData>;
@group(0) @binding(1) var<storage, read> curr_state: array<BodyTickData>;
@group(0) @binding(2) var<storage, read_write> render_state: array<BodyTickData>;
@group(0) @binding(3) var<uniform> uniforms: InterpUniforms;

@compute @workgroup_size(64)
fn interpolate_body(@builtin(global_invocation_id) id: vec3u) {
    let i = id.x;
    if (i >= uniforms.body_count) {
        return;
    }

    let prev = prev_state[i];
    let curr = curr_state[i];
    let t = uniforms.t;

    for (var point_idx: u32 = 0u; point_idx < 16u; point_idx++) {
        render_state[i].points[point_idx] = mix(prev.points[point_idx], curr.points[point_idx], t);
    }
    render_state[i].weapon_a = mix(prev.weapon_a, curr.weapon_a, t);
    render_state[i].weapon_b = mix(prev.weapon_b, curr.weapon_b, t);
    render_state[i].shield_center = mix(prev.shield_center, curr.shield_center, t);
    render_state[i].shield_normal = mix(prev.shield_normal, curr.shield_normal, t);
    render_state[i].owner = curr.owner;
    render_state[i].wound_mask = curr.wound_mask;
    render_state[i]._pad0 = 0u;
    render_state[i]._pad1 = 0u;
}
