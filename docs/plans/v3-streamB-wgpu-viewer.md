# Stream B: wgpu WASM Viewer

Status: **B1-B3 done, B4 next (depends on A1 body model)**
Depends on: Phase 0 (protocol crate)
Design spec: `docs/plans/v3-wgpu-renderer.md`
Linear: reference IA issue if one exists

## Goal

Replace the PixiJS 2D renderer with a Rust wgpu renderer compiled to
WASM, targeting browser WebGPU. GPU compute shaders handle entity
interpolation, LOD culling, and body-point transforms. Continuous
heightmap terrain with natural shading. Camera rotation for 3D viewing.

## Waves

### B1: wgpu scaffold + heightmap terrain + camera

**New crate:** `crates/viewer/`

**Files created:**
- `crates/viewer/Cargo.toml` — dependencies: wgpu, winit, glam, web-sys,
  wasm-bindgen, simulate-everything-protocol
- `crates/viewer/Trunk.toml` — Trunk build config
- `crates/viewer/index.html` — Trunk HTML template (canvas + SolidJS mount point)
- `src/lib.rs` — wasm_bindgen entry, winit event loop, WS connection,
  tick ingestion, frame loop
- `src/gpu.rs` — wgpu device/surface/adapter setup, pipeline registry
- `src/heightmap.rs` — heightmap texture upload, clipmap mesh generation
  (4 LOD rings), terrain render pass
- `src/camera.rs` — orbit camera struct (target, distance, azimuth,
  elevation), view/projection matrices via glam, smooth interpolation
- `src/input.rs` — winit event → camera action mapping (scroll=zoom,
  middle-drag=orbit, right-drag=pan, WASD=pan, Q/E=rotate)
- `src/shaders/terrain.wgsl` — vertex: bicubic heightmap displacement.
  Fragment: per-pixel normals from heightmap gradient, slope-based material
  blending, detail textures, triplanar mapping on steep faces, atmospheric
  fog, directional sun lighting

**Terrain rendering approach:**
- Heightmap stored as R32Float GPU texture (1m/sample)
- Material stored as R8Uint GPU texture (same resolution)
- Geometry clipmap: 4 LOD rings (1m/4m/16m/64m) centered on camera
- ~212k triangles total across all rings
- Vertex shader samples heightmap for displacement
- Fragment shader computes normals, blends materials by slope + noise,
  applies detail textures and lighting

**SolidJS integration:**
- wgpu canvas element at z-index 0
- SolidJS div overlay at z-index 1 with `pointer-events: none`
- Interactive UI elements get `pointer-events: auto`
- Communication via wasm_bindgen exported functions

**Status:** Done. Compiles to WASM, Trunk builds (569KB gzip), loads in
browser. Headless can't verify WebGPU rendering — needs manual browser test.

**Gate:** heightmap terrain renders with natural shading, camera orbits
smoothly, `trunk serve` works in Chrome/Firefox

### B2: Hex overlay + terrain mutation + chunk dirtying

**Files created:**
- `src/hex_overlay.rs` — hex border rendering (line segments draped on
  terrain), territory fill (alpha-blended hex polygons), fog-of-war
- `src/shaders/hex_overlay.wgsl` — vertex: project hex vertices onto
  terrain (sample heightmap at hex corner positions). Fragment: territory
  color with alpha.

**Files modified:**
- `src/heightmap.rs` — add chunk dirty tracking, hex→chunk mapping,
  partial texture re-upload via `queue.write_texture()` sub-region
- `src/lib.rs` — handle terrain mutation messages from server

**Hex-scoped chunk dirtying:**
- Heightmap divided into rectangular chunks (64×64 texels)
- Precomputed mapping: medium hex → overlapping chunks
- Mutation in a hex → dirty only overlapping chunks
- Upload only dirty chunk sub-regions to GPU

**Gate:** hex overlay renders on terrain surface, territory colors visible,
terrain mutation (height change) reflects in <1 frame, only affected
chunks re-uploaded

### B3: Entity rendering + interpolation compute + LOD

**Files created:**
- `src/entities.rs` — entity instance buffer, tick state double-buffer
  (prev/curr), compute shader dispatch, LOD-specific draw calls
- `src/lod.rs` — LOD compute shader dispatch (camera → screen-space hex
  size → tier assignment per entity), indirect draw count generation
- `src/shaders/entity.wgsl` — vertex: instance transform from storage
  buffer. Fragment: player color, facing indicator. Three variants for
  close/mid/far LOD.
- `src/shaders/interpolate.wgsl` — compute: lerp prev→curr entity
  positions at render framerate. Dispatched every frame with
  `t = (frame_time - tick_time) / tick_interval`.
- `src/shaders/lod.wgsl` — compute: per-entity screen-space hex size →
  LOD tier assignment. Writes indirect draw counts.

**Entity GPU data layout:**
```rust
#[repr(C)]
struct EntityGpuData {
    pos: [f32; 3], facing: f32,
    owner: u32, lod_tier: u32, entity_kind: u32,
    health_frac: f32, stamina_frac: f32, flags: u32,
    _pad: [f32; 2],
}  // 48 bytes per entity
```

