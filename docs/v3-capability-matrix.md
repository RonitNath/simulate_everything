# V3 Capability Matrix

Updated: 2026-04-14 (end-of-day, post Stream B/E/F merges)

Legend:
- `engine-live` — shared engine/runtime path actively uses it
- `shared-engine-unused` — implemented in shared engine, but not yet wired into the live tick/RR path
- `module-only` — primitives/modules/tests exist, but there is no shared execution path using them
- `placeholder` — type/surface exists, but the values are scaffolded or synthetic
- `not-landed` — still missing

## Original V3 Waves (A1-R5)

| Item | Status | Notes |
|------|--------|-------|
| S1 | `engine-live` | Spatial types, hex projection, index. |
| S2 | `engine-live` | Collision and spatial primitives. |
| D1 | `engine-live` | Body zones, wounds, bleed, vitals. |
| W1 | `engine-live` | Weapon/armor/material property model. |
| R1 | `engine-live` | Standalone wgpu viewer replacing PixiJS (B complete). |
| M1 | `shared-engine-unused` | Core steering primitives; live tick uses a subset. |
| D2 | `engine-live` | Impact resolution pipeline. |
| W2 | `engine-live` | Melee attack state and resolution. |
| A1 | `engine-live` | Layer types, agent output, cadence dispatch. |
| R2 | `engine-live` | Entity rendering with compute interpolation + LOD (B3). |
| M2 | `module-only` | Pathfinding/smoothing/formation-slot modules exist, not in live routing. |
| W3 | `shared-engine-unused` | Projectile entities and arc physics; full path partial. |
| A2 | `engine-live` | Operations layer — now injects methods, not task assignments (E). |
| E1 | `engine-live` | Entity model, containment, mapgen. |
| P1 | `engine-live` | Wire types, snapshot, territory/player/task surfaces. |
| A3 | `engine-live` | Tactical reasoning via resolution demand (E6). |
| A4 | `engine-live` | Strategy personalities — now adjust need weights (E). |
| A5 | `shared-engine-unused` | Damage tables and observations; learning loop partial. |
| E2 | `engine-live` | Engine-owned agent phase, shared economy. |
| P2 | `engine-live` | RR/replay surface. |
| R3 | `engine-live` | Projectile/wound/equipment presentation. |
| R4 | `engine-live` | Viewport culling. |
| P3 | `placeholder` | Live status; roads and deeper economy scaffolded. |
| P4 | `placeholder` | Review/capture surface. |
| R5 | `not-landed` | Deferred from V3.0 scope. |

## Concurrent Streams

| Stream | Status | Notes |
|--------|--------|-------|
| Phase 0 (protocol crate) | `engine-live` | Shared wire types + msgpack. |
| A (Verlet body) | `engine-live` | 16-point skeletal model, constraint solver, kinetic chain. |
| B (wgpu viewer) | `engine-live` | Terrain clipmap, entity rendering, body model, hex overlay, standalone shell, terrain raster streaming. |
| C (spatial index) | `engine-live` | Fine (10m), coarse (500m), hex mapping with hysteresis. |
| D (terrain ops) | `engine-live` | Analytic op log, compaction, rasterization cache, viewer streaming. |
| E (agent behavior) | `engine-live` | Needs, utility scorer, HTN engine, action queues, resolution demand, social state. E7 (validation) in progress. |
| F (compositional world) | `engine-live` | Physical properties, tool properties, matter stacks, site properties, affordance queries, tag-based economy. |

## Protocol / Frontend

| Surface | Status | Notes |
|---------|--------|-------|
| Entity needs/goal/action | `engine-live` | Exposed via SpectatorEntityInfo (E). |
| Physical/tool/matter/site | `engine-live` | Exposed via SpectatorEntityInfo (F). |
| Terrain raster init | `engine-live` | Full-resolution height/material via V3Init (D+B). |
| Terrain patch deltas | `engine-live` | Per-tick dirty patches via V3SnapshotDelta (D+B). |
| Body model wire | `engine-live` | BodyRenderInfo in deltas (A+B). |
| Inspector | `engine-live` | Shows needs, goal, action, physical properties. |

Primary references:
- [docs/plans/v3-sequencing.md](./plans/v3-sequencing.md)
- [docs/v3-engine-audit-2026-04-14.md](./v3-engine-audit-2026-04-14.md)
