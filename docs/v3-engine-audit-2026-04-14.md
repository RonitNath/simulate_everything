# V3 Engine Audit

Date: 2026-04-14 (updated end-of-day after Stream B/E/F merges)

Scope: full V3 engine state including Streams A-F.

Primary references:
- `docs/specs/v3-*`
- `docs/plans/v3-*`
- `crates/engine/src/v3/**`
- `crates/viewer/src/**`
- `crates/cli/src/v3bench.rs`

Companion docs:
- [V3 Capability Matrix](./v3-capability-matrix.md)
- [V3 Sequencing](./plans/v3-sequencing.md)

## Summary

V3 core is functionally landed. Six parallel streams (Phase 0 + A-F) have
merged to main. The engine has: entity unification with continuous 3D space,
material-interaction combat with Verlet body model, multi-resolution spatial
index, terrain operation log, autonomous agent behavior (needs/HTN/action
queues), and compositional world model (physical properties replacing typed
components). The wgpu WASM viewer renders terrain, entities, and body models.

The main remaining work is integration testing and validation (E7), movement
wiring into the action queue, terrain perception, and starting condition
generation.

## What is solid

- Entity model: composable components, containment, physical properties,
  tool properties, matter stacks, site properties
- Damage pipeline: 7-step impact resolution, wound/vitals/bleed
- Melee combat: attack progression, cooldown, body model hit detection
- Spatial index: fine (10m), hex (150m), coarse (500m), hex mapping with hysteresis
- Terrain ops: analytic operation log, compaction, rasterization cache,
  viewer streaming
- Behavior runtime: needs decay, utility scoring, HTN decomposition, action
  queue execution (tick-by-tick and batch), resolution demand, social state
- Compositional economy: affordance-based production, tag-based lookups,
  CommodityKind stockpiles
- Viewer: wgpu terrain with clipmap LOD, entity rendering with compute
  interpolation, body model rendering, hex overlay, standalone viewer shell
- Protocol: terrain raster init + patch deltas, needs/goal/action exposure,
  physical/tool/matter/site fields

## What remains

### Integration work
- **Movement/pathfinding into action queue**: MoveTo action should set waypoints
  and let steering execute. Currently partially wired — steering works, but
  the action queue → waypoint handoff needs testing.
- **Terrain perception**: StrategicView needs infrastructure/damage/opportunity
  assessment from terrain op state.
- **Starting conditions**: Mapgen should produce pre-established settlements
  (worn tools, plowed fields, roads, social relationships).
- **Combat learning loop**: Damage tables and observations exist but the
  feedback loop (observation → table update → tactical adaptation) isn't
  end-to-end wired.

### Validation (E7 — in progress)
- Behavior validation bench harness (statistical + forensic modes)
- Headless 2D renderer for frame-by-frame analysis without browser
- Arena scenarios: individual behavior, 1v1 combat, small groups, settlement
  stability, terrain exploitation
- TOML scenario configs with invariant checking
- Tick-level entity state snapshots with full decision context

### Visual verification
- WebGPU headless limitation: rendering pipeline verified but screenshots blank
  in headless Chromium. Headed browser session needed for visual confirmation.

## Recommended Next Order

1. **E7 validation infrastructure** (in progress) — forensic bench, headless
   renderer, arena scenarios. This is the acceptance test for whether
   autonomous behavior works.
2. **Movement → action queue wiring** — test MoveTo action drives steering
   correctly, entities arrive at destinations, patrol routes work.
3. **Terrain perception** — add infrastructure/damage assessment to
   StrategicView so strategy can reason about terrain state.
4. **Combat learning loop** — wire observations back to damage table, verify
   tactical adaptation.
5. **Starting conditions** — mapgen enhancement to produce pre-established
   settlements.
6. **Spec updates** — update v3-A-agents.md, entity-unification spec, and
   capability matrix to reflect E/F architecture.
