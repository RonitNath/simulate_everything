# Spec: V3 Domain S — Spatial Model

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2)
Sequencing: `docs/plans/v3-sequencing.md`

## Vision

The spatial foundation for V3. Entities live in continuous 3D space; the hex grid
is a derived projection for spatial queries and game-mechanical bucketing. The
ground is a mutable vertex heightfield with geological material. All obstacles —
walls, trees, buildings, terrain modifications — are entities with continuous
geometry. Collision, movement cost, and line-of-sight are computed in continuous
space against entity geometry and the heightfield.

## Use Cases

1. **Entity placement and lookup.** Any system can query "what entities are near
   position X" or "what entities are in hex H" via the spatial index. The index
   is incrementally maintained as entities move.

2. **Movement cost sampling.** The movement system (M domain) queries
   `effective_height_at(pos)` and `material_at(pos)` to compute slope and
   friction. These queries hit the vertex heightfield and are called per-entity
   per-tick — they must be fast.

3. **Projectile trajectory resolution.** A projectile in flight queries
   `query_arc(origin, velocity, gravity)` and receives all intersections along
   the parabolic path — entities, walls, and ground. The damage system walks the
   hit list, applying damage and reducing projectile energy until it stops.

4. **Line-of-sight.** Vision and targeting query `query_ray(origin, direction,
   max_dist)` against entities and the heightfield to determine what blocks
   sight.

5. **Terrain modification.** Worker entities dig and pile by calling
   `modify_vertex(id, delta_height)`. Lowering vertices produces fill material
   (resource); raising vertices consumes it. The heightfield mutates in place.
   Dig speed depends on vertex material (soil fast, rock slow).

6. **Spatial proximity.** Combat, steering, and AI query `query_radius(pos,
   distance)` and `query_ring(hex, n)` for nearby entities.

## Architecture

### Vec3 and Coordinate System

```
World space: Vec3 { x: f32, y: f32, z: f32 }
  Origin: center of hex (0,0) at world (0.0, 0.0, 0.0)
  x: increases east
  y: increases north
  z: altitude (surface entities: z = terrain_height_at(pos.xy))

Hex size: 150 meters flat-to-flat
  hex_radius (center to corner) = 150 / sqrt(3) ≈ 86.6m
  hex_to_world(ax: Axial) -> Vec3:
    x = sqrt(3) * hex_radius * (ax.q + ax.r / 2)
    y = 1.5 * hex_radius * ax.r
    z = terrain_height_at(x, y)
  world_to_hex(pos: Vec3) -> Axial:
    cube_round(pixel_to_cube(pos.x, pos.y))  // existing cube_round algorithm
```

f32 precision: at 30km (200×200 hex map), f32 gives ~1mm precision. Sufficient.
Cross-platform determinism retrofittable later via deterministic trig functions
(Factorio approach — custom sin/cos/atan, no architectural changes). Same-binary
determinism for V3.0.

### Vertex Heightfield

The ground is a flat array of vertices, offset-indexed.

```
struct Vertex {
    height: f32,          // meters above datum
    material: GeoMaterial, // Soil, Rock, Clay, Sand
}

vertices: Vec<Vertex>    // flat array, offset-indexed
vertex_at(col, row) -> &Vertex
vertex_at_mut(col, row) -> &mut Vertex
```

Vertex count ≈ 2 × hex count (plus boundary). Each hex corner maps to a vertex
via pure math — no stored adjacency.

**Queries:**
- `effective_height_at(pos: Vec2) -> f32` — barycentric interpolation of 3
  nearest vertices.
- `material_at(vertex_id) -> GeoMaterial` — direct lookup.
- `slope_at(pos: Vec2, direction: Vec2) -> f32` — directional gradient from
  vertex heights.

**Mutation:**
- `modify_vertex(id: VertexId, delta: f32)` — raises or lowers a vertex.
  Dig/pile mechanics (worker assignment, material conservation, tool quality
  scaling) belong to the E domain. S provides the mutation primitive only.

**Replay:** Delta layer. Base heightfield is reproducible from mapgen seed.
Only modified vertices are stored: sparse map of `VertexId -> accumulated_delta`.
Current state = base + deltas.

### Hex Projection

Hex membership is cached on each entity as `hex: Axial`. Updated per tick with
hysteresis: an entity only changes hex when its position is past the center of
the new hex (distance to new center < 0.4 × hex_radius). Prevents oscillation
at boundaries.

