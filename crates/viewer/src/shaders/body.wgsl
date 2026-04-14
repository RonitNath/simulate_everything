struct CameraUniforms {
    view_proj: mat4x4f,
    camera_pos: vec3f,
    _pad0: f32,
}

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

struct BodyUniforms {
    body_count: u32,
    _pad: vec3u,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4f,
    @location(0) color: vec3f,
    @location(1) local_xy: vec2f,
    @location(2) segment_len: f32,
    @location(3) radius: f32,
    @location(4) shape_kind: f32,
}

const BODY_SLOT_COUNT: u32 = 14u;
const SHAPE_CAPSULE: f32 = 0.0;
const SHAPE_DISC: f32 = 1.0;

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(1) @binding(0) var<storage, read> bodies: array<BodyTickData>;
@group(1) @binding(1) var<uniform> body_uniforms: BodyUniforms;

fn player_color(owner: u32) -> vec3f {
    switch owner {
        case 0u: { return vec3f(0.2, 0.4, 0.9); }
        case 1u: { return vec3f(0.9, 0.2, 0.2); }
        case 2u: { return vec3f(0.2, 0.8, 0.3); }
        case 3u: { return vec3f(0.9, 0.7, 0.1); }
        case 4u: { return vec3f(0.7, 0.3, 0.8); }
        case 5u: { return vec3f(0.1, 0.8, 0.8); }
        default: { return vec3f(0.9, 0.9, 0.9); }
    }
}

fn wound_zone_for_slot(slot: u32) -> u32 {
    switch slot {
        case 0u, 1u: { return 0u; } // head
        case 2u, 3u: { return 1u; } // torso
        case 4u, 5u: { return 2u; } // left arm
        case 6u, 7u: { return 3u; } // right arm
        default: { return 4u; } // legs
    }
}

fn segment_point_a(slot: u32) -> u32 {
    switch slot {
        case 0u: { return 0u; }
        case 1u: { return 0u; }
        case 2u: { return 1u; }
        case 3u: { return 8u; }
        case 4u: { return 2u; }
        case 5u: { return 4u; }
        case 6u: { return 3u; }
        case 7u: { return 5u; }
        case 8u: { return 10u; }
        case 9u: { return 12u; }
        case 10u: { return 11u; }
        default: { return 13u; }
    }
}

fn segment_point_b(slot: u32) -> u32 {
    switch slot {
        case 0u: { return 0u; }
        case 1u: { return 1u; }
        case 2u: { return 8u; }
        case 3u: { return 9u; }
        case 4u: { return 4u; }
        case 5u: { return 6u; }
        case 6u: { return 5u; }
        case 7u: { return 7u; }
        case 8u: { return 12u; }
        case 9u: { return 14u; }
        case 10u: { return 13u; }
        default: { return 15u; }
    }
}

fn segment_radius(slot: u32) -> f32 {
    switch slot {
        case 0u: { return 0.12; }
        case 1u: { return 0.06; }
        case 2u: { return 0.18; }
        case 3u: { return 0.16; }
        case 4u, 6u: { return 0.05; }
        case 5u, 7u: { return 0.04; }
        case 8u, 10u: { return 0.08; }
        default: { return 0.06; }
    }
}

fn decode_wound(mask: u32, zone: u32) -> f32 {
    let bits = (mask >> (zone * 3u)) & 0x7u;
    return min(f32(bits) / 3.0, 1.0);
}

fn quad_corner(vid: u32) -> vec2f {
    switch vid {
        case 0u: { return vec2f(0.0, 0.0); }
        case 1u: { return vec2f(1.0, 0.0); }
        case 2u: { return vec2f(0.0, 1.0); }
        case 3u: { return vec2f(0.0, 1.0); }
        case 4u: { return vec2f(1.0, 0.0); }
        default: { return vec2f(1.0, 1.0); }
    }
}

