# Spec: V3 Domain D — Damage Model

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2)
Sequencing: `docs/plans/v3-sequencing.md`
Dependencies: W1 (weapon/armor property structs for D2)

## Vision

Material-interaction damage physics. No health bars. A weapon hits a body zone,
the impact resolves against armor material properties, and the result is a wound
with a bleed rate — or a deflection. The same pipeline handles fists, swords,
arrows, sling stones, and cannon balls. Death comes from accumulated bleeding
over many ticks, not instant kills — creating "walking wounded" who fight at
degraded capability for long stretches, making attrition and medical logistics
matter.

## Use Cases

### UC1: Sword Strike Against Armored Defender

1. W domain constructs `Impact` from weapon properties + attacker state.
2. Hit location roll (hash-seeded from tick + attacker + defender IDs) → 0.0-1.0,
   biased by height difference. Mapped to `BodyZone` via weighted threshold lookup.
3. Block check: defender has stamina and facing permits block arc → stamina drained
   by `attack_force * block_efficiency`. If stamina sufficient, impact absorbed.
   If not, residual force passes through.
4. Armor lookup at hit zone: iron chain, hardness 0.7, thickness 0.3, coverage 0.95.
   Coverage roll: hash-seeded, 0.95 → 95% chance armor intercepts.
5. Angle of incidence: attack direction vs defender facing → surface normal per zone.
   Flanking hits land at better angles (closer to perpendicular on armor curves).
6. Penetration: `penetration_factor = KE * sharpness / cross_section` vs
   `resistance = hardness * thickness / sin(angle_from_surface)`.
   Sword slash has large cross-section → struggles against plate, adequate against chain.
7. Penetration succeeds → Wound { zone: Torso, severity: Laceration, bleed_rate: 0.005,
   attacker_id, damage_type: Slash, created_at: tick }.
8. Each subsequent tick: blood drains by sum of bleed rates (adjusted for clotting).

### UC2: Mace Blow Against Plate Armor

1. Same pipeline through steps 1-4.
2. Crush damage uses modified penetration: armor hardness ignored, only
   `mass * speed² vs thickness` matters. Bronze plate and steel plate of equal
   thickness absorb crush equally.
3. Even on deflection: force transmits through armor → stamina drain + stagger
   (proportional to force, 2-5 ticks). Defender cannot act during stagger.
4. On penetration: Fracture wound — low bleed (0.003/tick) but zone disabled.
   Leg fracture → immobile. Arm fracture → can't block/attack with that arm.

### UC3: Arrow Against Unarmored Target

1. Projectile entity reaches target (S domain handles trajectory + collision).
2. Pierce damage: tiny cross_section → high penetration factor. No armor →
   resistance is minimal (skin/cloth only).
3. Puncture wound at hit zone. Torso puncture → accelerated bleed (organs).
4. Arrow's `attacker_id` traces back to the archer for kill attribution.

### UC4: Prolonged Shield Wall Engagement

1. Two lines of infantry, shields up. Attacks land on shields → blocked.
2. Each block drains stamina: `cost = KE * block_efficiency`.
   Shield has high efficiency (low cost per block), but cost accumulates.
3. Stamina recovers between blocks: `stamina += recovery_rate * dt * (1.0 - wound_penalty)`.
   Unwounded defender recovers faster than wounded one.
4. After many exchanges, a defender's stamina depletes → can no longer block.
5. Next hit lands unblocked → wound → wound penalty slows stamina recovery →
   cascade failure. This is how shield walls break: attrition, not penetration.

### UC5: Downhill Cavalry Charge

1. Height difference biases hit_location toward defender's head (attacker swinging down).
2. Defender blocking uphill: `block_cost *= 1.0 + height_advantage * modifier` (tunable).
3. Cavalry weapon has high mass × speed → high KE → high penetration factor.
4. Even deflected blows stagger due to force magnitude.

## Body Zones

5 zones for V3.0: Head, Torso, LeftArm, RightArm, Legs.

### Zone Lookup

`fn zone_for_location(hit: f32) -> BodyZone` — weighted threshold lookup from a
const array of `(upper_bound, BodyZone)` pairs. Zones weighted by body proportion:
torso is the largest target, head the smallest.

