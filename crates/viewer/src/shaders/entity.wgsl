// Entity rendering shader — instanced rendering from storage buffer.
// Each entity reads its transform from the EntityGpuData storage buffer.
// Close LOD: colored shape + facing indicator.
// Mid LOD: colored dot + directional tick mark.

struct CameraUniforms {
    view_proj: mat4x4f,
    camera_pos: vec3f,
    _pad0: f32,
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

struct EntityUniforms {
    target_lod: u32,
    entity_count: u32,
    _pad: vec2f,
}

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
@group(1) @binding(0) var<storage, read> entities: array<EntityGpuData>;
@group(1) @binding(1) var<uniform> entity_uniforms: EntityUniforms;

// Player colors (same as hex overlay)
fn player_color(owner: u32) -> vec3f {
    switch owner {
        case 0u: { return vec3f(0.2, 0.4, 0.9); }
        case 1u: { return vec3f(0.9, 0.2, 0.2); }
        case 2u: { return vec3f(0.2, 0.8, 0.3); }
        case 3u: { return vec3f(0.9, 0.7, 0.1); }
        case 4u: { return vec3f(0.7, 0.3, 0.8); }
        case 5u: { return vec3f(0.1, 0.8, 0.8); }
        case 6u: { return vec3f(0.9, 0.5, 0.2); }
        case 7u: { return vec3f(0.6, 0.6, 0.6); }
        default: { return vec3f(1.0, 1.0, 1.0); }
    }
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4f,
    @location(0) color: vec3f,
    @location(1) alpha: f32,
}

// Generate a simple shape per entity using vertex index.
// Close LOD: 8-sided polygon (16 triangles from center)
// Mid LOD: 4-sided diamond (2 triangles) + facing tick (2 triangles)
@vertex
fn vs_entity(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) instance_id: u32,
) -> VertexOutput {
    let entity = entities[instance_id];
    var out: VertexOutput;

    // Skip entities not matching target LOD tier
    if entity.lod_tier != entity_uniforms.target_lod {
        // Degenerate triangle — will be clipped
        out.clip_pos = vec4f(0.0, 0.0, -2.0, 1.0);
        out.color = vec3f(0.0);
        out.alpha = 0.0;
        return out;
    }

    let color = player_color(entity.owner);
    out.color = color;
    out.alpha = 1.0;

    let cos_f = cos(entity.facing);
    let sin_f = sin(entity.facing);

    var local_pos: vec2f;

    if entity_uniforms.target_lod == 0u {
        // Close LOD: octagon (24 verts = 8 triangles from center)
        let tri_idx = vid / 3u;
        let vert_in_tri = vid % 3u;

        if vert_in_tri == 0u {
            local_pos = vec2f(0.0, 0.0); // center
        } else {
            let corner = tri_idx + vert_in_tri - 1u;
            let angle = f32(corner) * 0.785398; // π/4
            let r = 1.5; // meters radius
            local_pos = vec2f(cos(angle) * r, sin(angle) * r);
        }
    } else {
        // Mid LOD: diamond (6 verts = 2 triangles) + facing line (6 verts)
        if vid < 6u {
            // Diamond body
            let tri_idx = vid / 3u;
            let vert_in_tri = vid % 3u;
            let offsets = array<vec2f, 4>(
                vec2f(0.0, 1.0),  // top
                vec2f(0.7, 0.0),  // right
                vec2f(0.0, -1.0), // bottom
                vec2f(-0.7, 0.0), // left
            );
            if tri_idx == 0u {
                local_pos = offsets[vert_in_tri];
            } else {
                let idx = array<u32, 3>(0u, 2u, 3u);
                local_pos = offsets[idx[vert_in_tri]];
            }
        } else {
            // Facing indicator: thin line from center outward
            let line_vert = vid - 6u;
            let line_offsets = array<vec2f, 6>(
                vec2f(-0.1, 0.0),
                vec2f(0.1, 0.0),
                vec2f(0.0, 2.0),
                vec2f(0.0, 2.0),
                vec2f(0.1, 0.0),
                vec2f(-0.1, 0.0),
            );
            local_pos = line_offsets[line_vert];
            out.color = color * 1.3; // brighter facing indicator
        }
    }

    // Rotate by facing angle
    let rotated = vec2f(
        local_pos.x * cos_f - local_pos.y * sin_f,
        local_pos.x * sin_f + local_pos.y * cos_f,
    );

    // World position (entity pos + local offset, Y from entity pos)
    let world_pos = vec3f(
        entity.pos.x + rotated.x,
        entity.pos.y + 0.5, // slight Y offset above terrain
        entity.pos.z + rotated.y,
    );

    out.clip_pos = camera.view_proj * vec4f(world_pos, 1.0);
    return out;
}

@fragment
fn fs_entity(in: VertexOutput) -> @location(0) vec4f {
    if in.alpha < 0.01 {
        discard;
    }
    return vec4f(in.color, in.alpha);
}
