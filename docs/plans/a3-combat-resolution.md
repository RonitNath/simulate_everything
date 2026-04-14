# A3: Individual Facing-Based Combat — Implementation Plan

## Goal

Add per-tick individual combat resolution for entities with Combatant component.
Damage depends on attack angle relative to defender facing (shield arc system).
No engagement lock — combat is moment-to-moment. Dead entities removed on cleanup.

Legacy Unit engagement combat preserved (agents still issue Engage/Disengage
directives). Entity combat runs alongside it. When B4 (tactical layer) is built,
agents will switch to entity-level commands and legacy combat can be removed.

## Design Decisions

- **Dual combat systems**: Entity combat added alongside legacy Unit combat.
  Same pattern as A2 economy — both run until agents are migrated (B4).
- **No engagement lock**: Entity combatants attack every tick they have enemies
  in range (same hex or adjacent hex). No explicit engage/disengage flow.
- **Facing auto-update**: Simple heuristic — face nearest enemy. Tactical AI (B4)
  will control facing; for now it's automatic.
- **Attack angle from hex centers**: `atan2(dy, dx)` from attacker hex center to
  defender hex center using `axial_to_pixel()`. For same-hex combat, attack angle
  = attacker's facing direction.
- **Shield arc modifiers** (facing difference = |attack_angle - defender.facing|):
  - Front (< SHIELD_ARC_HALF): 0.3x (shield blocks most)
  - Side (< PI/2): 0.7x
  - Rear (>= PI/2): 1.5x
- **Death at health <= 0**: Entity removed from SlotMap, removed from
  container.contains, containment links cleaned up.
- **Snapshot-based damage**: Read all positions/facing/health at tick start,
  then apply damage. Prevents ordering effects.

## Constants (new in mod.rs)

- `DAMAGE_PER_TICK: f32 = 0.02` — base damage multiplier per attacker per tick
- `SHIELD_ARC_HALF: f32 = PI/6` — half-width of frontal shield arc (30deg)
- `FRONT_MODIFIER: f32 = 0.3`
- `SIDE_MODIFIER: f32 = 0.7`
- `REAR_MODIFIER: f32 = 1.5`

## Damage Sizing

With combat_skill = 1.0 (trained soldier):
- Head-on: 1.0 * 0.02 * 0.3 = 0.006/tick -> 167 ticks to kill
- Side: 1.0 * 0.02 * 0.7 = 0.014/tick -> 71 ticks to kill
- Rear: 1.0 * 0.02 * 1.5 = 0.030/tick -> 33 ticks to kill

Shield walls very durable, flanking/rear attacks decisive.

## Files Modified

### 1. `crates/engine/src/v2/mod.rs`
- Add DAMAGE_PER_TICK, SHIELD_ARC_HALF, FRONT_MODIFIER, SIDE_MODIFIER, REAR_MODIFIER

### 2. `crates/engine/src/v2/hex.rs`
- Add `axial_to_pixel(ax: Axial) -> (f32, f32)` — flat-top hex center coords
  - x = sqrt(3) * (q + r/2.0)
  - y = 1.5 * r

### 3. `crates/engine/src/v2/combat.rs`
- Add `entity_resolve_combat(state)`:
  - Snapshot: collect all combatant entities with (key, pos, owner, facing, combat_skill, health)
  - For each combatant, find hostile combatants on same hex or adjacent hexes
  - Auto-face: update facing toward nearest enemy
  - Compute attack angle: atan2(dy, dx) from attacker to defender hex center
    (same-hex: use attacker's facing direction)
  - Compute facing_diff: normalize |attack_angle - defender.facing| to [0, PI]
  - Apply modifier: front/side/rear
  - damage = attacker.combat_skill * DAMAGE_PER_TICK * modifier
  - Accumulate damage per defender, apply to Person.health
- Add `entity_cleanup_dead(state)`:
  - Collect entity keys where Person.health <= 0
  - Remove from container.contains lists
  - Remove from entities SlotMap
- Keep ALL existing legacy functions unchanged

### 4. `crates/engine/src/v2/sim.rs`
- Add `combat::entity_resolve_combat(state)` after `combat::resolve_combat(state)`
- Add `combat::entity_cleanup_dead(state)` in cleanup() before unit cleanup

### 5. Tests (in combat.rs)
- `entity_head_on_symmetric`: Two equal combatants facing each other, symmetric damage
- `entity_flanking_more_damage`: Two attackers from different angles, defender takes more
- `entity_rear_attack_1_5x`: Attacker behind defender, verify 1.5x modifier
- `entity_shield_arc_0_3x`: Attacker within shield arc, verify 0.3x modifier
- `entity_death_removes_entity`: Entity at 0 health removed after cleanup
- `entity_same_hex_combat`: Two combatants on same hex, damage uses attacker facing
- `entity_no_friendly_fire`: Combatants of same owner don't damage each other

## Tick Order (updated)

```
tick(state):
  ...
  combat::resolve_combat()           // Legacy unit combat (unchanged)
  combat::entity_resolve_combat()    // NEW: entity facing-based combat
  rout_weakened_units()              // Legacy
  move_convoys()
  move_units()
  ...
  cleanup()                          // MODIFIED: adds entity_cleanup_dead
  ...
```

## Verification

```bash
cargo test -p simulate-everything-engine
cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --seeds 0-4 --ticks 500 --size 30x30 --agents spread,striker
```
