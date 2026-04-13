# V2 Remaining Systems — Implementation Plan

This plan covers the designed-but-unbuilt V2 systems. These run on **Track B** (engine extension), parallel to Track A (agent iteration on the existing engine). The agent thread is independently tuning SpreadAgent behavior — vision, production, engagement, disengage — and should not be blocked by this work.

## Current State

Implementation status: all planned phases are now landed in a scoped working form.

The V2 engine now includes:
- Hex grid (axial coords, flat-top, even-r offset)
- Continuous tick sim (10hz, movement cooldowns)
- Entity units (strength 100→0, edge-based engagement, 1/sqrt(N) effectiveness)
- Two-resource economy (stationary units generate food + material, units consume food upkeep, starvation damages strength)
- Population roles, soldier training, settlements, and slow civilian migration
- Stockpiles, depots, convoys, and road levels
- Settlement radius-1 local accrual, remote frontier stockpile decay, and settler convoys for deliberate expansion
- Terrain pipeline with height, moisture, rivers, biomes, regions, and height-aware vision/combat/movement
- Fog of war with height bonus
- SpreadAgent updated for role assignment, training, depots, roads, settler launches, and convoy loading
- Web integration (round-robin, WebSocket spectator, replay), including timeout-scored winners at 3000 ticks

Key files:
- `crates/engine/src/v2/state.rs` — terrain, stockpiles, roads, regions, units, population, convoys
- `crates/engine/src/v2/sim.rs` — stockpile economy, population growth/training, convoy + unit movement, cleanup
- `crates/engine/src/v2/directive.rs` — movement/combat plus production, role assignment, convoys, depot/road building
- `crates/engine/src/v2/observation.rs` — Observation, UnitInfo, PopulationInfo, ConvoyInfo
- `crates/engine/src/v2/mapgen.rs` — terrain pipeline, region synthesis, general placement, initial population seeding
- `crates/engine/src/v2/mod.rs` — all tuning constants

## Guiding Principles

- **Each phase produces a working game.** Never break the existing game loop. The agent must still function after each phase (it may ignore new features, but it must not crash).
- **Extend, don't rewrite.** Add fields to existing structs, add new entity types, add new directive variants. Don't restructure what works.
- **Test after each phase.** Run `cargo test`, then run a game via the ASCII API and verify it plays to completion.
- **Commit after each phase.** One commit per phase with a descriptive message.

---

## Phase 1: Two-Resource Economy

**What changes:** Split single `resources: f32` into food + material. All units consume food per tick. Starvation degrades strength.

### Data model changes

`state.rs` — Player:
```rust
pub struct Player {
    pub id: u8,
    pub food: f32,        // was: resources: f32
    pub material: f32,    // NEW
    pub general_id: u32,
    pub alive: bool,
}
```

`state.rs` — Cell:
```rust
pub struct Cell {
    pub terrain_value: f32,    // existing: 0.0-3.0, now specifically = food productivity
    pub material_value: f32,   // NEW: 0.0-2.0, derived from terrain height/type
}
```

`mod.rs` — new constants:
```rust
pub const FOOD_RATE: f32 = 0.1;           // replaces RESOURCE_RATE
pub const MATERIAL_RATE: f32 = 0.05;      // material generation rate
pub const UNIT_FOOD_COST: f32 = 8.0;      // food cost to produce unit (was UNIT_COST=10)
pub const UNIT_MATERIAL_COST: f32 = 5.0;  // material cost to produce unit
pub const UPKEEP_PER_UNIT: f32 = 0.02;    // food consumed per unit per tick
pub const STARVATION_DAMAGE: f32 = 0.5;   // strength lost per tick when food <= 0
```

### Sim loop changes

`sim.rs` — `generate_resources()`:
- Stationary, unengaged units on a hex generate:
  - `food += cell.terrain_value * FOOD_RATE` to owner
  - `material += cell.material_value * MATERIAL_RATE` to owner

`sim.rs` — new `consume_upkeep()` step (add after generate_resources):
- Each player: `food -= unit_count * UPKEEP_PER_UNIT`
- If player food < 0: all units lose `STARVATION_DAMAGE` per tick. Food stays at 0 (no negative).

