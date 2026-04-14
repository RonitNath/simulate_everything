# Stream C: Multi-Resolution Hex Spatial Index

Status: **ready for implementation**
Depends on: nothing (can start immediately, parallel with A and B)
Design spec: `docs/plans/v3-wgpu-renderer.md` (spatial index section)
Linear: reference IA issue if one exists

## Goal

Add fine (~10m) and coarse (~500m) hex spatial indices alongside the
existing medium (~150m) hex grid. Different engine systems query at the
resolution that fits their scale. Combat queries drop from O(entities
per 150m hex) to O(entities per 10m hex). Strategic AI gets O(1)
aggregate lookups instead of iterating all entities.

## Current state

Single-resolution spatial index at ~150m hex radius
(`crates/engine/src/v3/index.rs`). `query_radius()` finds the hexes
overlapping a radius, iterates all entities in those hexes, fine-filters
by distance. Works, but 150m hexes are far too coarse for 5m combat
queries — a single hex may contain hundreds of entities.

## Architecture

Three independent flat hex grids. NOT H3-style nested hierarchy
(aperture-7 rotation breaks LOD/chunk alignment). Each grid is a
separate data structure optimized for its consumers.

| Level | Hex radius | Cells (3km map) | Storage | Updated |
|-------|-----------|-----------------|---------|---------|
| Fine | ~10m | ~90k | `HashMap<Axial, SmallVec<[EntityKey; 4]>>` | Every tick for moving entities |
| Medium | ~150m (existing) | ~400 | Flat array (current `SpatialIndex`) | On hex change (hysteresis) |
| Coarse | ~500m | ~36 | Flat array + `CoarseHexAggregate` per cell | On medium hex change |

### Why hexes at all levels (not squares for fine/coarse)

- Uniform 6-neighbor distance (square grids have diagonal √2× problem)
- Better circle approximation for range queries
- Consistent coordinate system across all engine systems

## Waves

### C1: Fine hex grid for combat queries

**Files created:**
- `crates/engine/src/v3/fine_index.rs` — `FineIndex` struct, insert/remove/
  move, `query_radius_fine()`, hex coordinate math at ~10m resolution

**Files modified:**
- `crates/engine/src/v3/sim.rs` — Update fine index for all moving entities
  each tick (in movement phase, after position integration)
- `crates/engine/src/v3/mod.rs` — Declare module
- `crates/engine/src/v3/state.rs` — Add `FineIndex` to `GameState`

**Fine index design:**
```rust
struct FineIndex {
    cell_size: f32,  // ~10m
    cells: HashMap<(i32, i32), SmallVec<[EntityKey; 4]>>,
}

impl FineIndex {
    fn cell_for_pos(&self, pos: Vec3) -> (i32, i32);
    fn insert(&mut self, pos: Vec3, entity: EntityKey);
    fn remove(&mut self, pos: Vec3, entity: EntityKey);
    fn move_entity(&mut self, old_pos: Vec3, new_pos: Vec3, entity: EntityKey);
    fn query_radius(&self, center: Vec3, radius: f32) -> impl Iterator<Item = EntityKey>;
}
```

**Integration with combat:**
- `weapon.rs` `resolve_melee()` currently does `edge_distance_2d` check
  against all entities in 150m hex ring. Replace with `fine_index.query_radius()`
  at weapon reach (~1.5-5m). This should scan ~0-5 entities instead of
  ~50-200.
- `collision.rs` narrow-phase similarly benefits

**Hex addressing at fine resolution:**
- Use axial coordinates scaled by cell_size: `q = floor(x / cell_size)`,
  `r` computed from hex geometry at the fine scale
- OR: simple spatial hash `(floor(x/10), floor(y/10))` with hex-shaped
  cells. The exact shape matters less than the cell size for performance.

**Tests:**
- Insert 1000 entities, query radius 5m → returns only entities within 5m
- Move entity across cell boundary → found in new cell, absent from old
- query_radius with 0 matches returns empty
- Performance: 10k entities, 1k queries at 5m radius < 1ms total

### C2: Coarse hex grid with aggregates

**Files created:**
- `crates/engine/src/v3/coarse_index.rs` — `CoarseIndex` struct with
  `CoarseHexAggregate` per cell, update on medium hex change

**Files modified:**
- `crates/engine/src/v3/state.rs` — Add `CoarseIndex` to `GameState`
- `crates/engine/src/v3/sim.rs` — Update coarse index when medium hex
  membership changes
- `crates/engine/src/v3/perception.rs` — Strategic perception reads coarse
  aggregates instead of iterating all entities
