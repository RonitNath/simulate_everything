# V3 W3: Projectile System

Source spec: `docs/specs/v3-W-weapons.md` (Ranged Resolution, Projectile tick)
Wave: 2 (depends on W2 melee resolution, D2 impact pipeline, S1 spatial)

## Scope

Projectile spawning from ranged attack resolution, skill-interpolated aim
computation, substep integration with gravity, entity and ground collision
detection, inert entity lifecycle. No LOS (deferred for flat-trajectory weapons).

## Files

### `crates/engine/src/v3/projectile.rs` (modify)

Existing: Projectile component, arrow constructor, constants.

**Add:**
- `spawn_projectile(weapon, archer_pos, aim_pos, owner) -> (Vec3, Vec3, Projectile)` — compute launch velocity for ballistic intercept, return (position, velocity, projectile)
- `compute_aim_pos(target_pos, target_vel, distance, projectile_speed, combat_skill) -> Vec3` — skill-interpolated aim. `lerp(target_pos, predicted_pos, combat_skill)`
- `tick_projectile(pos, vel, arc, terrain_height_fn, entity_check_fn) -> ProjectileTick` — substep loop with gravity, entity collision, ground collision
- `ProjectileTick` enum: `InFlight { pos, vel }`, `EntityHit { pos, target, impact }`, `GroundHit { pos }`
- Ballistic launch angle computation: given distance, speed, gravity, height_diff → launch angle. Use the standard projectile formula.

### Dependencies
- `spatial::Vec3` — positions and velocities
- `damage::Impact` — constructed on entity hit
- `projectile::GRAVITY`, `PROJECTILE_SUBSTEPS`, `PROJECTILE_DT_SUB` — already defined in W1

## Key Design Decisions
- Substep at dt_sub=0.1 within tick dt=1.0 → 10 substeps per tick
- Entity collision: check sphere intersection at each substep position
- Ground collision: pos.z <= terrain_height_at(pos.xy)
- Friendly fire emerges naturally — collision checks all entities, not just enemies
- Inert lifecycle: caller removes Projectile + Mobile components on hit/ground. Entity persists.
- No LOS check for arc projectiles in V3.0

## Tests
- [ ] Bow ranged attack spawns projectile with arc physics
- [ ] Substep prevents clipping (arrow doesn't skip through entity at dt=1.0)
- [ ] Skill=0 aims at current position; skill=1 leads moving target
- [ ] Projectile hitting ground returns GroundHit
- [ ] Projectile hitting entity returns EntityHit with Impact
- [ ] Friendly fire: arrow hits any entity in path