`directive.rs` — `Produce`:
- Check `food >= UNIT_FOOD_COST && material >= UNIT_MATERIAL_COST`
- Deduct both on production

### Mapgen changes

`mapgen.rs`:
- Generate `material_value` from a separate noise layer (different seed/frequency, biased toward high-terrain-value areas but not identical)
- Or simpler: `material_value = (3.0 - terrain_value) * 0.6` — high food areas have low material, high material areas have low food. Creates geographic tension.

### Observation changes

`observation.rs`:
- `resources: f32` → `food: f32, material: f32`

### Agent compatibility

SpreadAgent's production logic changes from `resources >= UNIT_COST` to `food >= UNIT_FOOD_COST && material >= UNIT_MATERIAL_COST`. Everything else works unchanged. The agent doesn't need to understand upkeep — it just sees its food dropping and units weakening if it over-produces.

### Tests

- Existing tests: update resource references from single float to food/material
- New test: verify starvation damages units when food hits 0
- New test: verify material_value is generated and accessible
- Integration: run a full game, verify it completes without panic

---

## Phase 2: Population Model

**What changes:** Units are no longer produced from thin air. Population exists on hexes. People are assigned roles. Soldiers are trained from population.

### Data model changes

`state.rs` — new struct:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Idle,
    Farmer,
    Worker,    // produces material
    Soldier,   // in training or trained
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Population {
    pub hex: Axial,
    pub owner: u8,
    pub count: u16,        // number of people in this cohort
    pub role: Role,
    pub training: f32,     // 0.0-1.0, only meaningful for Soldier role
}
```

`state.rs` — GameState:
```rust
pub struct GameState {
    // ... existing fields ...
    pub population: Vec<Population>,   // NEW
    pub next_pop_id: u32,              // NEW (if we need IDs)
}
```

### Sim loop changes

`sim.rs` — replace `generate_resources()`:
- Farmers on a hex: `food += count * cell.terrain_value * FARMER_RATE`
- Workers on a hex: `material += count * cell.material_value * WORKER_RATE`
- Idle: no production
- Soldiers: no production, training increases by `TRAINING_RATE` per tick (capped at 1.0)

`sim.rs` — population growth (add step):
- Per hex with population: if owner has food surplus, `growth = base_rate * food_satisfaction * (1 - pop/carrying_capacity)`
- New population spawns as Idle
- Growth only on hexes with Farmer population (food production enables growth)

### New directives

`directive.rs`:
```rust
Directive::AssignRole { hex_q: i32, hex_r: i32, role: Role, count: u16 },
Directive::TrainSoldier { hex_q: i32, hex_r: i32 },  // converts Idle→Soldier, costs material
```

`Directive::Produce` changes: instead of spending resources to create a unit from nothing, it converts trained soldiers (training >= threshold) into a military Unit entity. The population cohort is consumed.

### Unit creation flow

1. Population exists on hex (starts as Idle)
2. Agent assigns role → Soldier (costs material for equipment)
3. Soldier trains over time (training: 0.0 → 1.0)
4. Agent issues Produce → trained soldiers become a Unit entity, population cohort removed
5. Unit operates as before (movement, engagement, combat)

### Mapgen changes

- Each player starts with a population cohort at their general hex: ~20 Idle + ~5 Farmer + ~3 Worker
- Starting units are pre-created as before (5 units per player), representing the initial trained soldiers

### Observation changes

- Add `own_population: Vec<PopulationInfo>` to Observation
- Add `visible_enemy_population: Vec<PopulationInfo>` (population on visible hexes)

### Agent compatibility

SpreadAgent needs to:
1. Assign Idle → Farmer (enough to sustain food)
2. Assign Idle → Worker (enough for material)
3. Assign Idle → Soldier when ready to militarize
4. Produce units from trained soldiers

This is a more complex decision loop but the agent can use simple heuristics: keep 60% farmers, 20% workers, 20% soldiers. Produce when trained soldiers are available.

### Tests

- Test role assignment changes population role
- Test farmers produce food, workers produce material
- Test soldier training progression
- Test unit production from trained soldiers
- Test population growth from food surplus
- Integration: full game completes

---

## Phase 3: Convoys and Depots

**What changes:** Resources no longer teleport to the player pool. They accumulate at hexes and must be physically transported.

### Data model changes

`state.rs` — Cell:
```rust
pub struct Cell {
    pub terrain_value: f32,
    pub material_value: f32,
    pub food_stockpile: f32,      // NEW: food stored at this hex
    pub material_stockpile: f32,  // NEW: material stored at this hex
    pub has_depot: bool,          // NEW: depot increases storage capacity
    pub road_level: u8,           // NEW: 0=none, 1=trail, 2=dirt, 3=paved (Phase 4)
}
```

`state.rs` — new entity:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CargoType { Food, Material }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Convoy {
    pub id: u32,
    pub owner: u8,
    pub pos: Axial,
    pub destination: Axial,
    pub cargo_type: CargoType,
    pub cargo_amount: f32,
    pub capacity: f32,
    pub speed: f32,           // hexes per tick (modified by road level)
    pub move_cooldown: u8,
}
```

