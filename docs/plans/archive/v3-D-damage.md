# V3 Domain: D — Damage Model

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Damage Model section)
Sequencing: `docs/plans/v3-sequencing.md`

## Purpose

Implement material-interaction damage physics. No health bars. A weapon hits a
body zone, the impact resolves against armor material properties, and the result
is a wound with a bleed rate — or a deflection. The same pipeline handles fists,
swords, arrows, sling stones, and (future) cannon balls.

## Design Questions

### D.1 Body Zones

- 5 zones for V3.0: Head, Torso, LeftArm, RightArm, Legs. The hit_location value
  (0.0-1.0) maps to zones via threshold lookup. How should this be stored?
  A const array of (threshold, zone) pairs? A match on ranges?
- Zone surface normals: the spec mentions `surface_normal: f32` per zone for
  ricochet calculation. How is this computed? Is it relative to the entity's
  facing? A torso hit from the front has a different surface normal than from the
  side. Is the surface normal derived from `attack_angle - entity.facing` or
  stored as a fixed per-zone value?
- Extensibility: the spec says "easily extensible to continuous." The zone lookup
  should be a function `fn zone_for_location(hit: f32) -> ZoneId` that can swap
  implementations without changing callers. What's the trait boundary?

### D.2 Wound System

- Wound struct: zone, severity, bleed_rate. Should wounds also track: time since
  inflicted (for healing)? Weapon type that caused them (for medical treatment
  differences in future)? Attacker entity (for kill attribution)?
- Max wounds per entity: unbounded Vec, or capped SmallVec? A soldier in a long
  fight might accumulate 10-20 minor wounds. Memory concern at 10k entities?
- Wound severity computation: `penetration_depth = (penetration_factor - resistance) / resistance`?
  How does depth map to Severity enum? Linear thresholds?
- Wound effects on capability: the spec lists per-zone effects (head → accuracy,
  arm → block/attack speed, leg → movement speed). Are these computed from wound
  count and severity per zone, or from worst wound per zone? Cumulative seems
  more realistic.

### D.3 Blood and Bleed

- Blood pool: initial value 1.0, drains by sum of bleed rates per tick. This is
  simple but: do wounds clot over time (bleed_rate decreases)? That adds realism
  (scratches stop bleeding, deep wounds don't) but complexity.
- Death threshold: blood <= 0 = dead. Collapse at < 0.2. Combat degradation
  starts at < 0.5. Are these the right thresholds? Should they be constants in
  mod.rs?
- Blood loss rate during dt: `blood -= total_bleed * dt`. At dt=1.0 (tactical),
  a single laceration (0.005/tick) takes 100 ticks to drain from 1.0 to 0.5.
  That's ~100 seconds of game time for one wound to significantly degrade combat.
  Is that the right pace? Too slow? Too fast? Needs tuning via the bench harness.

### D.4 Stamina and Blocking

- Stamina pool: 1.0 full, drains from blocking and sprinting, recovers slowly.
  Recovery formula: `stamina += recovery_rate * dt * (1.0 - wound_penalty)`.
  What's recovery_rate? What's the wound_penalty formula?
- Block cost: `cost = attack_force * block_efficiency` where attack_force =
  weapon_weight * swing_speed. How does block_efficiency relate to weapon type?
  Shield: wide arc, high efficiency (low cost per block). Sword parry: narrow arc,
  medium efficiency. Two-handed weapon: narrow arc, low efficiency (high cost).
- Stamina and movement: the spec mentions sprinting costs stamina. Is sprinting a
  distinct state (toggle) or is it "any movement above walk speed costs stamina"?
- Stagger: when force exceeds a threshold despite block or on a crush deflection.
  Duration: 2-5 ticks. During stagger, entity can't act (no attack, no block, no
  move). Is the stagger duration proportional to force, or fixed?

### D.5 Penetration Physics

- The core formula: `penetration_factor > resistance` where:
  - `penetration_factor = kinetic_energy * sharpness / cross_section`
  - `resistance = armor_hardness * armor_thickness / sin(angle)`
  - `kinetic_energy = 0.5 * mass * speed²`
- How is cross_section determined per damage type? Slash: blade edge length × depth
  of cut? Pierce: point area? Crush: weapon face area? Or simplified to a single
  float per weapon?
- sin(angle) at very shallow angles → very high resistance (glancing blow). At
  angle = 0 (dead perpendicular), sin(0) → 0 which would mean zero resistance.
  Need to clarify: is angle measured from the surface (0 = parallel, PI/2 =
  perpendicular) or from the normal (0 = perpendicular, PI/2 = parallel)? The
  formula needs sin of the angle from the surface for ricochet behavior.
- Armor coverage: roll against coverage fraction. If the roll exceeds coverage,
  the attack finds a gap (no armor, only cloth/skin). How is this roll seeded?
  Deterministic from tick + attacker + defender IDs?

### D.6 Height Modifiers

- Attacking uphill: hit_location biases toward legs. How much bias? Additive
  offset on the 0.0-1.0 roll? `hit_location += height_diff * bias_factor`
  clamped to [0, 1]?
- Attacking downhill: hit_location biases toward head. Same formula with
  negative height_diff.
- Block angle from downhill attacker: increased stamina cost. Multiplicative
  factor? `block_cost *= 1.0 + height_advantage * 0.3`?

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| D1 | 0 | — | Body zone types, Wound struct, blood/stamina primitives |
| D2 | 1 | D1, W1 | Full 7-step impact resolution pipeline |

## Key Files (Expected)

- `crates/engine/src/v3/body.rs` — BodyZone, Wound, Severity, zone lookup
- `crates/engine/src/v3/damage.rs` — impact resolution pipeline, penetration calc
- `crates/engine/src/v3/vitals.rs` — blood, stamina, bleed accumulation, recovery

## Constraints

- Impact resolution must be allocation-free in the hot path. No Vec creation per
  impact. Wound appending is the only allocation (SmallVec push).
- All formulas use dt for multi-resolution compatibility.
- Deterministic: same inputs → same wounds. Random rolls seeded from game state.
- The pipeline must be testable in isolation: construct an Impact, run it against
  a Body + Equipment, verify the resulting Wound.
