# V3 wgpu Renderer — Plan

Status: **design-complete, ready for eng-lead decomposition**

## User stories

**As a spectator**, I want to view a theatre-scale conflict (100k tiles,
10k entities, 1k active combatants with body models) at 60fps with smooth
camera orbit, zoom, and pan.

**As the simulation platform**, I want the renderer to handle LOD
transitions seamlessly — density heatmap at strategic zoom, stack badges
at tactical zoom, individual body-model detail at close zoom — without
frame drops.

**As a future feature**, I want terrain to be mutable at runtime —
ditches dug, walls built, craters from explosions — so the terrain mesh
must support per-tile updates, not just static geometry.

## Architecture

### Core principle: Rust everywhere, GPU compute for bulk work

```
Rust Server (authoritative sim)
    |
    | WebSocket: tick state (entities, terrain deltas, body points)
    v
Rust WASM Module (browser)
    |
    |-- Deserialize tick state
    |-- Write entity state to GPU storage buffers
    |-- Write terrain deltas to GPU storage buffers
    |-- Submit GPU compute dispatches:
    |     - Interpolation (prev tick → current tick at render framerate)
    |     - LOD culling (camera frustum + distance tiers)
    |     - Body-point transform generation (for close-zoom combatants)
    |-- Submit GPU render passes:
    |     - Terrain pass (instanced hex tiles)
    |     - Entity pass (instanced by LOD tier)
    |     - Body-model pass (instanced limb primitives)
    |     - Overlay pass (health bars, selection indicators)
    |
    v
Browser Canvas (WebGPU)
    |
    | CSS overlay (pointer-events routing)
    v
SolidJS DOM UI (inspector, playback controls, status bars)
```

CPU work per frame: upload tick deltas (~640KB/tick at 20 ticks/sec),
dispatch compute shaders, submit render passes. GPU does all interpolation,
culling, and rendering. CPU is nearly idle between ticks.

## Terrain layer

### Two distinct surfaces: heightmap + hex overlay

The hex grid is a spatial index for the simulation (~100m per hex). The
terrain itself is a **continuous heightmap** at much higher resolution
(~1-2m per sample). These are separate rendering concerns:

1. **Heightmap mesh**: Dense triangle grid deformed by elevation data.
   A 1km × 1km area at 1m resolution = 1M height samples. This is the
   physical ground — ditches, walls, craters, hills all live here.
2. **Hex overlay**: Territory colors, borders, fog-of-war composited on
   top of the heightmap surface. Visual aid, not terrain geometry.

### Heightmap data model

Height and material stored as GPU textures (not per-hex buffers):

```rust
// R32Float texture: height in meters at each sample point
heightmap_texture: wgpu::Texture,  // e.g., 1024×1024 for a 1km² map at 1m res

// R8Uint texture: material/surface type index at each sample point
material_texture: wgpu::Texture,   // same resolution as heightmap
```

A 1024×1024 heightmap = 4MB (R32Float). Material map = 1MB (R8Uint).
Total: 5MB GPU memory for terrain. Trivial.

### Heightmap mesh strategy

**Dense triangle grid with vertex shader displacement.** The mesh is a
flat grid of triangles at heightmap resolution. The vertex shader samples
the height texture with bilinear/bicubic filtering to displace each
vertex vertically.

### Terrain shading (not Minecraft)

The heightmap resolution (1m) is the same as Minecraft's block size, but
the rendering is fundamentally different. Key techniques for natural
terrain:

**Per-pixel normals from heightmap gradient.** The fragment shader reads
4 adjacent height samples and computes the surface normal via cross
product. Hills look round, not faceted. Valleys darken naturally under
directional lighting.

**Slope-based material blending.** Where grassland meets rock, there's
no hard line. The fragment shader computes slope angle from the normal
and blends materials: rock on steep slopes, grass on flats, dirt on
moderate angles. A noise texture breaks up the blend boundary so
transitions look organic, not grid-aligned.

**Detail textures.** The material map at 1m gives macro color. Up close,
1m/texel is blurry. A tiling detail texture (repeating at ~0.25m) adds
fine-grain visual detail. Triplanar mapping on steep faces prevents
texture stretching on cliff faces.

**Atmospheric depth.** Distance fog fades far terrain to sky color,
hiding LOD transitions and giving depth.

