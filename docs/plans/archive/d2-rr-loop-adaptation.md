# D2: RR Loop Adaptation — Entity Snapshots in Replay

## Assessment

The D2 spec asks to update the RR loop for an entity-based engine. Most items
are already handled by D1 (spectator protocol) or premature (three-layer agents
have types but no implementations). The one actionable gap: replay `Frame` does
not capture entity data from `state.entities`.

## Scope

1. Add `EntitySnapshot` struct to `replay.rs` that captures entity component
   bags with public IDs instead of SlotMap keys.
2. Add `entity_snapshots: Vec<EntitySnapshot>` to `Frame`.
3. Update `capture_frame()` to snapshot `state.entities`.
4. Update `reconstruct_state()` to rebuild the entities SlotMap from snapshots.
5. Verify with `cargo test -p simulate-everything-engine`.

## Key files

- `crates/engine/src/v2/replay.rs` — Frame, capture_frame, reconstruct_state
- `crates/engine/src/v2/state.rs` — Entity struct (read-only reference)

## EntitySnapshot design

Entity contains SlotMap keys (EntityKey) in `contained_in`, `contains`, and
`combatant.engaged_with` that won't survive serialization. EntitySnapshot uses
public IDs (u32) instead:

```rust
pub struct EntitySnapshot {
    pub id: u32,
    pub q: Option<i32>,
    pub r: Option<i32>,
    pub owner: Option<u8>,
    pub contained_in_id: Option<u32>,
    pub contains_ids: Vec<u32>,
    pub person: Option<Person>,
    pub mobile: Option<MobileSnapshot>,
    pub vision: Option<Vision>,
    pub combatant_engaged_ids: Vec<u32>,
    pub combatant_facing: Option<f32>,
    pub resource: Option<Resource>,
    pub structure: Option<Structure>,
}
```

MobileSnapshot flattens Axial to (i32, i32) tuples for the route.

## Not in scope

- Three-layer agent integration (no implementations exist)
- Changing SpectatorEntity to build from state.entities (currently builds from old types)
- Removing old unit/convoy/population/settlement fields from Frame
