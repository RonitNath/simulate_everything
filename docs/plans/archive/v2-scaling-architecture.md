# V2 Scaling Architecture — Implementation Plan

Target: support 100k hex tiles and 10k units without architectural rewrites later.
This is an engine internals refactor — no new gameplay systems, no new directives.

## Current Bottlenecks

1. **O(n) entity lookups**: `units.iter().find(|u| u.id == id)` called dozens of times per tick in combat, directives, sim. At 10k units this dominates tick time.
2. **O(n) spatial queries**: "find units adjacent to hex X" scans all units. At 10k units × 6 neighbors this is 60k comparisons per query.
3. **Full-grid observation cloning**: 7 vectors of 100k floats/bools cloned per agent per poll. At 4 agents polling every 5 ticks = ~560k entries/tick.
4. **No terrain fog**: `terrain` and `material_map` sent unmasked — agents see the entire map on tick 0.
5. **No economic invariant checking**: resource creation/destruction across settlements, convoys, migration, decay, upkeep has no conservation assertion.
6. **Spectator gets agent-grade data**: WebSocket spectator receives full Observation (grids, exact stockpiles, population details) when it only needs render data (positions, ownership, engagement pairs). At 10 ticks/sec on 100k tiles this is ~1MB/s per spectator instead of ~10KB/s.

## Phase 1: SlotMap Entity Storage

Replace `Vec<Unit>`, `Vec<Population>`, `Vec<Convoy>` with `SlotMap` from the `slotmap` crate.

### Changes

**Cargo.toml**: Add `slotmap = "1"` to `crates/engine`.

**state.rs**:
```rust
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct UnitKey;
    pub struct PopKey;
    pub struct ConvoyKey;
}

pub struct GameState {
    // ...
    pub units: SlotMap<UnitKey, Unit>,
    pub population: SlotMap<PopKey, Population>,
    pub convoys: SlotMap<ConvoyKey, Convoy>,
    // Remove next_unit_id, next_pop_id, next_convoy_id — SlotMap generates keys
}
```

Unit, Population, Convoy structs drop their `id: u32` field. The SlotMap key IS the identity.

**Directive enum**: Change `unit_id: u32` → `unit_id: UnitKey`, `convoy_id: u32` → `convoy_id: ConvoyKey`.

**Observation structs**: `UnitInfo.id` → `UnitKey`, `ConvoyInfo.id` → `ConvoyKey`, `PopulationInfo.id` → `PopKey`.

**All lookup sites**: Replace `state.units.iter().find(|u| u.id == id)` with `state.units.get(key)`. Replace `state.units.iter().position(...)` + index access with direct key access. Grep for `.iter().find(|u| u.id` and `.iter().position(|u| u.id` across all v2/ files.

**Serialization**: SlotMap keys are internal only. At serialization boundaries (replay payloads, WebSocket spectator snapshots, `UnitInfo`/`ConvoyInfo` in observations), map `UnitKey → u32` using a per-tick key-to-id table. Frontend and replay consumers continue to see stable numeric IDs. This avoids breaking the existing `V2UnitSnapshot.id: number` contract.

### Verification
- `cargo test` passes
- No `.iter().find(|u| u.id` or `.iter().position(|u| u.id` remaining in v2/ code
- Replay record/reconstruct test still passes with stable numeric IDs
- Frontend `V2UnitSnapshot.id` type unchanged

## Phase 2: Spatial Index

Add a hex-indexed lookup table for O(1) "what entities are on/near this hex."

### Changes

**New file `crates/engine/src/v2/spatial.rs`**:
```rust
use smallvec::SmallVec;
use super::hex::Axial;
use super::state::UnitKey;

pub struct SpatialIndex {
    width: usize,
    height: usize,
    /// One entry per grid cell, lists unit keys present on that hex.
    cells: Vec<SmallVec<[UnitKey; 4]>>,
}

impl SpatialIndex {
    pub fn new(width: usize, height: usize) -> Self { ... }
    pub fn rebuild(&mut self, units: &SlotMap<UnitKey, Unit>) { ... }
    pub fn units_at(&self, ax: Axial) -> &[UnitKey] { ... }
    pub fn units_adjacent(&self, ax: Axial) -> impl Iterator<Item = UnitKey> { ... }
}
```

**Cargo.toml**: Add `smallvec = "1"`.

**GameState**: Add `pub spatial: SpatialIndex` field. Rebuild at start of each tick (before any system runs). Cost: O(units) per tick, paid once.

**Combat, sim, directive**: Replace `state.units.iter().filter(|u| u.pos == hex)` with `state.spatial.units_at(hex)`. Replace adjacency scans with `state.spatial.units_adjacent(hex)`.

**Convoy and population**: If needed, add separate spatial indices. Population is less hot (only queried during growth/migration, not combat). Convoys are queried during raiding. Evaluate after profiling.

### Verification
- `cargo test` passes
- Profile: combat tick time should be independent of total unit count for non-adjacent units

## Phase 3: Terrain Fog (Scouted Mask)