```wgsl
@vertex
fn vs_terrain(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let grid_pos = index_to_grid(vid, grid_width);
    let uv = grid_pos / vec2f(grid_width, grid_height);
    // Bicubic sampling eliminates stairstepping at sample boundaries
    let height = bicubic_sample(heightmap, sampler, uv).r;

    var out: VertexOutput;
    out.position = camera.view_proj * vec4f(grid_pos.x, height, grid_pos.y, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_terrain(in: VertexOutput) -> @location(0) vec4f {
    // Per-pixel normal from heightmap gradient (4 adjacent samples)
    let h_l = textureSample(heightmap, sampler, in.uv + vec2f(-texel, 0.0)).r;
    let h_r = textureSample(heightmap, sampler, in.uv + vec2f( texel, 0.0)).r;
    let h_d = textureSample(heightmap, sampler, in.uv + vec2f(0.0, -texel)).r;
    let h_u = textureSample(heightmap, sampler, in.uv + vec2f(0.0,  texel)).r;
    let normal = normalize(vec3f(h_l - h_r, 2.0 * texel_world, h_d - h_u));

    // Material from material map
    let mat_idx = textureLoad(material_map, vec2i(in.uv * map_size), 0).r;

    // Slope-based blending: steep = rock, flat = grass, moderate = dirt
    let slope = 1.0 - normal.y;  // 0 = flat, 1 = vertical
    let noise = textureSample(noise_tex, sampler, in.uv * 7.3).r;
    let blend = slope_blend(mat_idx, slope, noise);

    // Detail texture (tiling, adds fine grain up close)
    let detail = triplanar_sample(detail_atlas, in.world_pos, normal, mat_idx);

    // Directional lighting
    let light = max(dot(normal, sun_dir), 0.0) * 0.7 + 0.3;  // diffuse + ambient

    // Atmospheric fog
    let fog = exp(-in.view_dist * fog_density);

    return vec4f(mix(fog_color, blend * detail * light, fog), 1.0);
}
```

### Terrain mutation flow

Mutations write to a **region of the heightmap texture**, not per-hex:

```
Server tick: "ditch dug from (450.0, 120.0) to (470.0, 120.0), depth 1.5m, width 2m"
    |
    v
WASM: compute affected texels (20m × 2m = ~40 texels at 1m res)
      write_texture() to update height values in that rectangle
    |
    v
GPU: next frame vertex shader reads updated heights, mesh deforms
```

| Mutation | Scale | Texels updated | Bytes written |
|----------|-------|----------------|---------------|
| Dig ditch (20m × 2m) | Small | ~40 | 160 bytes |
| Earthen wall (20m × 1m × 2m high) | Small | ~20 | 80 bytes |
| Building foundation (10m × 10m) | Medium | ~100 | 400 bytes |
| Bomb crater (15m radius) | Medium | ~700 | 2.8KB |
| Hillside collapse (50m × 30m) | Large | ~1,500 | 6KB |

All mutations use `queue.write_texture()` on a sub-region. No mesh
rebuild. No full texture re-upload. The vertex shader reads the updated
texture next frame.

### Hex-scoped chunk dirtying

The heightmap is stored as rectangular GPU texture chunks (e.g., 64×64
texels per chunk). A precomputed mapping links each medium hex (~150m)
to the set of rectangular chunks it overlaps. When the server reports
a terrain mutation within a hex, only the overlapping chunks are marked
dirty and re-uploaded. This bounds the update cost to the hex's footprint,
not the entire map.

```rust
struct TerrainChunkMap {
    /// For each medium hex, the rectangular chunks that overlap it.
    /// Precomputed at map init, ~4 chunks per hex on average.
    hex_to_chunks: HashMap<Axial, SmallVec<[ChunkId; 4]>>,
    /// Per-chunk dirty flag, cleared after GPU upload.
    dirty: Vec<bool>,
}
```

Mutation flow with hex scoping:
```
Server: "ditch dug at (450, 120)"
  → WASM: find medium hex containing (450, 120)
  → lookup hex_to_chunks → mark 1-2 chunks dirty
  → write_texture() for each dirty chunk's sub-region
  → clear dirty flags
```

This means a ditch across a hex boundary dirties ~2-4 chunks (not the
whole map), and a bomb crater affecting 3 hexes dirties ~6-12 chunks.

### Terrain LOD

At strategic zoom, the full-resolution grid is too dense. LOD via
**clipmap or geometry clipmap**:

| Distance from camera | Grid resolution | Triangles |
|---------------------|-----------------|-----------|
| Near (0-200m) | 1m (full res) | ~160k |
| Mid (200-1km) | 4m (quarter res) | ~40k |
| Far (1-5km) | 16m | ~10k |
| Horizon (>5km) | 64m | ~2.5k |
| **Total** | | **~212k triangles** |

Geometry clipmaps are a solved GPU technique (Losasso & Hoppe, 2004).
Each ring is a pre-built grid mesh at its resolution; the vertex shader
samples the same heightmap texture at different mip levels.

### Hex overlay

The hex grid is a separate render pass drawn ON TOP of the terrain:

- At close zoom: hex borders as line segments projected onto the
  terrain surface (sample heightmap at border vertices)
- At mid zoom: filled hex polygons with alpha for territory color,
  draped onto terrain
- At strategic zoom: solid colored regions (merge hexes by owner)

The hex overlay reads the same heightmap texture to follow terrain
contours. It's a visual layer, not geometry — if a ditch crosses a
hex boundary, the hex overlay follows the ditch's height.

### Hex overlay data model

Per-hex metadata (separate from terrain heightmap):

```rust
#[repr(C)]
struct HexOverlayData {
    owner: u32,           // territory owner (player color)
    flags: u32,           // bitfield: has_road, has_river, fog_state, etc.
}
```

100k hexes × 8 bytes = 800KB GPU buffer. Updated when territory changes.

## Entity layer

### Instance buffer layout

Per-entity GPU data:

```rust
#[repr(C)]
struct EntityGpuData {
    // Interpolated position (written by compute shader)
    pos: [f32; 3],
    facing: f32,
    // Visual state
    owner: u32,           // player color index
    lod_tier: u32,        // 0=close, 1=mid, 2=far (written by culling compute)
    entity_kind: u32,     // person, structure, etc.
    health_frac: f32,     // 0-1 for health bar
    stamina_frac: f32,    // 0-1 for stamina bar
    flags: u32,           // bitfield: has_body_model, is_dead, is_staggered, etc.
    _pad: [f32; 2],       // align to 48 bytes
}
```

10k entities × 48 bytes = 480KB GPU buffer.

### Entity LOD tiers

| Tier | Condition | Rendering | Instance count |
|------|-----------|-----------|----------------|
| Close | hex >= 60 screen px | Body-model primitives (separate pass) | ~50-200 |
| Mid | hex >= 20 screen px | Colored dot + facing indicator | ~500-2000 |
| Far | hex < 20 screen px | Density heatmap (compute shader aggregates per-hex) | N/A (compute output) |

LOD assignment is a **GPU compute shader**: for each entity, compute
screen-space hex size from camera, write lod_tier to instance buffer.
Render passes filter by lod_tier using indirect draw counts.

### Interpolation compute shader

```wgsl
@group(0) @binding(0) var<storage, read> prev_state: array<EntityTickData>;
@group(0) @binding(1) var<storage, read> curr_state: array<EntityTickData>;
@group(0) @binding(2) var<storage, read_write> render_state: array<EntityGpuData>;
@group(0) @binding(3) var<uniform> interp: InterpolationUniforms; // { t: f32 }

@compute @workgroup_size(256)
fn interpolate(@builtin(global_invocation_id) id: vec3u) {
    let i = id.x;
    if (i >= arrayLength(&prev_state)) { return; }

    let prev = prev_state[i];
    let curr = curr_state[i];
    let t = interp.t;

    render_state[i].pos = mix(prev.pos, curr.pos, t);
    render_state[i].facing = lerp_angle(prev.facing, curr.facing, t);
    // ... copy visual state from curr
}
```

Dispatched every frame with `t = (frame_time - tick_time) / tick_interval`.
10k entities at workgroup size 256 = 40 workgroups. Executes in microseconds.

## Body-model layer

### When active

Body-model rendering activates for entities at close LOD tier that have
`has_body_model` flag set. Typically 50-200 entities simultaneously.

### Data model

Per-entity body points stored in a separate storage buffer:

```rust
#[repr(C)]
struct BodyModelGpuData {
    points: [[f32; 4]; 16],  // 16 body points, xyz + radius (w)
    // Total: 256 bytes per entity
}
```

200 body models × 256 bytes = 51.2KB. Trivial.

### Body-model rendering

Each limb segment is a capsule (two hemispheres + cylinder) rendered as
an instanced mesh. 16 segments per entity × 200 entities = 3,200 capsule
instances. Single draw call with per-instance transforms read from the
body-point storage buffer.

