# E2 Implementation Plan — Sim Tick Loop

Source: `docs/plans/v3-E-entity.md` (E2 scope)
Spec: `docs/specs/v3-entity-unification-2026-04-13.md`

## Deliverable

### `sim.rs` — Tick function orchestrating all systems

The tick function calls each subsystem in order. Systems that don't exist
yet (agents, economy, structures) are stubbed as no-ops with TODO comments.

```
tick(state, dt):
  1. rebuild_spatial_index(state)       — recompute hex membership for all entities
  2. compute_steering(state, dt)        — steering behaviors produce acceleration
  3. integrate_movement(state, dt)      — velocity + position updates, speed clamping
  4. resolve_melee(state)               — tick attack states, resolve ready attacks
  5. advance_projectiles(state, dt)     — substep integration, ground/entity hits
  6. apply_impacts(state)               — run impacts through D2 pipeline
  7. tick_vitals(state, dt)             — bleed, stamina recovery, stagger tick-down
  8. cleanup_dead(state)                — remove dead entities, eject equipment
  9. cleanup_projectiles(state)         — remove spent projectiles
  10. check_elimination(state)           — detect eliminated players
  11. state.game_time += dt; state.tick += 1
```

Key ordering constraints (from E.4):
- Spatial index first (everything queries it)
- Movement before combat (entities reach positions, then fight)
- Melee before projectile impacts (simultaneous melee + ranged)
- All damage before bleed (accumulated wounds bleed together)
- Cleanup last

## Dependencies (all met)

- E1: Entity, GameState, lifecycle ✓
- M1: steering, movement integration ✓
- D2: impact resolution pipeline ✓
- W2: melee attack resolution ✓
- W3: projectile system ✓

## Verification

```bash
cargo test -p simulate-everything-engine -- v3::sim
```