Add per-player `scouted` bitmask. Terrain is revealed when scouted and stays revealed. Dynamic state (stockpiles, units, population) uses the existing `visible` mask.

### Changes

**GameState**: Add `pub scouted: Vec<Vec<bool>>` — one bool-vec per player, indexed by grid offset.

**sim.rs / vision.rs**: After computing `visible`, OR it into `scouted[player_id]`. Scouted bits are monotonically set, never cleared.

**observation.rs**:
- `terrain`: masked by `scouted` (0.0 for unscouted hexes)
- `material_map`: masked by `scouted`
- `road_levels`: masked by `scouted` (roads persist in memory)
- `height`: masked by `scouted` (new field to add to observation — agents need height for pathfinding)
- `food_stockpiles`, `material_stockpiles`, `stockpile_owner`: masked by `visible` (already done)
- Add `scouted: Vec<bool>` to Observation struct

**Agent**: SpreadAgent needs to handle unknown terrain. For unscouted hexes, assume average terrain value for pathfinding decisions. Exploration becomes a real tradeoff.

**mapgen.rs**: Starting hexes within vision radius of generals are pre-scouted.

### Verification
- Test: unscouted hex terrain reads as 0.0 in observation
- Test: scouted hex retains terrain after losing vision
- Test: starting area is pre-scouted
- `cargo test` passes

## Phase 4: Delta Observations

Replace full-grid observation cloning with initial state + per-tick deltas.

### Changes

**New structs in observation.rs**:
```rust
pub struct InitialObservation {
    pub width: usize,
    pub height: usize,
    pub player: u8,
    // Full scouted terrain snapshot (only scouted hexes have values)
    pub terrain: Vec<f32>,
    pub material_map: Vec<f32>,
}

pub struct ObservationDelta {
    pub tick: u64,
    pub player: u8,
    // Newly scouted hexes since last observation
    pub newly_scouted: Vec<(usize, f32, f32)>, // (index, terrain, material)
    // Changed hex states (entered/left vision, stockpile changes)
    pub hex_changes: Vec<HexDelta>,
    // Entity updates
    pub own_units: Vec<UnitInfo>,
    pub visible_enemies: Vec<UnitInfo>,
    pub own_population: Vec<PopulationInfo>,
    pub visible_enemy_population: Vec<PopulationInfo>,
    pub own_convoys: Vec<ConvoyInfo>,
    pub visible_enemy_convoys: Vec<ConvoyInfo>,
    pub visible: Vec<bool>,
    pub total_food: f32,
    pub total_material: f32,
}

pub struct HexDelta {
    pub index: usize,
    pub food_stockpile: f32,
    pub material_stockpile: f32,
    pub stockpile_owner: Option<u8>,
    pub road_level: u8,
}
```

**Agent trait**:
```rust
pub trait Agent: Send {
    fn name(&self) -> &str;
    fn init(&mut self, obs: &InitialObservation);
    fn act(&mut self, delta: &ObservationDelta) -> Vec<Directive>;
    fn reset(&mut self) {}
}
```

**Change tracking**: Add `BitVec` (from `bitvec` crate) per grid for stockpile changes. Set bit when `add_stockpile`, `capture_hex`, `decay_frontier_stockpiles`, or any other grid mutation runs. At observation time, iterate set bits to build `hex_changes`. Clear after observation.

**Backward compatibility**: Keep `observe()` as a convenience that builds a full observation from InitialObservation + delta. Useful for tests, replay reconstruction, and simple agents.

**SpreadAgent**: Must maintain internal map state. `init()` stores the initial terrain. `act()` applies deltas to internal state before decision-making. This is more complex but necessary for scale.

### Verification
- Test: InitialObservation + sequence of deltas reconstructs same state as full observe()
- Test: only changed hexes appear in hex_changes
- Profile: observation construction time proportional to changes, not grid size

## Phase 5: Spectator/Agent Observation Split

Separate the WebSocket spectator payload from the AI agent observation. They have fundamentally different needs: the spectator renders sprites, the agent makes decisions.

### Three observation types

**`SpectatorInit`** — sent once on WebSocket connect:
```rust
pub struct SpectatorInit {
    pub width: usize,
    pub height: usize,
    pub terrain: Vec<f32>,       // full grid — spectators see everything
    pub material_map: Vec<f32>,
    pub height_map: Vec<f32>,
    pub region_ids: Vec<u16>,
    pub player_count: u8,
}
```