Starting thresholds (tunable):

| Range | Zone | Proportion |
|-------|------|-----------|
| 0.00–0.08 | Head | 8% |
| 0.08–0.20 | LeftArm | 12% |
| 0.20–0.32 | RightArm | 12% |
| 0.32–0.72 | Torso | 40% |
| 0.72–1.00 | Legs | 28% |

The function signature is the stable API. Implementation can swap to continuous
distribution later without changing callers.

### Surface Normals

Derived from `attack_angle - entity.facing`, not fixed per zone. A torso hit
from the front has a different angle of incidence than from the flank. This means
facing matters for both block availability AND armor effectiveness — flanking
degrades armor protection, not just removes blocking.

Computation: `angle_from_surface = abs(attack_direction - defender_facing - zone_normal_offset)`,
where `zone_normal_offset` is a small per-zone constant (torso front ≈ 0, side arm
≈ π/2). The result feeds into `sin(angle_from_surface)` in the resistance formula.

## Wound System

```rust
struct Wound {
    zone: BodyZone,
    severity: Severity,
    bleed_rate: f32,        // per-tick drain on blood pool
    damage_type: DamageType, // Slash, Pierce, Crush — for future medical treatment
    attacker_id: EntityKey,  // kill attribution
    created_at: u64,         // tick — for clotting calculation
}
```

Storage: `SmallVec<[Wound; 4]>` per entity. Inline for typical combat (0-4 wounds),
heap-allocates for heavily wounded soldiers in prolonged fights.

### Severity

Computed from penetration depth: `depth = (penetration_factor - resistance) / resistance`.

| Severity | Depth Range | Bleed Rate | Notes |
|----------|------------|------------|-------|
| Scratch | 0.0–0.3 | 0.001/tick | Accumulates over many hits |
| Laceration | 0.3–0.7 | 0.005/tick | Reduces zone capability |
| Puncture | 0.7–1.5 | 0.01/tick | Deep bleed, organ risk at torso |
| Fracture | crush-specific | 0.003/tick | Low bleed, zone disabled |

Depth thresholds are tunable constants.

### Wound Effects (Cumulative)

Effects accumulate across all wounds in a zone — three scratches degrade
capability meaningfully. Per-zone effect is computed as sum of
`severity_weight(wound)` for all wounds in that zone.

| Zone | Effect | Mechanism |
|------|--------|-----------|
| Head | Accuracy penalty | Sum of wound severities → accuracy multiplier |
| Head | Unconsciousness risk | Puncture+ triggers roll against consciousness |
| LeftArm/RightArm | Block/attack speed reduction | Per-arm, affects that side only |
| Legs | Movement speed reduction | Fracture → immobile |
| Torso | Accelerated bleed | Organ damage multiplier on bleed rate |

### Clotting

Wounds clot over time. `effective_bleed = bleed_rate * clot_factor(age)` where
`age = current_tick - created_at`.

Clotting curve: scratches clot quickly (bleed → 0 within ~50 ticks), deep wounds
clot slowly or not at all. `clot_factor` decreases from 1.0 toward a floor that
depends on severity:

| Severity | Clot Floor | Clot Half-Life (ticks) |
|----------|-----------|----------------------|
| Scratch | 0.0 | ~20 |
| Laceration | 0.2 | ~60 |
| Puncture | 0.5 | ~100 |
| Fracture | 0.1 | ~40 |

Formula: `clot_factor = floor + (1.0 - floor) * 2^(-age / half_life)`.

Exact values are bench-tunable.

## Blood and Bleed

Blood pool: `f32`, initial 1.0. Each tick:
`blood -= sum(wound.effective_bleed_rate) * dt`.