`state.rs` — GameState:
```rust
pub struct GameState {
    // ... existing fields ...
    pub convoys: Vec<Convoy>,     // NEW
    pub next_convoy_id: u32,      // NEW
}
```

### Economy changes

**Remove player-level resource pool.** Resources exist physically at hexes.

`sim.rs` — resource generation:
- Farmers produce food → added to `cell.food_stockpile` at their hex
- Workers produce material → added to `cell.material_stockpile` at their hex
- Storage cap per hex: 50 base, 200 with depot

`sim.rs` — unit upkeep:
- Each unit consumes food from `cell.food_stockpile` at its current hex
- If hex food_stockpile is 0: starvation damage
- Units on the move consume from whatever hex they're on

`sim.rs` — convoy movement:
- Convoys pathfind toward destination
- Speed modified by road level: `base_speed * (1.0 + road_level as f32 * 0.5)`
- On arrival: deposit cargo into destination hex stockpile
- Convoys consume food from hex stockpile as they travel (they eat too)

### New directives

```rust
Directive::LoadConvoy { hex_q: i32, hex_r: i32, cargo_type: CargoType, amount: f32 },
    // Creates a convoy at this hex, loads from local stockpile
Directive::SendConvoy { convoy_id: u32, dest_q: i32, dest_r: i32 },
    // Sets convoy destination
Directive::BuildDepot { hex_q: i32, hex_r: i32 },
    // Costs material from local stockpile, sets has_depot=true
```

### Player struct simplification

```rust
pub struct Player {
    pub id: u8,
    // food and material fields REMOVED — resources are on hexes now
    pub general_id: u32,
    pub alive: bool,
}
```

The agent sees total food/material as aggregates in the observation, but they're computed from hex stockpiles, not stored on the player.

### Unit production changes

`Directive::Produce` now requires:
- Trained soldiers at the general's hex (from Phase 2)
- Material in `cell.material_stockpile` at general's hex (for equipment)
- Food in `cell.food_stockpile` at general's hex (initial rations)

### Observation changes

- Add per-hex stockpile info to visible hexes
- Add `own_convoys: Vec<ConvoyInfo>` 
- Add `total_food: f32, total_material: f32` as computed aggregates for strategic planning
- Convoy positions are visible to enemies if in their vision (convoys can be spotted and raided)

### Convoy raiding

Convoys are non-combat entities. If an enemy unit ends on the convoy's hex, or is on an adjacent hex when the convoy moves through, the convoy is captured (cargo transferred to the raiding unit's hex stockpile, convoy removed). No engagement needed — convoys can't fight.

This creates a natural raiding mechanic: fast units sent behind enemy lines to intercept supply convoys.

### Agent compatibility

SpreadAgent needs significant updates:
1. Build depots at general hex and forward positions
2. Create convoys to move food from farming hexes to army positions
3. Route convoys along roads (Phase 4)
4. Protect convoys with escorts (position military units along supply routes)

