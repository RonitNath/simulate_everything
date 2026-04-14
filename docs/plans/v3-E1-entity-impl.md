# E1 Implementation Plan — Entity Model, Containment, Lifecycle, Mapgen

Source: `docs/plans/v3-E-entity.md` (E1 scope)
Spec: `docs/specs/v3-entity-unification-2026-04-13.md`

## Deliverables

### 1. `state.rs` — Entity struct and GameState

Entity struct with optional component bags (AoS, per plan E.1):

```rust
pub struct Entity {
    pub id: u32,                        // Public monotonic ID
    pub pos: Option<Vec3>,              // None if contained
    pub hex: Option<Axial>,             // Cached hex membership (derived from pos)
    pub owner: Option<u8>,              // Player owner

    // Containment
    pub contained_in: Option<EntityKey>,
    pub contains: SmallVec<[EntityKey; 4]>,

    // Components
    pub person: Option<Person>,
    pub mobile: Option<Mobile>,
    pub combatant: Option<Combatant>,
    pub vitals: Option<Vitals>,
    pub wounds: Option<WoundList>,
    pub equipment: Option<Equipment>,
    pub weapon_props: Option<WeaponProperties>,
    pub armor_props: Option<ArmorProperties>,
    pub projectile: Option<Projectile>,
    pub structure: Option<Structure>,
    pub resource: Option<Resource>,
}
```

New component types defined inline:
- `Person { role: Role, combat_skill: f32 }` — living being with a role
- `Combatant { facing: f32, target: Option<EntityKey>, attack: Option<AttackState>, cooldown: Option<CooldownState> }` — can fight
- `Structure { structure_type: StructureType, build_progress: f32, integrity: f32, capacity: usize }` — building
- `Resource { resource_type: ResourceType, amount: f32 }` — material/food
- `Role`, `StructureType`, `ResourceType` enums

GameState:
```rust
pub struct GameState {
    pub entities: SlotMap<EntityKey, Entity>,
    pub spatial_index: SpatialIndex,
    pub heightfield: Heightfield,
    pub map_width: usize,
    pub map_height: usize,
    pub num_players: u8,
    pub game_time: f64,
    pub tick: u64,
    pub next_id: u32,
}
```

### 2. `lifecycle.rs` — Spawn, death, cleanup, containment

- `spawn_entity(state, EntityBuilder) -> EntityKey` — assigns ID, inserts, updates spatial index
- `EntityBuilder` — builder pattern for composing entities
- `contain(state, container, contained)` — bidirectional containment
- `uncontain(state, contained)` — remove from container
- `cleanup_dead(state)` — remove dead entities, eject equipment, update containment
- `eject_contained(state, dying_key)` — move contained entities to dying entity's pos

### 3. `mapgen.rs` — V3 entity population

Per player (positioned at general hex from V2 mapgen or evenly spaced):
- 1 settlement structure entity
- ~30 person entities (Farmer/Worker/Idle mix)
- ~5 soldier entities (with equipment from stockpile)
- Starting equipment: swords + leather armor for soldiers

Uses `rand::Rng` seeded deterministically.

### 4. `mod.rs` updates

Add `pub mod state;`, `pub mod lifecycle;`, `pub mod mapgen;`.

## Dependencies (all met)

- S1: Vec3, hex, spatial index ✓
- D1: body zones, wounds, vitals ✓
- W1: weapon/armor properties ✓

## Verification

```bash
cargo build -p simulate-everything-engine
cargo test -p simulate-everything-engine -- v3::state
cargo test -p simulate-everything-engine -- v3::lifecycle
cargo test -p simulate-everything-engine -- v3::mapgen
```