**Rendering by LOD tier:**
- Close (hex >= 60 screen px): colored shape + facing indicator (placeholder
  until B4 adds body model)
- Mid (hex >= 20 screen px): colored dot + directional tick mark
- Far (hex < 20 screen px): density heatmap (compute shader aggregates
  per-hex entity count, renders as hex fill alpha)

**Gate:** 10k entities render at 60fps across all LOD tiers,
interpolation produces smooth movement from 20 tick/sec input, LOD
transitions are seamless during zoom

### B4: Body-model rendering + capsule instancing

**Depends on:** A1 (body model struct + protocol body_points field)

**Files created:**
- `src/body_model.rs` — body-point storage buffer, capsule mesh template,
  instanced draw for limb segments, wound tinting, weapon/shield rendering
- `src/shaders/body.wgsl` — vertex: capsule geometry from two endpoint
  positions + radius in storage buffer. Fragment: player-colored body
  parts with wound tint (blend toward crimson by damage factor per zone).
- `src/shaders/body_interpolate.wgsl` — compute: lerp body points between
  ticks (same pattern as entity interpolation)

**Body-model rendering:**
- 16 body points × 200 close-zoom entities = 3,200 capsule instances
- Single instanced draw call with per-instance transforms from storage buffer
- Capsule mesh: hemisphere + cylinder + hemisphere (pre-built template)
- Wound tinting: per-zone damage factor → color blend in fragment shader
- Weapon: elongated capsule from hand to sword tip, phase-dependent glow
- Shield: disc mesh at off-hand position, normal-oriented

**Gate:** 200 body-model entities render at 60fps with visible combat
poses, wound tinting, weapon/shield

### B5: Overlays + text + polish

**Files created:**
- `src/overlay.rs` — screen-space quad rendering for health bars, stamina
  bars, selection highlights, damage number popups
- `src/text.rs` — SDF glyph atlas loader, text quad generation, distance
  field rendering
- `src/shaders/overlay.wgsl` — vertex: screen-space positioning from
  projected world position. Fragment: colored quad with alpha.
- `src/shaders/text.wgsl` — vertex: glyph quad positioning.
  Fragment: SDF distance field sampling for crisp text at any zoom.
- `assets/font_atlas.png` — pre-baked SDF font atlas (generated at build time)

**Gate:** full feature parity with current PixiJS renderer — health bars,
stamina bars, entity labels, selection highlighting all work

### B6: Delete PixiJS + V1/V2 code

**Frontend removal:**
- `pixi.js` from `package.json`
- All PixiJS rendering: `src/v3/render/*.ts`, `src/v3/HexCanvas.tsx`,
  `src/v3/entityMap.ts`, `src/v3/applySnapshotDelta.ts`
- V1 code: `src/types.ts`, `src/Board.tsx`, `src/App.tsx`, `src/LiveApp.tsx`,
  `src/ScoreboardApp.tsx`, `src/styles/board.css.ts`
- V2 code: `src/v2types.ts`, `src/HexBoard.tsx`, `src/HexCanvas.tsx`,
  `src/V2SimApp.tsx`, `src/V2App.tsx`
- Shared V1/V2: `src/Nav.tsx`

**Backend removal:**
- V1 routes and WS handlers in `crates/web/src/main.rs`
- V1 round-robin: `crates/web/src/roundrobin.rs`
- V2 routes, WS handlers, round-robin: `crates/web/src/v2_roundrobin.rs`,
  `crates/web/src/v2_rr_review.rs`, V2 protocol types
- V1 engine: `crates/engine/src/game.rs`, `crates/engine/src/agent.rs`,
  `crates/engine/src/expander_agent.rs`, `crates/engine/src/swarm_agent.rs`,
  `crates/engine/src/pressure_agent.rs`, `crates/engine/src/subprocess_agent.rs`
- V2 engine: `crates/engine/src/v2/` directory
- V1/V2 CLI modes in `crates/cli/`
- Update `docs/architecture.md` — remove V1/V2 sections

**Gate:** clean build with no PixiJS references, no V1/V2 code, all
viewer features work via wgpu. `cargo check --workspace` and
`bun run build` both pass.

## Verification criteria (full stream)

- [ ] Heightmap terrain renders with natural shading at 60fps
- [ ] Camera orbit/pan/zoom with mouse + keyboard
- [ ] Terrain mutation (ditch/wall/crater) updates in <1 frame
- [ ] Only affected chunks re-uploaded on mutation
- [ ] Hex overlay drapes correctly onto terrain surface
- [ ] 10k entities render across LOD tiers at 60fps
- [ ] Interpolation compute produces smooth 60fps from 20 tick/sec
- [ ] Body-model capsules render for 200 entities at close zoom
- [ ] Wound tinting, weapon, shield visible on body models
- [ ] Health/stamina bars, text labels, selection highlighting work
- [ ] SolidJS UI overlays render on top of wgpu canvas
- [ ] Works in Chrome and Firefox (WebGPU)
- [ ] Bundle size <3MB gzipped
- [ ] No PixiJS or V1/V2 code remains after B6