For a minimal working agent: just build a depot at general hex, create convoys from nearby farming hexes to general, produce from general. The agent won't be good at supply logistics, but the game will function.

### Tests

- Test food accumulates at hex, not player
- Test convoy creation, loading, movement, unloading
- Test depot increases storage capacity
- Test starvation when hex stockpile empty
- Test convoy capture by enemy unit
- Integration: full game with convoy-based economy

---

## Phase 4: Roads

**What changes:** Hexes have road levels that affect movement speed for units and convoys.

### Data model

Already added `road_level: u8` to Cell in Phase 3. Values:
- 0: None (base terrain movement cost)
- 1: Trail (slight speed boost, enables pack animals later)
- 2: Dirt road (significant speed boost, enables carts)
- 3: Paved (maximum speed boost)

### Movement changes

`sim.rs` — movement cooldown:
```rust
let road_bonus = match cell.road_level {
    0 => 0.0,
    1 => 0.3,  // trail: 30% faster
    2 => 0.6,  // dirt: 60% faster  
    3 => 1.0,  // paved: 100% faster (halves cooldown)
    _ => 0.0,
};
let base = BASE_MOVE_COOLDOWN as f32 + terrain_value * TERRAIN_MOVE_PENALTY;
let cooldown = (base * (1.0 - road_bonus * 0.5)).max(1.0) as u8;
```

Same formula applies to convoys.

### Construction

```rust
Directive::BuildRoad { hex_q: i32, hex_r: i32, level: u8 },
```

Requirements per level:
- Trail (1): workers assigned to hex, N ticks of labor, no material
- Dirt (2): workers + material from local stockpile (wood)
- Paved (3): workers + stone from local stockpile (must be transported there)

Construction time scales with terrain difficulty:
```rust
let terrain_multiplier = match terrain_type {
    flat => 1.0,
    hills => 3.0,
    mountain => 8.0,  // derive from terrain_value or height
};
let build_ticks = base_ticks * terrain_multiplier;
```

Road construction is a building-in-progress: workers are assigned, ticks count down, road level increases when complete. This uses the population role system from Phase 2 — workers assigned to a hex with a BuildRoad directive are "builders."

### Observation changes

- Add `road_level` to per-hex terrain info in observation
- Roads are visible terrain features (always visible, like terrain_value)

### Agent compatibility

SpreadAgent: build trails along frequently-traveled routes. Simple heuristic — after a unit moves along a path 3+ times, build a trail there.

### Tests

- Test road reduces movement cooldown
- Test road construction requires workers and time
- Test paved road requires stone material at hex
- Test convoy speed improves on roads
- Integration: game with roads connecting key positions

---

## Phase 5: Terrain Generation Upgrade

**What changes:** Replace basic Perlin noise with the full terrain pipeline: height field, erosion, rivers, moisture, biomes, regions.

This is the largest phase. It changes mapgen only — the sim loop, combat, and agent are unaffected (they read terrain_value and material_value which are still floats on Cell, just generated differently).

### Data model changes

`state.rs` — Cell:
```rust
pub struct Cell {
    pub terrain_value: f32,      // food productivity (from biome + moisture + height)
    pub material_value: f32,     // material productivity (from geology)
    pub height: f32,             // NEW: elevation from noise + erosion
    pub moisture: f32,           // NEW: from rain shadow simulation
    pub biome: Biome,            // NEW
    pub is_river: bool,          // NEW
    pub water_access: f32,       // NEW: 0.0-1.0
    // ... stockpile, depot, road fields from earlier phases
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Biome {
    Desert, Steppe, Grassland, Forest, Jungle, Tundra, Mountain,
}
```

`state.rs` — GameState:
```rust
pub struct GameState {
    // ... existing fields ...
    pub regions: Vec<Region>,    // NEW
}

pub struct Region {
    pub id: u16,
    pub name: String,
    pub archetype: RegionArchetype,
    pub hexes: Vec<Axial>,       // which hexes belong to this region
    pub avg_fertility: f32,
    pub avg_minerals: f32,
    pub defensibility: f32,
}

pub enum RegionArchetype {
    RiverValley, Highland, MountainRange, CoastalPlain,
    Forest, Desert, Pass, Delta, Plateau, Steppe,
}
```