**`SpectatorSnapshot`** — sent every tick over WebSocket. Render-focused, small:
```rust
pub struct SpectatorSnapshot {
    pub tick: u64,
    pub units: Vec<SpectatorUnit>,        // id, owner, q, r, strength, is_general
    pub engagements: Vec<(u32, u32)>,     // (unit_a_id, unit_b_id) pairs
    pub convoys: Vec<SpectatorConvoy>,    // id, owner, q, r, cargo_type
    pub hex_ownership: Vec<Option<u8>>,   // stockpile_owner grid (or delta)
    pub road_levels: Vec<u8>,             // or delta of changed hexes
    pub settlements: Vec<(i32, i32, u8)>, // (q, r, owner) for settlement markers
    pub players: Vec<SpectatorPlayer>,    // id, alive, score
}

pub struct SpectatorUnit {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub strength: f32,
    pub is_general: bool,
    pub engaged: bool,
}

pub struct SpectatorConvoy {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub cargo_type: CargoType,
}

pub struct SpectatorPlayer {
    pub id: u8,
    pub alive: bool,
    pub population: u16,         // total pop count
    pub territory: u16,          // owned hex count
    pub food_level: u8,          // 0-3 bucket (starving/low/ok/surplus), not exact float
    pub material_level: u8,      // same
}
```

**`Observation` / `ObservationDelta`** — the existing agent interface from Phase 4. Fog-masked, detailed. Never touches the WebSocket.

### What spectators DON'T get
- Exact stockpile floats per hex (just ownership color + food/material level buckets on player HUD)
- Population role breakdowns per hex
- Training progress on soldier cohorts
- Convoy cargo amounts or destinations
- The `scouted` bitmask (spectators see everything — fog is for agents)

### Changes

**New file `crates/engine/src/v2/spectator.rs`**: `SpectatorInit`, `SpectatorSnapshot`, `SpectatorUnit`, `SpectatorConvoy`, `SpectatorPlayer` structs + `fn snapshot(state: &GameState) -> SpectatorSnapshot` builder.

**`crates/web/src/v2_roundrobin.rs`**: Replace current observation serialization over WebSocket with `SpectatorInit` on connect + `SpectatorSnapshot` per tick.

**`crates/web/src/v2_protocol.rs`**: Update WebSocket message types to distinguish `Init` and `Snapshot` frames.

**Frontend**: Update `V2UnitSnapshot` type to match `SpectatorUnit`. Drop fields that no longer arrive. Add `SpectatorPlayer` to HUD rendering.

### Size estimate

On a 30×30 map with 20 units, 5 convoys, 2 players:
- Current full Observation: ~30KB serialized
- SpectatorSnapshot: ~1KB serialized
- At 10 ticks/sec: 300KB/s → 10KB/s per spectator

On a 300×300 map (100k tiles) with 500 units:
- Full Observation: ~3MB serialized
- SpectatorSnapshot: ~15KB serialized (units + hex_ownership delta)

### Verification
- Frontend renders correctly from SpectatorSnapshot (no regressions)
- WebSocket bandwidth measured before/after
- Agent behavior unchanged (agents never see SpectatorSnapshot)

## Phase 6: Economic Conservation Assertion

Debug-mode check that resources are conserved across tick boundaries.

### Changes

**sim.rs**: Add `#[cfg(debug_assertions)]` block at start and end of `tick()`:

```rust
pub fn tick(state: &mut GameState) {
    #[cfg(debug_assertions)]
    let pre_totals = economic_totals(state);

    // ... existing tick logic ...

    #[cfg(debug_assertions)]
    {
        let post_totals = economic_totals(state);
        let food_delta = post_totals.food - pre_totals.food;
        let expected_delta = pre_totals.food_produced - pre_totals.food_consumed;
        assert!(
            (food_delta - expected_delta).abs() < 0.01,
            "food conservation violated: delta={food_delta}, expected={expected_delta}"
        );
        // Same for material
    }
}

fn economic_totals(state: &GameState) -> EconomicSnapshot {
    // Sum: all hex stockpiles + all convoy cargo + player aggregates
    // Track: food produced this tick, food consumed this tick
}
```

This requires threading production/consumption accumulators through the tick. Add a `TickAccumulator` struct that each system writes to, passed through the tick pipeline. Only exists in debug builds.

### Verification
- Debug build tests pass (no conservation violations)
- Release build has zero overhead (cfg gated)

## Phasing and Dependencies

```
Phase 1 (SlotMap)       — no dependencies, can start immediately
Phase 2 (Spatial)       — depends on Phase 1 (uses UnitKey)
Phase 3 (Terrain fog)   — independent of Phase 1-2
Phase 4 (Delta obs)     — depends on Phase 3 (scouted mask) and Phase 1 (entity keys)
Phase 5 (Spectator)     — depends on Phase 1 (entity key→id mapping)
Phase 6 (Debug econ)    — independent, can land anytime
```

Phases 1+3+6 can run in parallel. Phase 2 follows 1. Phases 4+5 follow 1+3.
Phase 5 can also land independently as a quick win before Phase 1 — the spectator split doesn't require SlotMap, it just benefits from the key→id mapping convention if SlotMap is already in.

## What This Does NOT Cover

- Full ECS framework adoption (not needed — typed SlotMaps with spatial index suffice)
- Parallel system execution via ECS scheduler (use rayon within hot systems instead)
- Region-level terrain hints (V4 information layer, not V2)
- Population or convoy spatial indices (evaluate after profiling; combat is the hot path)
