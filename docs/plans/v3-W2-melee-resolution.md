# V3 W2: Melee Attack Resolution

Source spec: `docs/specs/v3-W-weapons.md` (Attack Pipeline → Melee Resolution)
Wave: 1 (depends on D2 impact pipeline, S1 spatial — both landed)

## Scope

Per-tick attack state progression, melee resolution with range check,
Impact construction from weapon properties, stagger interaction with
commitment model, computed cooldown. No projectiles (that's W3).

## Key Interfaces from D Domain

- `damage::Impact` — constructed by W2 from weapon + attacker state
- `damage::DefenderState` — built from Equipment + Vitals + ArmorProperties
- `damage::BlockCapability` — from shield WeaponProperties
- `damage::resolve_impact()` → `ImpactResult`
- `damage::apply_impact_result()` — mutates Vitals + WoundList
- `vitals::Vitals` — stamina for cooldown computation, stagger state

## Files

### `crates/engine/src/v3/weapon.rs` (modify)

Add melee resolution functions. Data structs already exist from W1.

**New types:**
- `CooldownState { ticks_remaining: u16 }` — entity can't start new attack while > 0

**New functions:**
- `start_attack(target, weapon_key) -> AttackState` — already exists as `AttackState::new()`
- `tick_attack(state, weapon) -> AttackTick` — increment progress, check commitment, check windup complete
- `resolve_melee(weapon, attacker_pos, target_pos, attacker_facing, staggered, rng_seed) -> Option<Impact>` — range check, compute aim, construct Impact. Returns None if whiff.
- `compute_cooldown(weapon, stamina) -> u16` — `base_recovery * (weight / weight_ref) * (1/stamina)`
- `handle_stagger(state, weapon) -> StaggerResult` — uncommitted → Cancel, committed → Degrade(penalties)

**New enums:**
- `AttackTick { InProgress, Committed, Ready }` — result of tick_attack
- `StaggerResult { Cancelled, Degraded { accuracy_penalty: f32, force_penalty: f32 } }`

### Constants
- `WEIGHT_REF: f32 = 1.0` — reference weight for cooldown scaling
- `STAGGER_ACCURACY_PENALTY: f32 = 2.0` — dispersion multiplier when staggered
- `STAGGER_FORCE_PENALTY: f32 = 0.5` — swing speed multiplier when staggered
- `CROSS_SECTION_SLASH: f32`, `CROSS_SECTION_PIERCE: f32`, `CROSS_SECTION_CRUSH: f32` — per-damage-type defaults

## Tests

From spec verification:
- [ ] Iron sword melee produces Impact with DamageType::Slash
- [ ] Target out of reach during windup → whiff
- [ ] Stagger during uncommitted → cancel
- [ ] Stagger during committed → degrade (higher dispersion, lower force)
- [ ] Cooldown increases with weight, decreases with stamina
- [ ] Commitment transitions at correct fraction of windup
