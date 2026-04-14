# V3 Domain: E — Entity Model and Sim Tick

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Entity Model + Simulation Tick)
Sequencing: `docs/plans/v3-sequencing.md`

## Purpose

The entity model and sim tick are the integration layer — they pull together all
domain systems (spatial, movement, damage, weapons, agents) into a unified game
loop. This domain covers: the Entity struct with component bags, containment,
mapgen, and the tick function that orchestrates all systems.

This is NOT a design domain for /design agents — the design is in the other 6
domains. This is the assembly domain: take the designed systems and wire them
into a working game.

## Design Questions

### E.1 Entity Struct Layout

- The spec defines Entity with 9 optional components. In Rust, `Option<T>` for
  each component means the Entity struct is large (sum of all component sizes +
  discriminants). At 10k entities, struct size matters for cache performance.
- Alternative: SoA (struct-of-arrays) — separate SlotMaps for each component type,
  keyed by EntityKey. Person components in one contiguous array, Mobile in another.
  Cache-friendly for system passes that only read one component type (e.g., the
  bleed system only reads Body + Person).
- Recommendation for V3.0: start with AoS (components inline on Entity). Measure.
  Switch to SoA if profiling shows cache misses in hot loops. The EntityKey
  abstraction means callers don't care about storage layout.
- Containment: `contained_in: Option<EntityKey>` and `contains: SmallVec<[EntityKey; 4]>`.
  Containment must be bidirectionally consistent. When adding entity B to A's
  contains list, B's contained_in must be set to A. A helper function enforces
  this.

### E.2 Entity Lifecycle

- Creation: `GameState::spawn_entity(components...) -> EntityKey`. Assigns
  monotonic public ID, inserts into SlotMap, updates spatial index if pos is Some.
- Death: entity death (blood <= 0) goes through cleanup_dead in the tick.
  The entity becomes an inert corpse: Mobile, Combatant, and stack membership
  are stripped. Equipment remains contained_in the corpse. The entity stays in
  the SlotMap at its position. No automatic removal — disposal (burial, looting)
  is an agent-issued task. Battlefields accumulate bodies and equipment.
- Containment on death: equipment stays contained in the corpse (not ejected).
  If a container structure is destroyed, contained entities are ejected.
  If a contained entity dies, it stays in the container's contains list as inert.
- Projectile on impact: arrow entities that embed in a target become inert
  and stay at the impact position. Arrows that hit ground stay as inert entities.
  No despawn in V3.0.

### E.3 Mapgen

- V2 mapgen already generates hex terrain with height, moisture, biomes, regions.
  V3 mapgen additionally spawns entity populations.
- Per player: spawn at general hex with ~30 person entities (mix of Farmer, Worker,
  Idle), ~5 Soldier entities (with basic equipment from starting stockpile), 1
  General entity (special person or structure?), 1 settlement structure, starting
  equipment stockpile.
- Equipment stockpile: a structure entity containing weapon and armor entities.
  Starting equipment: enough swords + leather armor for the initial soldiers.
  Some raw materials for production.
- Terrain entities: V3.0 does not need resource entities on the map. Resources
  are produced by farmers/workers and accumulate in stockpile (cell-level floats
  from V2, or per-hex resource entities?). Decision needed: are stockpiles
  cell-level floats or entity piles? Cell-level is simpler and carries from V2.
  Entity piles are more consistent with "everything is an entity." Recommendation:
  cell-level stockpiles for V3.0, entity piles in future.

### E.4 Sim Tick Integration

The tick function orchestrates all systems in order. Each system is implemented
in its own domain. The tick just calls them in sequence.

```rust
pub fn tick(state: &mut GameState, dt: f64) {
    // Spatial (domain S)
    spatial::rebuild_index(state);
    spatial::compute_territory(state);

    // Agents (domain A)
    agents::run_strategy(state, dt);
    agents::run_operations(state, dt);
    agents::run_tactical(state, dt);

    // Commands
    commands::execute(state);

    // Economy
    economy::produce_resources(state, dt);
    economy::consume_food(state, dt);
    vitals::recover_stamina(state, dt);

    // Movement (domain M)
    steering::compute_forces(state);
    movement::integrate(state, dt);
    collision::resolve(state);

    // Combat (domains D + W)
    combat::resolve_melee(state, dt);
    projectile::advance(state, dt);
    projectile::detect_impacts(state);
    damage::apply_impacts(state);
    vitals::apply_bleed(state, dt);

    // Structures
    structures::update_construction(state, dt);

    // Cleanup
    lifecycle::cleanup_dead(state);
    lifecycle::check_elimination(state);

    state.game_time += dt;
    state.tick += 1;
}
```

Key ordering constraints:
- Spatial index must be rebuilt before anything queries it.
- Agent commands execute before simulation — agents decide, then the world resolves.
- Movement before combat — entities reach their positions, then fight.
- Melee before projectile impacts — simultaneous melee and ranged in the same tick.
- Bleed after all damage sources — accumulated wounds bleed together.
- Cleanup last — dead entities are removed after all systems have run.

### E.5 Economy

- V2 has a working economy: stockpiles per hex, food/material production, upkeep,
  starvation. This carries to V3 largely unchanged.
- The key change: V2's "units generate resources" becomes "person entities with
  Farmer/Worker role, located in a structure, generate resources to hex stockpile."
  Same effect, entity-based.
- Equipment production: a Workshop structure with Worker persons inside produces
  weapon/armor entities from material stockpile. Production time = ticks × workers.
  Output: an equipment entity added to the workshop's contains list.

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| E1 | 2 | S1, D1, W1 | Entity struct, containment, mapgen, lifecycle |
| E2 | 3 | E1, M1, D2, W2, W3 | Sim tick loop integrating all systems |

## Key Files (Expected)

- `crates/engine/src/v3/state.rs` — Entity, GameState, component types
- `crates/engine/src/v3/lifecycle.rs` — spawn, death, cleanup, containment
- `crates/engine/src/v3/mapgen.rs` — terrain + entity population generation
- `crates/engine/src/v3/sim.rs` — tick function, system orchestration
- `crates/engine/src/v3/economy.rs` — resource production, consumption, equipment production
- `crates/engine/src/v3/mod.rs` — constants, module declarations

## Constraints

- Entity struct size should be profiled. If > 256 bytes, consider SoA.
- The tick function must complete in < 10ms at 500 entities (100 ticks/sec headroom
  at the default 1 tick/sec).
- Mapgen must produce a playable starting state: each player has enough food
  production, enough soldiers, and enough equipment to survive and grow.
- All state transitions must be deterministic for replay.
