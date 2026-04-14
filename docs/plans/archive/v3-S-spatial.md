# V3 Domain: S — Spatial Model

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Spatial Model section)
Sequencing: `docs/plans/v3-sequencing.md`

## Purpose

Define the spatial foundation that all other systems build on. Entities live in
continuous 3D space. The hex grid is a derived projection used for spatial
queries and game-mechanical bucketing.

## Design Questions

These are the questions a /design agent should resolve. The spec establishes
the direction; this domain needs the implementation details.

### S.1 Vec3 and Coordinate System

- f32 or f64 for world coordinates? f64 avoids precision issues on large maps
  (200×200 hex = 30km) but doubles memory per entity. What's the right tradeoff
  at 10k entities?
- World origin: center of map or corner? Hex (0,0) maps to what world coordinate?
- Conversion functions: `hex_to_world(Axial) -> Vec3` and `world_to_hex(Vec3) -> Axial`.
  Use cube_round for the reverse. Specify exact formulas for flat-top hex with
  the project's existing even-r offset storage convention.
- How does terrain height integrate? Is terrain height stored per-hex and
  interpolated for continuous positions within a hex? Or per-vertex (hex corners)
  with barycentric interpolation?

### S.2 Hex-as-Projection

- When is hex membership recomputed? Every tick for all entities, or lazily when
  position changes past a threshold?
- Hysteresis implementation: the spec says "only change hex when past center of
  new hex." Define the exact threshold. Is it distance-from-hex-center, or
  fraction-of-hex-crossed? What about entities that move fast enough to skip a
  hex entirely in one tick?
- Should hex membership be a field on Entity (cached, updated per tick) or purely
  derived (computed on every query)? Caching saves repeated pixel_to_hex calls
  but requires bookkeeping.

### S.3 Spatial Index

- Data structure: `HashMap<Axial, SmallVec<[EntityKey; N]>>` or a flat array
  indexed by hex offset? The hex grid has known dimensions, so a flat array is
  O(1) with no hashing overhead.
- What's the right SmallVec size? Most hexes have 0-3 entities. Inline 4?
- Ring queries: "all entities within N hex rings." Pre-generate ring offsets for
  each N (the project already has hex neighbor math). Return an iterator.
- How do projectiles interact with the spatial index? They move fast and may
  cross multiple hexes per tick. Index them in their current hex only, or in
  all hexes along their trajectory?

### S.4 Collision

- Separation force formula: `force = max_force * (1.0 - distance / (radius_a + radius_b))`
  when distance < sum of radii? Or use a smoother falloff?
- Terrain boundary: check destination hex passability before or after velocity
  integration? Before = entities never enter impassable hexes. After = clamp
  position back to boundary (allows sliding along walls).
- Wall segments on hex edges: how are these stored and queried? A bitfield per
  hex (6 edges)? How does an entity moving through continuous space detect it's
  crossing a walled edge?
- Iteration order for collision resolution: does it matter? With soft separation
  forces, order shouldn't affect convergence much, but verify.

### S.5 Z-Axis Infrastructure

- In V3.0, all surface entities have z = terrain_height_at(pos.xy). Projectiles
  have z > terrain during flight. What data structures need the z component now
  vs can be added later?
- Layer enum: `Underground(u8) | Surface | Air(u8)`. Is this derived from z
  relative to terrain height, or stored? Derivation seems cleaner.
- Spatial index: does the hex HashMap need a layer key now, or is that a V3.3
  addition? If we add it now (unused), does it cost anything?

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| S1 | 0 | — | Vec3 type, hex_to_world/world_to_hex, spatial index, hex membership |
| S2 | 0 | — | Collision system (separation + terrain + wall edge detection) |

## Key Files (Expected)

- `crates/engine/src/v3/spatial.rs` — Vec3, coordinate conversions, spatial index
- `crates/engine/src/v3/collision.rs` — separation, terrain boundary, wall edges
- `crates/engine/src/v3/hex.rs` — existing hex math, extended for world conversion

## Constraints

- Must be compatible with the existing hex.rs axial coordinate system.
- Spatial index must support 10k entities at 10 ticks/sec without allocation in
  the query hot path.
- All conversion functions must be deterministic (same input → same output) for
  replay correctness.