### Mapgen pipeline

`mapgen.rs` — complete rewrite:

1. **Place players** with rotational symmetry
2. **Generate region graph** — abstract nodes with archetypes, edges with border types. Fairness constraints: each player's Voronoi partition gets equivalent region composition.
3. **Assign hexes to regions** — constrained flood fill from region seeds, Voronoi relaxation for organic boundaries
4. **Generate height field** — layered noise parameterized per region archetype. Ridged noise for mountain regions. Low noise amplitude for valley regions.
5. **Hydraulic erosion** — particle-based, ~10K droplets. Adapts to hex grid using hex neighbors for gradient computation.
6. **Flow accumulation** — compute drainage per hex, mark rivers where accumulation > threshold. Strahler numbers for river width.
7. **Moisture simulation** — prevailing wind direction, moisture from ocean/rivers, rain shadow behind mountains
8. **Biome assignment** — Whittaker diagram lookup from temperature (latitude + elevation) × moisture
9. **Derive terrain_value and material_value** — from biome, moisture, height, river proximity
10. **Fairness validation** — compare per-player Voronoi partitions. Retry if imbalanced beyond threshold.
11. **Region naming** — procedural names from archetype + distinctive feature

### Dependencies

- `noise` crate (already in use)
- No new crate dependencies needed for erosion (implement particle simulation directly)

### Height effects on gameplay

Height differences between adjacent hexes affect:
- **Movement cost**: steep slope = extra cooldown
- **Combat**: uphill attacker penalty (multiply damage dealt by `1.0 - 0.02 * height_diff`)
- **Vision**: higher hex sees further (vision radius + 1 per 10 height units above surroundings)

These are sim.rs changes, not mapgen changes, but they depend on height data existing.

### Observation changes

- Add height, moisture, biome, is_river, water_access to terrain info
- Add region info: name, archetype for each hex's region
- Terrain is still always visible (static data sent once)

### Agent compatibility

SpreadAgent doesn't need changes — it reads terrain_value as before. The Centurion agent (future) will use region data for strategic reasoning.

### Tests

- Test height field generates with reasonable range
- Test rivers follow downhill paths
- Test biomes vary across map
- Test region assignment covers all hexes
- Test fairness validation catches severely imbalanced maps
- Test height affects movement cost and combat
- Integration: game on new terrain plays to completion

---

## Phase Ordering and Dependencies

```
Phase 1: Two-Resource Economy
  └→ Phase 2: Population Model (depends on food/material split)
      └→ Phase 3: Convoys and Depots (depends on population for drivers)
          └→ Phase 4: Roads (depends on convoy movement system)

Phase 5: Terrain Generation Upgrade (independent — can run parallel to phases 2-4)
```

Phase 5 only touches mapgen. Phases 1-4 touch the sim loop and economy. They're independent tracks that can be developed in parallel if desired, but phases 1-4 are sequential (each builds on the previous).

**Recommended execution:** Phases 1-4 sequentially, Phase 5 in parallel with phases 3-4.

## Handoff Points with Track A (Agent Thread)

After each phase, the agent thread should be notified so SpreadAgent can be updated:

| Phase | Agent change needed |
|-------|-------------------|
| 1 | Production check: food >= X AND material >= Y |
| 2 | Role assignment heuristics (farmer/worker/soldier ratio) |
| 3 | Convoy creation and routing (significant new behavior) |
| 4 | Road building heuristics |
| 5 | None (agent reads terrain_value as before) |

Phase 3 is the biggest agent impact — the agent must learn to manage supply or its armies starve.

## Verification

After each phase:
```bash
cargo test
cargo run --bin simulate_everything_cli -- v2-bench --games 10  # if harness exists
curl -s "http://localhost:3333/api/v2/ascii?seed=42&width=30&height=30&ticks=200"
```

The ASCII output should show a game that plays to completion. Watch for: units starving (expected if agent doesn't manage supply well), convoys moving between hexes, roads appearing on frequently-traveled paths.
