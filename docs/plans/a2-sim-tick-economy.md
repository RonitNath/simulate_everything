# A2: Sim Tick Rewrite (Economy) — Implementation Plan

## Goal

Replace legacy Population-based economy (farmer/worker production, soldier training)
with entity-based economy operating on individual Person entities. Add explicit
per-person food consumption and Builder construction mechanics. Keep legacy Unit/Convoy
economy unchanged for now (A3 handles combat, B* handles agents).

## Design Decisions

- **Entity economy replaces population economy**: Remove population income loop from
  `generate_resources()`. New `entity_produce_resources()` iterates Person entities
  with Farmer/Worker roles contained in Structure entities. Produces identical amounts
  per-person as legacy system (FARMER_RATE * terrain_value per farmer, WORKER_RATE *
  material_value per worker).
- **Explicit food consumption is NEW behavior**: Every Person entity consumes food
  from the hex stockpile of its container (or own hex if not contained). Starvation
  degrades Person.health. This changes economy balance — acceptable since A2 is a
  rewrite, not an additive layer.
- **Entity role sync on directive**: When legacy `AssignRole` directive runs, also
  update corresponding entity Person roles at that hex. Keeps dual storage consistent.
- **Builder construction**: Person entities with role=Builder at a hex containing a
  Structure entity with build_progress < 1.0 increment build_progress per tick.
- **Soldier training**: Person entities with role=Soldier increase combat_skill per
  tick (replaces legacy Population training field).
- **Legacy Unit income preserved**: Stationary military units still generate
  food/material via legacy `generate_resources()`.
- **Legacy grow_population/migrate_population preserved**: Population growth and
  migration still use legacy Population groups. Entity population grows when legacy
  population grows (sync on growth).

## Constants (new in mod.rs)

- `PERSON_FOOD_RATE: f32 = 0.005` — food consumed per person per tick
- `BUILD_RATE: f32 = 0.02` — build_progress per builder per tick
- `STARVATION_HEALTH_DAMAGE: f32 = 0.01` — health lost per person per tick when starving

## Files Modified

### 1. `crates/engine/src/v2/mod.rs`
- Add PERSON_FOOD_RATE, BUILD_RATE, STARVATION_HEALTH_DAMAGE constants

### 2. `crates/engine/src/v2/sim.rs`
- Add `entity_produce_resources(state)`:
  - Iterate entities with Person component where contained_in points to a Structure entity
  - Farmer: produce terrain_value * FARMER_RATE food at structure hex
  - Worker: produce material_value * WORKER_RATE material at structure hex
  - Uses existing `add_stockpile()` for capped deposit + debug accounting
- Add `entity_train_soldiers(state)`:
  - Iterate entities with Person { role: Soldier } 
  - Increment combat_skill by TRAINING_RATE, cap at SOLDIER_READY_THRESHOLD
- Add `entity_consume_food(state)`:
  - Every Person entity consumes PERSON_FOOD_RATE from hex stockpile
  - If no food available, decrement Person.health by STARVATION_HEALTH_DAMAGE
  - Entity's hex = container's hex if contained, else entity's own pos
- Add `entity_build_structures(state)`:
  - Person entities with role=Builder
  - Find Structure entity at same hex (or in same container)
  - Increment Structure.build_progress by BUILD_RATE, cap at 1.0
- Remove population income loop from `generate_resources()` (farmer/worker/training)
- Integrate new functions into `tick()` after territory computation

### 3. `crates/engine/src/v2/directive.rs`
- In `assign_role()`: after modifying Population groups, sync entity Person roles
  at the same hex for the same owner. Change `count` entity persons from old role
  to new role.

### 4. `crates/engine/src/v2/state.rs`
- Add helper: `entity_hex(entity) -> Option<Axial>` — returns entity's pos, or
  container's pos if contained_in is set
- Add helper: `entities_in_structure(key) -> Vec<EntityKey>` — persons in a structure

### 5. Tests (in sim.rs)
- `entity_farmer_produces_food`: Create farmer entity in structure, tick, check stockpile
- `entity_person_starves_without_food`: Person at hex with no food, tick, check health < 1.0
- `entity_builder_advances_construction`: Builder + structure at < 1.0, tick, check progress
- `entity_soldier_trains`: Soldier entity, tick, check combat_skill increased
- `entity_role_assignment_syncs`: Legacy AssignRole directive, verify entity roles changed
- `entity_person_fed_when_food_available`: Person at hex with food, tick, check health unchanged

## Tick Order (updated)

```
tick(state):
  rebuild_spatial()
  compute_territory()
  update_settlement_types()
  city_ai (every 5 ticks)
  generate_resources()          // MODIFIED: unit income only, no pop income
  entity_produce_resources()    // NEW: entity farmer/worker production
  entity_train_soldiers()       // NEW: entity soldier training
  grow_population()             // Legacy
  migrate_population()          // Legacy
  entity_consume_food()         // NEW: per-person food consumption
  consume_upkeep()              // Legacy: unit/convoy upkeep
  entity_build_structures()     // NEW: builder construction
  combat::resolve_combat()
  rout_weakened_units()
  move_convoys()
  move_units()
  decay_frontier_stockpiles()
  decrement_cooldowns()
  cleanup()
  cleanup_stale_engagements()
  refresh_player_totals()
  tick += 1
```

## Verification

```bash
cargo test -p simulate-everything-engine
cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --seeds 0-4 --ticks 500 --size 30x30 --agents spread,striker
```

Games must complete without panics. Winners may differ from A1 baseline due to
economy rebalancing (expected).