A vertex shader computes capsule geometry from the two endpoint positions
and radius stored in the body-point buffer.

### Body-model interpolation

Same pattern as entity interpolation — a compute shader lerps body points
between ticks. The server sends body points at tick rate; the GPU
interpolates at frame rate.

## Multi-resolution hex spatial index

### Why multi-resolution

Different engine systems need spatial queries at different scales:
- **Combat**: "enemies within 5m weapon reach" — needs ~10m cells
- **Tactical AI**: "terrain and units in this 200m area" — needs ~150m cells
- **Strategic AI**: "resources and army strength in 2km region" — needs ~500m cells
- **Rendering**: "which terrain chunks are visible" — needs coarse culling
- **Terrain mutation**: "which chunks to dirty for this ditch" — needs hex→chunk mapping

A single 150m hex grid (the current `SpatialIndex`) is too coarse for
combat and too fine for strategic queries.

### Architecture: three independent flat hex grids

NOT H3-style nested hierarchy (aperture-7 rotation breaks LOD/chunk
alignment). Instead, three independent hex grids at different resolutions.
No parent-child bit tricks — just three separate indices.

| Level | Hex radius | Cells (3km map) | Updated | Used by |
|-------|-----------|-----------------|---------|---------|
| Fine | ~10m | ~90k | Every tick | Combat range queries, body-model collision, narrow-phase |
| Medium | ~150m (existing) | ~400 | On hex change (hysteresis) | Pathfinding, tactical AI, movement steering, terrain chunk dirtying |
| Coarse | ~500m | ~36 | On medium change | Strategic AI aggregates, rendering frustum culling |

### Why hexes at all levels (not squares for fine/coarse)

- **Uniform neighbor distance**: all 6 neighbors equidistant. Square grids
  have diagonal neighbors at √2× distance — a 5m combat range check on a
  square grid either misses diagonal targets or over-includes corners.
- **Better circle approximation**: hex k-ring disk is closer to a circle
  than a square k-ring. Tighter culling for "all entities within Xm."
- **Consistent mental model**: every system thinks in hexes. No mixing
  coordinate systems.

### Cross-level mapping

Precomputed at map init, stored as flat arrays:

```rust
/// For each fine hex, which medium hex contains it.
fine_to_medium: Vec<Axial>,          // indexed by fine hex linear index

/// For each medium hex, which coarse hex contains it.
medium_to_coarse: Vec<Axial>,        // indexed by medium hex linear index

/// For each medium hex, which fine hexes it contains.
medium_to_fine: Vec<SmallVec<[u32; 32]>>,

/// For each medium hex, which rectangular terrain chunks overlap it.
medium_to_chunks: Vec<SmallVec<[ChunkId; 4]>>,
```

### Entity update flow across levels

```rust
fn on_entity_move(entity: EntityKey, old_pos: Vec3, new_pos: Vec3) {
    // Fine: always update (cheap — hash table insert/remove)
    let old_fine = pos_to_fine_hex(old_pos);
    let new_fine = pos_to_fine_hex(new_pos);
    if new_fine != old_fine {
        fine_index.move_entity(old_fine, new_fine, entity);
    }

    // Medium: update only if hex changed (existing hysteresis)
    let old_med = entity_hex[entity];
    let new_med = update_hex_membership(old_med, new_pos); // with hysteresis
    if new_med != old_med {
        medium_index.move_entity(old_med, new_med, entity);
        entity_hex[entity] = new_med;

        // Coarse: update only if coarse hex changed (~rare)
        let old_coarse = medium_to_coarse[old_med];
        let new_coarse = medium_to_coarse[new_med];
        if new_coarse != old_coarse {
            coarse_index.update_aggregates(old_coarse, new_coarse, entity);
        }
    }
}
```

Fine updates happen every tick for moving entities (~10k moves/tick).
Medium updates happen only on hex boundary crossing (~100/tick).
Coarse updates happen rarely (~5/tick).

### Coarse hex aggregates

Each coarse hex stores precomputed summaries for strategic AI:

```rust
struct CoarseHexAggregate {
    population: [u16; MAX_PLAYERS],    // entity count per player
    army_strength: [f32; MAX_PLAYERS], // sum of combat effectiveness
    resource_totals: ResourceTotals,   // food, material, etc.
    terrain_profile: TerrainProfile,   // avg height, dominant material, road coverage
}
```