Fast-moving entities (projectiles) that skip hexes entirely are handled by
trajectory queries (query_ray, query_arc), not hex membership.

### Spatial Index

Flat array indexed by hex offset coordinates. Matches V2 pattern.

```
struct SpatialIndex {
    width: usize,
    height: usize,
    cells: Vec<SmallVec<[EntityKey; 4]>>,
}
```

**Update strategy:** Incremental. Entity moves → remove from old cell, insert
into new cell. Full rebuild available as fallback (debug mode, initialization).

**Query API:**

| Method | Returns | Use case |
|--------|---------|----------|
| `query_radius(pos, dist)` | Iterator<EntityKey> | Steering, melee, proximity |
| `query_ray(origin, dir, max_dist)` | Vec<Hit> | LOS, flat trajectory projectiles |
| `query_arc(origin, velocity, gravity)` | Vec<Hit> | Parabolic projectile trajectories |
| `query_ring(hex, n)` | Iterator<EntityKey> | AI scanning, area effects |
| `entities_at(hex)` | &[EntityKey] | Direct hex lookup |

All queries: hex culling first (spatial index), then precise geometry tests
against candidate entities. This optimization lives inside the query methods —
callers don't reimplement it.

`query_ray` and `query_arc` return **all** intersections ordered by distance
along the path. Callers (damage system) walk the list and decide when the
projectile stops.

### Entity Geometry

Four collision primitives:

| Primitive | Use case | Data |
|-----------|----------|------|
| **Circle** | People, animals, trees, round structures | center + radius |
| **LineSegment** | Walls, fences, palisades | start + end + thickness |
| **OrientedRect** | Buildings, wagons | center + half_extents + rotation |
| **Triangle** | Bastion fortifications, star forts | 3 vertices |

Each entity with physical presence carries a geometry variant. Entities without
geometry (resources contained in another entity) have `None`.

Pairwise intersection tests for all combinations (circle-circle, circle-segment,
circle-rect, circle-triangle, segment-segment, segment-rect, segment-triangle,
rect-rect, rect-triangle, triangle-triangle) plus ray/arc intersection for each
primitive.

### Collision System

Three collision types:

**Entity-entity (mobile).** Soft separation via steering force. When two mobile
entities overlap (distance < sum of radii or geometry intersection):
```
separation_force = max_force * (1.0 - penetration_depth / max_penetration)
```
Direction: away from the other entity's center. Entities compress under pressure
(bottleneck, retreat) but don't phase through each other.

**Entity-entity (terrain/structure).** Hard collision against walls (LineSegment),
trees/boulders (Circle), buildings (OrientedRect), and fortifications (Triangle).
These are entities with geometry in the spatial index — the same query and
intersection code handles them. A person walking into a wall gets a hard
deflection (slide along the wall surface), not soft separation. Breaching or
scaling requires specific actions, not movement through.

**Entity-ground.** Entity z is clamped to `effective_height_at(pos.xy)` for
surface entities. Vertical velocity from falling or jumping (future) resolves
against the heightfield.

