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

### Mutable hex terrain

Terrain is NOT static. The mesh must support per-tile mutations at runtime:
- **Near term**: dig ditches (lower tile height), build earthen walls
  (raise tile height, change material), place structures
- **Far term**: bomb craters (destroy tiles, create rubble, lower height
  in blast radius), fire damage (change material/color), flooding
  (water level per tile)

### Data model

Per-tile state stored in a GPU storage buffer:

```rust
#[repr(C)]
struct TilGpuData {
    height: f32,          // terrain elevation (mutable)
    material: u32,        // biome/surface type index (mutable)
    color_override: u32,  // territory owner tint (packed RGBA)
    flags: u32,           // bitfield: has_road, has_river, has_structure, damaged, etc.
}
```

100k tiles × 16 bytes = 1.6MB GPU buffer. Fits comfortably in any GPU.

### Terrain mesh strategy

**Instanced hex tiles, NOT a single merged mesh.** Rationale:
- Per-tile height/material mutations require updating individual tiles
- A merged mesh would need partial re-upload or full rebuild on mutation
- Instanced rendering: one hex polygon template, 100k instances with
  per-instance height/material/color from the storage buffer
- GPU vertex shader reads per-instance data and offsets the hex template
- Mutation = update 16 bytes in the storage buffer for that tile

### Terrain update flow

```
Server tick: "tile (34, 17) height changed from 5.0 to 3.2, material = rubble"
    |
    v
WASM: write 16 bytes at offset (34*width + 17) * 16 in terrain buffer
    |
    v
GPU: next frame reads updated height/material, renders correctly
```

No mesh rebuild. No re-upload of the entire terrain. Single tile update
cost: 16 bytes written to a mapped buffer region.

### Terrain LOD

At strategic zoom (100k tiles visible), full hex outlines are expensive.
LOD tiers for terrain:

| Tier | Condition | Rendering |
|------|-----------|-----------|
| Detail | <500 tiles visible | Full hex outlines, height shading, road/river lines |
| Standard | 500-10k tiles visible | Filled hex polygons, no outlines, color-coded by height+material |
| Coarse | >10k tiles visible | Merge adjacent same-material hexes into larger colored regions (compute shader) |

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

### Rust WASM crate: `crates/viewer/`

```
crates/viewer/
├── Cargo.toml
├── src/
│   ├── lib.rs              -- wasm_bindgen entry, event loop, tick ingestion
│   ├── gpu.rs              -- wgpu device/surface setup, pipeline creation
│   ├── terrain.rs          -- hex terrain mesh, instance buffer, tile updates
│   ├── entities.rs         -- entity instance buffer, LOD, interpolation dispatch
│   ├── body_model.rs       -- body-point buffer, capsule instancing
│   ├── camera.rs           -- orbit camera, projection, input → camera state
│   ├── input.rs            -- winit event handling, mouse/keyboard → actions
│   ├── overlay.rs          -- health bars, selection, text quads
│   ├── text.rs             -- SDF glyph atlas, text rendering
│   ├── lod.rs              -- LOD compute shader dispatch, tier assignment
│   └── shaders/
│       ├── terrain.wgsl    -- hex tile vertex/fragment shader
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
wgpu = { version = "24", features = ["webgpu"] }
winit = "0.31"
glam = "0.30"
web-sys = { version = "0.3", features = ["WebSocket", "MessageEvent", ...] }
wasm-bindgen = "0.2"
js-sys = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"  # or rmp-serde for msgpack
```

### Build

```bash
# Dev (with trunk)
cd crates/viewer && trunk serve

# Production
cargo build --target wasm32-unknown-unknown --release -p simulate-everything-viewer
wasm-bindgen --out-dir frontend/dist/wasm --target web target/wasm32-unknown-unknown/release/simulate_everything_viewer.wasm
wasm-opt -Oz -o frontend/dist/wasm/viewer_bg.wasm frontend/dist/wasm/viewer_bg.wasm
```