Strategic AI queries are O(1) lookups into these aggregates instead of
iterating all entities.

## Camera system

### Orbit camera

- **Target point**: world position the camera orbits around
- **Distance**: zoom level (orbit radius)
- **Azimuth**: horizontal rotation angle
- **Elevation**: vertical angle (clamped 10°-85°)
- **Projection**: perspective for close zoom, smooth transition to
  orthographic at strategic zoom

### Input mapping

| Input | Action |
|-------|--------|
| Scroll wheel | Zoom in/out (adjust distance) |
| Middle-drag | Orbit (adjust azimuth/elevation) |
| Right-drag | Pan (move target point) |
| Left-click | Select entity |
| WASD | Pan camera target |
| Q/E | Rotate azimuth |

Built with winit event handling in WASM. Camera state is a Rust struct;
projection/view matrices computed with glam and uploaded as a uniform buffer.

### Camera-driven LOD

The camera's view-projection matrix feeds the LOD compute shader.
Screen-space hex size = `hex_world_radius / camera_distance * viewport_height`.
This drives terrain LOD tier, entity LOD tier, and body-model activation.

## Overlay layer

Health bars, stamina bars, selection highlights, and damage numbers are
rendered as textured quads in screen space, positioned by projecting
entity world positions through the camera.

Text rendering uses a pre-baked glyph atlas (SDF font atlas generated
at build time). Glyphon or a custom SDF text renderer.

## SolidJS integration

SolidJS owns the DOM. wgpu owns the canvas. They coexist via CSS layering:

```html
<div style="position: relative">
  <canvas id="wgpu-canvas" style="position: absolute; inset: 0" />
  <div id="solid-ui" style="position: absolute; inset: 0; pointer-events: none">
    <!-- SolidJS mounts here. Interactive elements get pointer-events: auto -->
    <InspectorPanel />
    <PlaybackControls />
    <ScoreBar />
  </div>
</div>
```

Communication: SolidJS reads entity state from the WASM module's exported
accessors (selected entity info, game clock, scores). SolidJS writes user
actions (select entity, toggle layer, change playback speed) via WASM
function calls.

## Module structure

### Shared protocol crate: `crates/protocol/`

Shared types between server and viewer. Compiles for both native (server)
and wasm32 (viewer) targets. MessagePack serialization.

```
crates/protocol/
├── Cargo.toml
└── src/
    ├── lib.rs              -- re-exports
    ├── tick.rs             -- TickMessage, TerrainDelta, EntitySnapshot
    ├── entity.rs           -- SpectatorEntityInfo, BodyZone, WoundSeverity
    ├── terrain.rs          -- HeightmapPatch, MaterialPatch
    └── init.rs             -- InitMessage (map dimensions, full heightmap, full entity list)
```

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
rmp-serde = "1"
glam = { version = "0.30", features = ["serde"] }
```

MessagePack over the wire, not JSON. Rationale:
- 2-4x faster deserialization in Rust vs serde_json
- 30-50% smaller payloads (no field name strings)
- The viewer is Rust WASM — no JS debugging advantage from JSON
- Debugging: browser WS inspector shows binary frames, but the WASM
  module can expose a debug endpoint that decodes to human-readable text

### Rust WASM crate: `crates/viewer/`

```
crates/viewer/
├── Cargo.toml
├── src/
│   ├── lib.rs              -- wasm_bindgen entry, event loop, tick ingestion
│   ├── gpu.rs              -- wgpu device/surface setup, pipeline creation
│   ├── heightmap.rs        -- continuous terrain mesh, clipmap LOD, texture updates
│   ├── hex_overlay.rs      -- hex grid borders, territory colors, draped on terrain
│   ├── entities.rs         -- entity instance buffer, LOD, interpolation dispatch
│   ├── body_model.rs       -- body-point buffer, capsule instancing
│   ├── camera.rs           -- orbit camera, projection, input → camera state
│   ├── input.rs            -- winit event handling, mouse/keyboard → actions
│   ├── overlay.rs          -- health bars, selection, text quads
│   ├── text.rs             -- SDF glyph atlas, text rendering
│   ├── lod.rs              -- LOD compute shader dispatch, tier assignment
│   └── shaders/
│       ├── terrain.wgsl    -- heightmap displacement vertex/fragment
│       ├── hex_overlay.wgsl -- hex border/fill vertex/fragment (terrain-draped)
│       ├── entity.wgsl     -- entity instance vertex/fragment shader
│       ├── body.wgsl       -- capsule vertex/fragment shader
│       ├── interpolate.wgsl -- entity + body-point interpolation compute
│       ├── lod.wgsl        -- LOD assignment compute
│       ├── overlay.wgsl    -- screen-space quad vertex/fragment
│       └── text.wgsl       -- SDF text vertex/fragment
└── assets/
    └── font_atlas.png      -- pre-baked SDF font atlas