@vertex
fn vs_body(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) instance_id: u32,
) -> VertexOutput {
    var out: VertexOutput;
    let body_index = instance_id / BODY_SLOT_COUNT;
    let slot = instance_id % BODY_SLOT_COUNT;

    if (body_index >= body_uniforms.body_count) {
        out.clip_pos = vec4f(0.0, 0.0, -2.0, 1.0);
        out.color = vec3f(0.0);
        out.local_xy = vec2f(0.0);
        out.segment_len = 0.0;
        out.radius = 0.0;
        out.shape_kind = SHAPE_CAPSULE;
        return out;
    }

    let body = bodies[body_index];
    let base_color = player_color(body.owner);
    let corner = quad_corner(vid);

    if (slot < 12u) {
        let a = body.points[segment_point_a(slot)].xyz;
        let b = body.points[segment_point_b(slot)].xyz;
        let radius = segment_radius(slot);
        let axis = b - a;
        let len = max(length(axis), 0.001);
        let dir = axis / len;
        let view_dir = normalize(camera.camera_pos - (a + b) * 0.5);
        var right = cross(view_dir, dir);
        if (length(right) < 0.001) {
            right = vec3f(1.0, 0.0, 0.0);
        }
        right = normalize(right);

        let along = mix(-radius, len + radius, corner.x);
        let lateral = mix(-radius, radius, corner.y);
        let world = a + dir * along + right * lateral;
        let tint = decode_wound(body.wound_mask, wound_zone_for_slot(slot));

        out.clip_pos = camera.view_proj * vec4f(world, 1.0);
        out.color = mix(base_color, vec3f(0.65, 0.05, 0.08), tint);
        out.local_xy = vec2f(along, lateral);
        out.segment_len = len;
        out.radius = radius;
        out.shape_kind = SHAPE_CAPSULE;
        return out;
    }

    if (slot == 12u) {
        if (body.weapon_b.w < 0.5) {
            out.clip_pos = vec4f(0.0, 0.0, -2.0, 1.0);
            out.color = vec3f(0.0);
            out.local_xy = vec2f(0.0);
            out.segment_len = 0.0;
            out.radius = 0.0;
            out.shape_kind = SHAPE_CAPSULE;
            return out;
        }
        let a = body.weapon_a.xyz;
        let b = body.weapon_b.xyz;
        let radius = body.weapon_a.w;
        let axis = b - a;
        let len = max(length(axis), 0.001);
        let dir = axis / len;
        let view_dir = normalize(camera.camera_pos - (a + b) * 0.5);
        var right = cross(view_dir, dir);
        if (length(right) < 0.001) {
            right = vec3f(1.0, 0.0, 0.0);
        }
        right = normalize(right);
        let along = mix(-radius, len + radius, corner.x);
        let lateral = mix(-radius, radius, corner.y);
        let world = a + dir * along + right * lateral;
        out.clip_pos = camera.view_proj * vec4f(world, 1.0);
        out.color = vec3f(0.92, 0.92, 0.95);
        out.local_xy = vec2f(along, lateral);
        out.segment_len = len;
        out.radius = radius;
        out.shape_kind = SHAPE_CAPSULE;
        return out;
    }

    if (body.shield_normal.w < 0.5) {
        out.clip_pos = vec4f(0.0, 0.0, -2.0, 1.0);
        out.color = vec3f(0.0);
        out.local_xy = vec2f(0.0);
        out.segment_len = 0.0;
        out.radius = 0.0;
        out.shape_kind = SHAPE_DISC;
        return out;
    }

    let center = body.shield_center.xyz;
    let normal = normalize(body.shield_normal.xyz);
    let radius = body.shield_center.w;
    let up = select(vec3f(0.0, 0.0, 1.0), vec3f(0.0, 1.0, 0.0), abs(normal.z) > 0.8);
    let tangent = normalize(cross(up, normal));
    let bitangent = normalize(cross(normal, tangent));
    let local = vec2f(mix(-radius, radius, corner.x), mix(-radius, radius, corner.y));
    let world = center + tangent * local.x + bitangent * local.y;

    out.clip_pos = camera.view_proj * vec4f(world, 1.0);
    out.color = vec3f(0.55, 0.55, 0.6);
    out.local_xy = local;
    out.segment_len = 0.0;
    out.radius = radius;
    out.shape_kind = SHAPE_DISC;
    return out;
}

@fragment
fn fs_body(in: VertexOutput) -> @location(0) vec4f {
    if (in.shape_kind < 0.5) {
        let closest_x = clamp(in.local_xy.x, 0.0, in.segment_len);
        let dist = distance(vec2f(in.local_xy.x, in.local_xy.y), vec2f(closest_x, 0.0));
        if (dist > in.radius) {
            discard;
        }
    } else {
        if (length(in.local_xy) > in.radius) {
            discard;
        }
    }

    return vec4f(in.color, 1.0);
}
