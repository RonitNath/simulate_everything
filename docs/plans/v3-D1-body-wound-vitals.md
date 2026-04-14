# Plan: D1 — Body, Wound, Vitals Primitives

Source spec: `docs/specs/v3-D-damage.md`
Wave: 0 (parallel with S1, S2, W1, R1)
Dependencies: W1 (BodyZone, DamageType already in armor.rs) ✅ landed

## Files

### `crates/engine/src/v3/body.rs`
- `zone_for_location(hit: f32) -> BodyZone` — weighted threshold lookup
  - Thresholds: Head 0.08, LeftArm 0.20, RightArm 0.32, Torso 0.72, Legs 1.00
- `surface_angle(attack_direction: f32, defender_facing: f32, zone: BodyZone) -> f32`
  - Returns angle from surface plane (0 = glancing, π/2 = perpendicular)
  - Per-zone normal offset: Torso 0, LeftArm π/2, RightArm -π/2, Head 0, Legs 0

### `crates/engine/src/v3/wound.rs`
- `Severity` enum: Scratch, Laceration, Puncture, Fracture
- `Wound` struct: zone, severity, bleed_rate, damage_type, attacker_id, created_at
- `severity_for_depth(depth: f32, is_crush: bool) -> (Severity, f32)` — returns severity + bleed rate
- `clot_factor(severity: Severity, age: u64) -> f32` — exponential decay with per-severity floor
- `effective_bleed(wound: &Wound, current_tick: u64) -> f32`
- `wound_severity_weight(severity: Severity) -> f32` — numeric weight for cumulative effects

### `crates/engine/src/v3/vitals.rs`
- `MovementMode` enum: Walk, Run, Sprint
- `Vitals` struct: blood (f32), stamina (f32), stagger_ticks (u16), movement_mode
- `Vitals::tick_bleed(wounds, dt)` — drain blood by sum of effective bleed rates
- `Vitals::tick_stamina_recovery(wounds, dt)` — recover stamina with wound penalty
- `Vitals::effectiveness() -> f32` — combat degradation from blood loss
- `Vitals::is_collapsed() -> bool`, `is_dead() -> bool`
- `Vitals::stamina_drain_for_mode(mode, dt) -> f32`
- Constants: all tunable, centralized at top of file

### `crates/engine/src/v3/mod.rs`
- Add `pub mod body; pub mod wound; pub mod vitals;`

## Tests (from spec verification checklist)
- zone_for_location covers full [0, 1) range
- Clotting: scratch at age 50+ → bleed near 0; puncture at age 50 → still significant
- Blood drain from multiple wounds → cumulative
- Blood < 0.5 → degraded effectiveness
- Blood < 0.2 → collapse
- Blood ≤ 0.0 → death
- Stamina recovery: torso wound → heavy penalty; arm wound → light penalty
- Cumulative wound effects: 3 scratches → meaningful degradation
- Walk/Run/Sprint → correct stamina drain per mode
- Surface angle: frontal → near π/2, flanking → near 0