```

### Dependencies

```toml
[dependencies]
simulate-everything-protocol = { path = "../protocol" }
wgpu = { version = "24", features = ["webgpu"] }
winit = "0.31"
glam = "0.30"
web-sys = { version = "0.3", features = ["WebSocket", "MessageEvent", ...] }
wasm-bindgen = "0.2"
js-sys = "0.3"
rmp-serde = "1"
```

### Build: Trunk

Trunk is the WASM build tool. It wraps `cargo build --target wasm32`,
`wasm-bindgen`, `wasm-opt`, and asset bundling into a single command.
It serves a dev server with auto-rebuild on file change (not hot-reload —
page refreshes, state lost, but rebuild is automatic).

Why Trunk over manual build:
- One command (`trunk serve`) vs a 4-step shell pipeline
- Handles wasm-bindgen glue generation, wasm-opt compression, asset
  copying, HTML template injection automatically
- Dev server with auto-rebuild and live-reload (page refresh)
- Production build (`trunk build --release`) produces optimized output
- Eject path is trivial: Trunk's steps are standard cargo + wasm-bindgen
  + wasm-opt. If Trunk becomes a problem, replace with a Makefile.

```bash
# Dev
cd crates/viewer && trunk serve --open

# Production
cd crates/viewer && trunk build --release
# Output: crates/viewer/dist/ (WASM + JS glue + HTML + assets)
```

Bundle size target: <3MB gzipped (wgpu + Naga + viewer code).

## Migration path from PixiJS

### Phase 0: Protocol crate + MessagePack
- Extract `crates/protocol/` with shared types (tick, entity, terrain, init)
- MessagePack serialization (rmp-serde)
- Server sends msgpack over WS; existing JSON protocol kept as fallback
  during migration (viewer negotiates format on connect)
- **Gate**: server and a test client exchange msgpack tick data correctly

### Phase 1: wgpu scaffold + terrain
- Set up crates/viewer with wgpu + winit WASM target, Trunk build
- Heightmap mesh with clipmap LOD (dense near camera, coarse far)
- Vertex shader displacement from height texture
- Material texture for surface coloring
- Hex overlay as separate render pass (territory colors, borders)
- Orbit camera with mouse controls
- SolidJS UI overlay with inspector panel
- **Gate**: terrain renders correctly with continuous heightmap, camera
  orbits, heightmap sub-region update works (ditch/wall mutation)

### Phase 2: Entity rendering
- Entity instance buffer with LOD tiers
- Interpolation compute shader (server ticks → 60fps)
- Close-zoom: colored dot + facing indicator
- Mid-zoom: stack badges
- Far-zoom: density heatmap (compute shader)
- **Gate**: 10k entities render at 60fps across all LOD tiers

### Phase 3: Body-model rendering
- Body-point storage buffer
- Capsule instancing for limb segments
- Body-point interpolation compute shader
- Wound tinting, weapon rendering, shield disc
- **Gate**: 200 body-model entities render at 60fps with visible combat poses

### Phase 4: Overlays + polish
- Health/stamina bars as screen-space quads
- SDF text rendering for labels
- Selection highlighting
- Damage number popups
- **Gate**: full feature parity with current PixiJS renderer

### Phase 5: Delete PixiJS
- Remove pixi.js dependency from frontend/package.json
- Remove all PixiJS rendering code (v3/render/*.ts, v3/HexCanvas.tsx)
- Remove V1/V2 frontend code (Board.tsx, HexBoard.tsx, HexCanvas.tsx, etc.)
- Remove V1/V2 backend routes and WebSocket handlers
- **Gate**: clean build, no PixiJS references, all viewer features work via wgpu

## Terrain mutability detail

All mutations write to sub-regions of the heightmap and/or material
textures. The vertex shader reads the updated textures next frame.
No mesh rebuild ever.

### Near-term mutations

| Action | Heightmap effect | Material effect | Scale |
|--------|-----------------|-----------------|-------|
| Dig ditch | Lower height samples along path | → dirt | 20-50m path, 1-2m wide |
| Earthen wall | Raise height samples along path | → packed_earth | 20-50m path, 1m wide |
| Building foundation | Flatten height samples in footprint | → stone/wood | 10-20m square |
| Road | Smooth height gradient along path | → gravel/cobble | Arbitrary length, 3-5m wide |

### Far-term mutations

| Action | Heightmap effect | Material effect | Scale |
|--------|-----------------|-----------------|-------|
| Bomb crater | Radial height falloff (parabolic) from impact point | → rubble in crater, debris ring around | 10-30m radius |
| Fire | No height change | → charred | Spread area |
| Flooding | No height change (water is separate surface) | No change | Watershed |
| Erosion | Gradual height reduction along water flow paths | → exposed rock/clay | Slow, large area |
| Mining | Lower height in extraction area | → excavated/ore | 10-50m area |

### Water surface (future)

Water is a separate render pass with its own surface level per cell,
transparency, and flow visualization. Not part of the heightmap. The
water surface clips against the heightmap — water fills depressions,
flows downhill. Deferred until water mechanics land in the engine.

## Performance budget

| Component | Target | Measurement |
|-----------|--------|-------------|
| Terrain heightmap mesh (~212k tris via clipmap) | <1.5ms GPU | 4-5 draw calls at different LOD rings |
| Hex overlay (100k hexes) | <0.5ms GPU | Instanced hex outlines/fills |
| Entity interpolation compute (10k) | <0.1ms GPU | 40 workgroups × 256 |
| Entity render (10k across LOD tiers) | <0.5ms GPU | 3 instanced draws |
| Body-model render (200 entities) | <0.3ms GPU | 3,200 capsule instances |
| LOD compute (10k entities) | <0.1ms GPU | 40 workgroups |
| Overlay render | <0.2ms GPU | Screen-space quads |
| **Total GPU frame time** | **<3.2ms** | **Target: 60fps = 16.6ms budget** |
| CPU per-tick work | <2ms | Deserialize + buffer upload |
| CPU per-frame work | <0.5ms | Submit compute + render passes |
| Terrain mutation (worst case: 1500 texels) | <0.1ms | queue.write_texture sub-region |

5x headroom below the 60fps ceiling.

## Verification criteria

- [ ] Continuous heightmap terrain renders with clipmap LOD at 60fps
- [ ] Heightmap sub-region mutation (ditch/wall/crater) updates in <1 frame
- [ ] Hex overlay drapes correctly onto terrain surface
- [ ] 10k entities render across LOD tiers at 60fps
- [ ] Interpolation compute shader produces smooth 60fps movement from 20 tick/sec input
- [ ] Body-model capsules render for 200 entities at close zoom
- [ ] Camera orbit, pan, zoom with mouse + keyboard
- [ ] SolidJS UI overlays render on top of wgpu canvas
- [ ] SolidJS can read entity state from WASM module
- [ ] Works in Chrome, Firefox, and Safari (WebGPU)
- [ ] Bundle size <3MB gzipped
- [ ] Startup time <2 seconds on broadband

## Resolved decisions

1. **MessagePack** for WebSocket tick data. Both endpoints are Rust
   (server native, viewer WASM). 2-4x faster deserialization, 30-50%
   smaller payloads. Debugging via WASM-side decode-to-text endpoint.

2. **Shared `crates/protocol/` crate** for tick/entity/terrain types.
   Compiles for both native and wasm32. Single source of truth for
   wire format.

3. **Trunk** for WASM build pipeline. One-command dev server with
   auto-rebuild. Eject path is trivial (standard cargo + wasm-bindgen
   + wasm-opt). See Build section for details.

## Open questions

1. **Heightmap resolution vs map scale**: At 1m per sample, a 10km × 10km
   map = 100M samples = 400MB. Need to decide: fixed resolution? Adaptive?
   Streaming chunks? Probably chunked loading with a viewport-centered
   window, similar to how game engines handle open-world terrain.

2. **Water rendering**: Separate transparent surface with per-cell water
   level, flow visualization. Deferred until water mechanics land in
   the engine. Needs its own render pass after terrain, before entities.

3. **Normal map generation**: For terrain lighting, normals should be
   computed from the heightmap (compute shader or at mutation time).
   Affects visual quality significantly. Include in Phase 1 or defer?