Bundle size target: <3MB gzipped (wgpu + Naga + viewer code).

## Migration path from PixiJS

### Phase 1: wgpu scaffold + terrain
- Set up crates/viewer with wgpu + winit WASM target
- Render hex terrain as instanced tiles with height/material coloring
- Orbit camera with mouse controls
- SolidJS UI overlay with inspector panel
- **Gate**: terrain renders correctly, camera orbits, tiles update on mutation

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

### Near-term mutations

| Action | Terrain effect | GPU update |
|--------|---------------|------------|
| Dig ditch | Lower tile height by 1-2m, material → dirt | Write height + material for affected tiles |
| Build earthen wall | Raise tile height by 1-3m, material → packed_earth | Write height + material for affected tiles |
| Place structure | Set structure flag, spawn structure entity | Write flags for tile |
| Road construction | Set has_road flag | Write flags for tile |

### Far-term mutations

| Action | Terrain effect | GPU update |
|--------|---------------|------------|
| Bomb crater | Lower height in blast radius (falloff), material → rubble, destroy structures | Write height + material for blast area tiles |
| Fire | Material → charred for affected tiles, structures burn | Write material for fire area |
| Flooding | Per-tile water level (separate buffer or extend TileGpuData) | Write water level |
| Erosion | Gradual height changes from water/wind | Write height over time |

All mutations are the same GPU operation: write 16 bytes per affected tile
to the terrain storage buffer. No mesh rebuild. The vertex shader reads
updated values next frame.

For large-area mutations (bomb crater affecting 50 tiles), batch-write
50 × 16 = 800 bytes. Instant.

## Performance budget

| Component | Target | Measurement |
|-----------|--------|-------------|
| Terrain render (100k tiles) | <1ms GPU | Instanced draw, one hex template |
| Entity interpolation compute (10k) | <0.1ms GPU | 40 workgroups × 256 |
| Entity render (10k across LOD tiers) | <0.5ms GPU | 3 instanced draws |
| Body-model render (200 entities) | <0.3ms GPU | 3,200 capsule instances |
| LOD compute (10k entities) | <0.1ms GPU | 40 workgroups |
| Overlay render | <0.2ms GPU | Screen-space quads |
| **Total GPU frame time** | **<2.2ms** | **Target: 60fps = 16.6ms budget** |
| CPU per-tick work | <2ms | Deserialize + buffer upload |
| CPU per-frame work | <0.5ms | Submit compute + render passes |

Massive headroom. The GPU frame time budget is 7x below the 60fps ceiling.

## Verification criteria

- [ ] Hex terrain renders 100k tiles at 60fps with orbit camera
- [ ] Per-tile height/material mutation updates in <1 frame
- [ ] 10k entities render across LOD tiers at 60fps
- [ ] Interpolation compute shader produces smooth 60fps movement from 20 tick/sec input
- [ ] Body-model capsules render for 200 entities at close zoom
- [ ] Camera orbit, pan, zoom with mouse + keyboard
- [ ] SolidJS UI overlays render on top of wgpu canvas
- [ ] SolidJS can read entity state from WASM module
- [ ] Works in Chrome, Firefox, and Safari (WebGPU)
- [ ] Bundle size <3MB gzipped
- [ ] Startup time <2 seconds on broadband

## Open questions

1. **Message format**: JSON or MessagePack for WebSocket tick data? MessagePack
   is faster to deserialize in Rust but less debuggable. Could start JSON,
   switch to msgpack when serialization shows up in profiles.

2. **Shared types between server and viewer**: The server crate and viewer
   crate both need tick/entity types. Extract a `crates/protocol/` crate
   with shared types, compiled for both native and wasm32 targets.

3. **Trunk vs manual build**: Trunk simplifies the WASM build pipeline but
   adds a dependency. Manual cargo + wasm-bindgen + wasm-opt is more
   control. Start with trunk, eject if needed.

4. **Water rendering**: For future flooding/rivers, water needs its own
   render pass with transparency. Deferred to when water mechanics land
   in the engine.
