# A1: Entity Model and State — Implementation Plan

## Goal

Add V3 unified Entity model alongside existing V2 types. The entity SlotMap is the
new canonical representation. Old SlotMaps (units, convoys, population, settlements)
remain as a compatibility layer so sim.rs, combat.rs, directive.rs, city_ai.rs,
observation.rs, vision.rs, agents, spectator, and replay continue to compile and
function unchanged.

## Design Decisions

- **Dual storage**: `entities: SlotMap<EntityKey, Entity>` added to GameState alongside
  the four legacy SlotMaps. This IS the "thin compatibility layer" — old code uses old
  fields, new code uses entities. A2/A3 migrates sim.rs to entities and removes old fields.
- **Mapgen creates both**: `generate()` spawns entities AND populates legacy SlotMaps.
  No sync layer needed — both are authoritative for their consumers.
- **SpatialIndex**: Add EntityKey-based methods. Keep UnitKey methods for compatibility.
- **Component types**: Defined as specified in the V3 spec (Person, Mobile, Vision,
  Combatant, Resource, Structure).
- **Role enum**: Add `Builder` variant to existing Role enum in state.rs.

## Files Modified

### 1. `crates/engine/src/v2/state.rs`
- Add `EntityKey` via `new_key_type!`
- Add component structs: Person, Mobile, Vision, Combatant, Resource, Structure
- Add enums: ResourceType, StructureType (reuse existing SettlementType mapping)
- Add Entity struct with component bags + containment
- Add `pub entities: SlotMap<EntityKey, Entity>` and `pub next_entity_id: u32` to GameState
- Add `Builder` variant to Role enum
- Add query methods on GameState:
  - `entity_units()` → entities with Person + Mobile + Combatant
  - `entity_structures()` → entities with Structure
  - `resources_at(hex)` → Resource entities at hex
  - `entities_at(hex)` → all entities at hex
  - `contained_in(key)` → entities inside another
- Keep all existing types and fields unchanged

### 2. `crates/engine/src/v2/mapgen.rs`
- In `generate()`, after creating legacy SlotMaps, also create entities:
  - Each Unit → Entity { person(Soldier), mobile, vision, combatant }
  - Each Population group → individual Entity { person(role) } contained in settlement entity
  - Each Settlement → Entity { structure(type), capacity }
- Store entity keys for containment linking

### 3. `crates/engine/src/v2/spatial.rs`
- Add `entity_cells: Vec<SmallVec<[EntityKey; 4]>>` field
- Add `rebuild_entities()` method
- Add `entities_at()`, `has_entity_at()`, `entities_adjacent()` methods
- Keep existing UnitKey methods unchanged

### 4. `crates/engine/src/v2/mod.rs`
- No constant changes needed (entity defaults come from spec)

### 5. Tests (in state.rs or new test module)
- Entity creation with various component combinations
- Containment: put person in structure, verify contained_in/contains
- Query methods return correct subsets
- Mapgen produces valid entity state alongside legacy state
- All existing tests pass unchanged

## Verification

```bash
cargo test -p simulate-everything-engine
cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --seeds 0-4 --ticks 500 --size 30x30 --agents spread,striker
```

Games must play to completion with same winners for same seeds (entity system is
additive; legacy game loop unchanged).