- `crates/engine/src/v3/strategy.rs` — Strategic AI uses coarse aggregates
- `crates/engine/src/v3/mod.rs` — Declare module

**Coarse hex aggregate:**
```rust
struct CoarseHexAggregate {
    /// Entity count per player
    population: [u16; MAX_PLAYERS],
    /// Sum of combat effectiveness per player
    army_strength: [f32; MAX_PLAYERS],
    /// Resource totals in this region
    resources: ResourceTotals,
    /// Terrain summary (dominant material, avg height, road coverage)
    terrain_profile: TerrainProfile,
    /// Entity keys for full iteration when needed
    entities: Vec<EntityKey>,
}
```

**Update flow:**
- When entity changes medium hex, check if coarse hex also changed
- If coarse hex changed: decrement old aggregate, increment new
- Aggregates maintained incrementally — no full recomputation
- Terrain profile recomputed only when terrain mutation occurs in the
  coarse hex region

**Strategic AI integration:**
- `perception.rs` `strategic_perception()` currently returns placeholder
  territory/economy/threat summaries. Replace with coarse aggregate reads.
- `strategy.rs` can query "total army strength in 2km region" as
  sum of coarse hex aggregates in a k-ring, O(k²) instead of O(all entities)

**Tests:**
- Aggregate population counts match actual entity count per coarse hex
- Entity move across coarse boundary updates both old and new aggregates
- Entity death decrements aggregate correctly
- Strategic perception returns accurate territory summary from aggregates
- k-ring aggregate query matches brute-force sum

### C3: Cross-level mapping + hex-scoped terrain chunks

**Files created:**
- `crates/engine/src/v3/hex_mapping.rs` — Precomputed cross-level lookups,
  hex→terrain-chunk mapping

**Files modified:**
- `crates/engine/src/v3/state.rs` — Add `HexMapping` to `GameState`,
  initialize at map creation
- `crates/engine/src/v3/sim.rs` — Wire `on_entity_move()` cascade through
  all three index levels

**Cross-level mappings (precomputed at map init):**
```rust
struct HexMapping {
    /// For each fine hex, which medium hex contains it
    fine_to_medium: HashMap<(i32, i32), Axial>,
    /// For each medium hex, which coarse hex contains it
    medium_to_coarse: Vec<(i32, i32)>,  // indexed by medium hex linear index
    /// For each medium hex, overlapping rectangular terrain chunks
    medium_to_chunks: Vec<SmallVec<[ChunkId; 4]>>,
}
```

**Entity move cascade:**
```rust
fn on_entity_move(state: &mut GameState, entity: EntityKey, old_pos: Vec3, new_pos: Vec3) {
    // Fine: always check (cheap hash table move)
    state.fine_index.move_entity(old_pos, new_pos, entity);

    // Medium: only on hex change (existing hysteresis)
    let old_med = state.entity_hex(entity);
    let new_med = state.update_hex_membership(entity, new_pos);
    if new_med != old_med {
        state.medium_index.move_entity(old_med, new_med, entity);

        // Coarse: only on coarse hex change (rare)
        let old_coarse = state.hex_mapping.medium_to_coarse[old_med];
        let new_coarse = state.hex_mapping.medium_to_coarse[new_med];
        if old_coarse != new_coarse {
            state.coarse_index.move_entity(old_coarse, new_coarse, entity);
        }
    }
}
```

**Terrain chunk dirtying (for future viewer integration):**
- When server processes a terrain mutation, find affected medium hexes
- Look up `medium_to_chunks` → mark those chunk IDs dirty
- Include dirty chunk IDs in the tick message so viewer knows which
  sub-regions to re-upload

**Tests:**
- Cross-level mapping is consistent (fine→medium→coarse chain is valid)
- Entity move cascade updates all relevant levels
- 10k entity moves per tick: cascade overhead < 0.5ms
- Chunk dirtying: mutation in hex X dirties exactly the expected chunks
- Mutation spanning 2 hexes dirties the union of both hex's chunks

## Verification criteria (full stream)

- [ ] Fine index query at 5m radius scans <10 entities (not hundreds)
- [ ] Combat query performance: 1k queries × 5m radius < 1ms with 10k entities
- [ ] Coarse aggregates match brute-force computation
- [ ] Strategic AI reads aggregates in O(1) per coarse hex
- [ ] Entity move cascade updates all 3 levels correctly
- [ ] Cross-level mapping is consistent at map init
- [ ] Terrain chunk dirtying scopes to affected hexes only
- [ ] No regression in existing spatial queries (medium hex)
- [ ] All existing tests pass (spatial index behavior unchanged for medium tier)