| Blood Level | Effect |
|-------------|--------|
| 1.0–0.5 | Normal combat |
| < 0.5 | Combat degradation (slower, less accurate) |
| < 0.2 | Collapse (falls, can't act, continues bleeding) |
| <= 0.0 | Death |

Degradation below 0.5 is a linear multiplier: `effectiveness = blood / 0.5`
(so at 0.3 blood, effectiveness is 0.6×).

Thresholds are tunable constants.

## Stamina

Pool: `f32`, initial 1.0.

### Recovery

`stamina += recovery_rate * dt * (1.0 - wound_penalty)`

**recovery_rate**: bench-tunable constant. Starting value: 0.05/tick (full
recovery from empty in 20 ticks with no wounds).

**wound_penalty**: per-zone weighted sum of wound severities, clamped [0, 1].

| Zone | Penalty Weight | Rationale |
|------|---------------|-----------|
| Torso | 1.0 | Breathing, core stability |
| Legs | 0.5 | Pain, reduced circulation |
| Head | 0.3 | Disorientation |
| Arms | 0.2 | Minimal impact on recovery |

`wound_penalty = clamp(sum(zone_weight * sum(severity_weights_in_zone)), 0.0, 1.0)`

### Drain

Three sources of stamina drain:

| Source | Cost | Notes |
|--------|------|-------|
| Blocking | `KE * block_efficiency` | Per-block event |
| Sprinting | `sprint_drain * dt` | Per-tick while sprinting |
| Running | `run_drain * dt` | Per-tick while running, lower than sprint |

Walking has zero stamina cost.

### Block Mechanics

Block requires: stamina > 0, attack within block arc, entity not staggered.

| Equipment | Block Arc | Block Efficiency (lower = cheaper) |
|-----------|----------|-----------------------------------|
| Shield | ~180° | 0.3 |
| Sword parry | ~60° | 0.6 |
| Two-handed | ~40° | 0.8 |

Block efficiency is a property of the equipped weapon/shield (W domain defines).

### Stagger

Triggered when impact force exceeds a threshold despite block, or on any crush
deflection above a force threshold.

Duration: proportional to force, clamped [2, 5] ticks.
`stagger_ticks = clamp(base + (force - threshold) * scale, 2, 5)`

During stagger: entity cannot attack, block, or move. Vulnerable to follow-up.

## Movement Modes

Three explicit modes, agent-selected:

| Mode | Stamina Cost | Speed | Use Case |
|------|-------------|-------|----------|
| Walk | 0 | base_speed | Sustainable forever, default |
| Run | run_drain/tick | ~1.5× base | Force march, pursuit, sustained |
| Sprint | sprint_drain/tick | ~2× base | Tactical burst, closing distance |

Exact speed multipliers and drain rates are bench-tunable. Run is the middle
ground: costs stamina but sustainable for extended maneuvers. Sprint is expensive
and short-lived.

## Penetration Physics

### Core Formula

```
kinetic_energy = 0.5 * mass * speed²
penetration_factor = kinetic_energy * sharpness / cross_section
resistance = armor_hardness * armor_thickness / sin(angle_from_surface)

if penetration_factor > resistance → wound
else → deflection
```

**angle_from_surface**: 0 = glancing (parallel to armor surface), π/2 = perpendicular.
`sin(0) = 0` → infinite resistance (ricochet). `sin(π/2) = 1` → minimum resistance
(dead-on hit penetrates best). Matches real ballistic physics.

### Cross Section

Single `f32` per weapon. Not derived from damage type — each weapon has its own
cross-section tuned independently. A dagger slash and a greatsword slash have
different cross-sections. Allows per-weapon balance without damage-type constraints.

### Crush Damage (Modified Formula)

Crush ignores armor hardness. Modified resistance:
`resistance = armor_thickness / sin(angle_from_surface)`

Hardness dropped from the formula. Bronze plate and steel plate of equal thickness
absorb crush equally. This makes crush the counter to high-quality armor —
a mace doesn't care if it's bronze or steel, only how thick.

Crush can still be deflected (if thickness is sufficient), but even on deflection,
force transmits → stamina drain + potential stagger.

### Armor Coverage

Coverage is a fraction (0.0–1.0) per armor piece per zone. Chain: ~0.95 (small
gaps at links). Plate: ~0.85 (gaps at joints, visor).

Roll against coverage: hash-seeded from `(tick, attacker_id, defender_id)`.
If roll > coverage → attack finds a gap, resolves against skin/cloth only
(near-zero resistance).

### Determinism

All random rolls use hash-based determinism: `hash(tick, attacker_id, defender_id)`
seeds the roll. No global RNG dependency. Same inputs → same wound, regardless
of evaluation order. Impacts are testable in isolation — construct an Impact,
run against Body + Equipment, verify result.

## Height Modifiers

All height modifier constants are bench-tunable.

### Hit Location Bias

`hit_location += height_diff * bias_factor`, clamped [0, 1].

- Attacking uphill (negative height_diff): bias toward legs (higher values)
- Attacking downhill (positive height_diff): bias toward head (lower values)

### Block Cost

`block_cost *= 1.0 + height_advantage * height_block_modifier`

Defending against a downhill attacker costs more stamina (awkward angle).

## Scope

### V3.0 (ship this)

- D1 (Wave 0): BodyZone enum, Wound struct, Severity enum, blood/stamina pool
  primitives, zone lookup function, clotting formula
- D2 (Wave 1): Full 7-step impact resolution pipeline, penetration calculation,
  crush modification, deflection effects (stamina drain, stagger), wound
  application, bleed accumulation per tick
- Movement mode state (Walk/Run/Sprint) on entity, stamina drain per mode
- Cumulative wound effects per zone
- Hash-based deterministic rolls
- All tunable constants centralized, flagged for bench harness

### Deferred

- Continuous body zones (V3.1) — current zone lookup is swappable
- Medical treatment system (wound type matters for healing) — wound stores
  damage_type for future use
- Wound infection / gangrene over long timescales
- Cauterization / bandaging (active healing actions)
- Armor degradation from repeated hits
- Pain/morale system (wound count affects willingness to fight)

## Verification

- [ ] Construct Impact with known weapon/armor properties → verify wound severity
      matches expected penetration depth
- [ ] Crush impact against plate → verify hardness is ignored in resistance calc
- [ ] Glancing blow (angle near 0) → verify deflection (infinite resistance)
- [ ] Perpendicular hit → verify penetration (minimum resistance)
- [ ] Coverage roll miss → verify skin-only resistance
- [ ] Block with sufficient stamina → verify no wound, stamina drained correctly
- [ ] Block with insufficient stamina → verify residual force passes through
- [ ] Stagger from force > threshold → verify duration proportional to force, clamped [2,5]
- [ ] Wound clotting: scratch at age 50+ → verify effective bleed near 0
- [ ] Wound clotting: puncture at age 50 → verify effective bleed still significant
- [ ] Blood drain from multiple wounds → verify cumulative bleed × dt
- [ ] Blood < 0.5 → verify degraded effectiveness
- [ ] Blood < 0.2 → verify collapse state
- [ ] Blood ≤ 0.0 → verify death
- [ ] Stamina recovery with torso wound → verify heavy penalty
- [ ] Stamina recovery with arm wound → verify light penalty
- [ ] Cumulative wound effects: 3 scratches to arm → verify meaningful degradation
- [ ] Height bias: attacker downhill → verify head zone hit more likely
- [ ] Flanking attack → verify worse armor angle (closer to perpendicular)
- [ ] Walk/Run/Sprint → verify correct stamina drain per mode
- [ ] Determinism: same (tick, attacker, defender) → same roll output
- [ ] 10k entity benchmark: verify impact resolution is allocation-free (except wound push)

## Files

| File | Contents |
|------|----------|
| `crates/engine/src/v3/body.rs` | BodyZone enum, zone_for_location(), surface normal derivation |
| `crates/engine/src/v3/wound.rs` | Wound struct, Severity enum, clotting logic, wound effects |
| `crates/engine/src/v3/vitals.rs` | Blood pool, stamina pool, bleed accumulation, recovery, movement modes |
| `crates/engine/src/v3/damage.rs` | Impact struct, 7-step resolve_impact() pipeline, penetration calc |

## Constraints

- Impact resolution is allocation-free in the hot path. No Vec creation per
  impact. Wound push to SmallVec is the only allocation (and only when exceeding
  inline capacity).
- All formulas use `dt` for multi-resolution compatibility (tactical 1s, cinematic 10ms).
- Deterministic: hash-based rolls, no global RNG, same inputs → same wounds.
- Testable in isolation: construct Impact + Body + Equipment → verify Wound output.
- All tunable constants collected in a single location (mod.rs or constants block),
  clearly named, with doc comments noting they are bench-tunable.