**Terrain does not create hard impassable boundaries.** Steep slopes have high
movement cost (via the M domain's speed modifiers), but nothing is binary
blocked. Even cliffs can be scaled — slowly, expensively, with risk. Walls and
structures are hard obstacles that block movement — but they can be breached,
scaled, or destroyed.

### Movement Cost Model

Movement cost is derived, not stored. Three components:

1. **Slope** — directional gradient between vertex heights at the entity's
   position. Steeper = slower. Computed from `slope_at(pos, movement_direction)`.
2. **Base friction** — from geological material of the nearest vertices.
   Rock > clay > soil > sand (for walking). Derived from vertex material.
3. **Entity modifiers** — roads (reduce cost), mud/crops/rubble (increase cost).
   These are entities in the spatial index; the movement system queries nearby
   surface entities for modifiers.

The M domain owns the formula. S provides the query primitives.

### Z-Axis Infrastructure

Vec3 carries z from day one. All surface entities have
`z = effective_height_at(pos.xy)`. Projectiles in flight have z > surface
(parabolic arc). The z component participates in all geometry tests.

**Layer enum (derived, not stored):**
```
enum Layer {
    Underground(u8),  // future: depth tiers
    Surface,
    Air(u8),          // future: altitude tiers
}

fn layer_of(entity_z: f32, terrain_z: f32) -> Layer
```

The spatial index remains 2D (hex only) in V3.0. Layer keying deferred to V3.3
when underground/air entities exist.

## Security

Not applicable — single-player simulation engine, no network input in the
spatial module. Wire protocol (P domain) handles serialization boundaries.

## Privacy

No PII handled.

## Audit

Vertex modifications are recorded in the delta layer (vertex_id, delta, tick).
Entity position changes are captured by the replay system (P domain). No
additional audit trail needed in S.

## Convention References

- `crates/engine/src/v2/hex.rs` — existing axial coordinate system, neighbor
  math, distance, ring queries. V3 extends this; does not replace it.
- `crates/engine/src/v2/spatial.rs` — V2 spatial index pattern (flat array,
  SmallVec<4>). V3 follows the same pattern with incremental updates.
- `crates/engine/src/v2/state.rs` — V2 Cell struct with height, moisture,
  biome. V3 replaces per-hex height with per-vertex heightfield.

## Scope

### V3.0 (ship this)

- Vec3 type, hex_to_world / world_to_hex conversions
- Vertex heightfield (flat array, height + GeoMaterial, barycentric interpolation)
- Vertex mutation API (modify_vertex) + delta layer for replay
- Spatial index (flat array, SmallVec<4>, incremental update, full-rebuild fallback)
- Hex projection with hysteresis (cached on entity, per-tick update)
- Collision system (4 geometry primitives, entity-entity separation, entity-ground)
- Query API (query_radius, query_ray, query_arc, query_ring)
- Z carried in Vec3, surface entities only
- Layer enum (derived from z vs terrain height)
- World origin at hex (0,0) center, x east, y north

### Deferred

- **Tunnels / underground layer** — deferred to V3.3. Requires spatial index
  layer keying and sub-surface pathfinding.
- **Spatial index layer keying** — (hex, layer) composite key. Not needed until
  underground/air entities exist.
- **Cross-platform deterministic trig** — retrofittable via custom sin/cos/atan
  functions. No architectural changes needed. Ship when cross-platform replay
  matters.
- **Sub-hex heightfield grids** — if per-vertex proves too coarse for specific
  terrain features, add 4×4 sample grid per hex. Unlikely to be needed.
- **Flow field sampling** — M domain concern, but requires barycentric
  interpolation of hex-center flow vectors at continuous positions.

## Verification

- [ ] `hex_to_world(world_to_hex(pos))` round-trips to within 0.5 × hex_radius
  of the original position (hex quantization is lossy by design)
- [ ] `world_to_hex(hex_to_world(ax))` round-trips exactly for all valid hex
  coordinates
- [ ] `effective_height_at` interpolates smoothly between vertex heights (no
  discontinuities at hex boundaries)
- [ ] Spatial index incremental update produces identical state to full rebuild
  (fuzz test: random entity movements, compare index state)
- [ ] `query_arc` returns all intersections in distance order — test with
  projectile passing through 2 wall segments before hitting ground
- [ ] `query_ray` correctly occludes behind heightfield terrain (ray from valley
  cannot see past ridge)
- [ ] Hysteresis prevents hex oscillation: entity at boundary does not flip
  hexes when stationary
- [ ] Vertex mutation + delta layer: modify vertices, save deltas, reconstruct
  from base + deltas, compare to mutated state
- [ ] 10k entities, 10 tps: spatial index queries complete within tick budget
  (100ms) with no allocation in the query hot path
- [ ] All 4 geometry primitives: pairwise intersection tests correct (unit tests
  for each of the 10 combinations)
- [ ] Replay determinism: same seed + same inputs → identical vertex state and
  entity positions after 1000 ticks (same binary)

## Deploy Strategy

Engine-only domain. No deployment — consumed by other V3 domains (M, D, W, E)
and eventually by the sim tick loop (E2). Verified via `cargo test` and the
benchmark harness.

## Files

| File | Contents |
|------|----------|
| `crates/engine/src/v3/spatial.rs` | Vec3, coordinate conversions, vertex heightfield, effective_height_at, slope_at |
| `crates/engine/src/v3/index.rs` | SpatialIndex (flat array, incremental update, full rebuild fallback) |
| `crates/engine/src/v3/collision.rs` | Geometry primitives (Circle, LineSegment, OrientedRect, Triangle), pairwise tests, separation forces |
| `crates/engine/src/v3/query.rs` | query_radius, query_ray, query_arc, query_ring — hex culling + precise geometry |
| `crates/engine/src/v3/mod.rs` | Module exports |
| `crates/engine/src/v3/hex.rs` | Extended hex math: hex_to_world, world_to_hex, vertex index math |
