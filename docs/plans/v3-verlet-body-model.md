# V3 Verlet Body Model — Plan

## User stories

**As a spectator**, I want to see soldiers with visible body posture — arm
extension, guard stance, footwork — so I can read the combat at a glance
instead of inferring it from abstract attack-phase labels.

**As the combat engine**, I want sword hits to resolve against actual limb
geometry so that hit location, guard effectiveness, and weapon reach emerge
from physics rather than probability tables.

**As an agent designer**, I want reach to emerge from body mechanics (torso
twist + arm extension + weapon length) so agents that use their body well
outperform agents that don't, creating legible skill differences.

## Current state

Entities are single-point bodies (Entity.pos: Vec3) with a scalar facing
(Combatant.facing: f32). Hit location is a probability roll biased by height
difference. Weapon reach is a flat distance check
(`edge_distance_2d <= weapon.reach`). The renderer draws a fixed-shape glyph
at the entity center.

## Target state

Each combatant gets a skeletal body model — 16 Verlet point masses connected
by distance and angular constraints. The sword tip is a physics object on the
arm chain. Combat resolution becomes geometric: sword-tip sweep vs limb-segment
capsule hitboxes. Reach emerges from body configuration (stance width, arm
extension, torso twist). The protocol sends body-chunk positions; the renderer
draws shapes at those positions.

## Skeleton topology

```
                    [head]
                      |
                    [neck]
                   /      \
          [L shoulder]   [R shoulder]
              |               |
          [L elbow]      [R elbow]
              |               |
          [L hand]       [R hand]----[sword tip]
                   \      /
                [upper spine]
                      |
                [lower spine]
                   /      \
            [L hip]       [R hip]
              |               |
            [L knee]      [R knee]
              |               |
            [L foot]      [R foot]
```

16 points (15 body + 1 equipment). Each point has:
- Position (Vec3, relative to entity root)
- Previous position (for Verlet integration: velocity = pos - prev_pos)
- Mass (kg, for force response)
- Constraint attachment list (implicit from topology)

## Constraint types

### Distance constraints
Enforce limb segment lengths. Each constraint: (point_a, point_b, rest_length).

| Segment          | Points              | Rest length (m) |
|------------------|----------------------|------------------|
| Neck             | head ↔ neck          | 0.15             |
| L upper arm      | L shoulder ↔ L elbow | 0.30             |
| R upper arm      | R shoulder ↔ R elbow | 0.30             |
| L forearm        | L elbow ↔ L hand     | 0.28             |
| R forearm        | R elbow ↔ R hand     | 0.28             |
| Upper torso      | neck ↔ upper spine   | 0.25             |
| Lower torso      | upper spine ↔ lower spine | 0.25        |
| L shoulder mount | neck ↔ L shoulder    | 0.20             |
| R shoulder mount | neck ↔ R shoulder    | 0.20             |
| L thigh          | L hip ↔ L knee       | 0.45             |
| R thigh          | R hip ↔ R knee       | 0.45             |
| L shin           | L knee ↔ L foot      | 0.42             |
| R shin           | R knee ↔ R foot      | 0.42             |
| L hip mount      | lower spine ↔ L hip  | 0.15             |
| R hip mount      | lower spine ↔ R hip  | 0.15             |
| Weapon           | R hand ↔ sword tip   | weapon.reach     |

### Angular constraints
Enforce joint limits. Each constraint: (point_a, point_pivot, point_b, min_angle, max_angle).

| Joint       | Chain               | Range (deg)  | Notes                       |
|-------------|----------------------|--------------|-----------------------------|
| Elbow       | shoulder-elbow-hand  | 15–170       | Can't hyperextend           |
| Knee        | hip-knee-foot        | 10–165       | Can't hyperextend           |
| Shoulder    | neck-shoulder-elbow  | 0–180        | Full forward/up range       |
| Hip         | spine-hip-knee       | 10–150       | Limits kick/lunge extension |
| Spine twist | upper spine-lower spine (angular) | -60–60 | Torso rotation range  |

### Grounding constraint
Feet have a soft pin to ground level (z ≈ terrain height at foot position).
During stance, feet are quasi-static — only move during step transitions.
This prevents the body from floating or sinking.

## Verlet integration

Per physics substep (multiple per sim tick for stability):

```
for each point:
    velocity = point.pos - point.prev_pos
    point.prev_pos = point.pos
    point.pos += velocity * damping + acceleration * dt²

for N constraint iterations (3-5):
    for each distance constraint:
        delta = b.pos - a.pos
        dist = |delta|
        correction = (dist - rest_length) / dist * 0.5
        a.pos += delta * correction * (b.mass / total_mass)
        b.pos -= delta * correction * (a.mass / total_mass)

    for each angular constraint:
        // project points to satisfy min/max angle at pivot
```

Damping: 0.98 (slight energy loss prevents oscillation).
Substeps per tick: 4 (at 20 ticks/sec = 80 Hz physics, sufficient for
constraint stability with 16 points).

## Stance system

Stances are target configurations — desired positions for key points relative
to the entity root and facing. The body doesn't teleport to stance; instead,
forces are applied toward target positions and the Verlet solver relaxes there
naturally.

### Stance definition

```rust
struct Stance {
    name: &'static str,
    /// Target offsets from entity root, in body-local coordinates
    /// (x = right, y = forward/facing, z = up).
    targets: [(BodyPoint, Vec3); N],
    /// How strongly each point is pulled toward its target.
    /// 0.0 = free, 1.0 = rigid pin.
    stiffness: f32,
}
```

### Core stances

| Stance       | Sword hand target     | Feet                    | Notes                    |
|--------------|-----------------------|-------------------------|--------------------------|
| Neutral      | Low right, arm relaxed| Shoulder width          | Default idle             |
| High guard   | Above head, center    | Shoulder width          | Defends head, slow swing |
| Mid guard    | Center chest height   | Shoulder width, lead foot forward | Balanced        |
| Low guard    | Below waist, right    | Wide stance             | Defends legs, fast upswing |
| Lunge        | Extended forward      | Deep split, lead far forward | Max reach, slow recovery |
| Withdraw     | Retracted to chest    | Narrow, weight back     | Defensive retreat        |

Transitions between stances happen over 2-4 ticks (100-200ms), driven by
applying spring forces toward new target positions.

## Force transfer model (kinetic chain)

A sword swing is not just arm movement — it's a sequential activation from
feet through hips, torso, shoulder, arm, to sword tip. Each link adds velocity.

### Swing phases mapped to body mechanics

1. **Windup**: Weight shifts to rear foot. Torso rotates away from target
   (loading). Sword hand moves to start position (high for overhead, right
   for forehand, left for backhand).

2. **Commitment**: Sequential activation:
   - Rear foot pushes (ground reaction force)
   - Hips rotate toward target
   - Torso follows hips (spine twist)
   - Shoulder accelerates arm
   - Elbow extends
   - Wrist snaps (final velocity addition)

   Each link applies force to the next. The sword tip velocity is the sum of
   all link contributions. Peak tip velocity at full extension with good
   timing ≈ 10-15 m/s for a longsword.

3. **Recovery**: Deceleration. The kinetic chain unloads in reverse. Feet
   re-establish stable stance. A missed swing carries momentum past the
   target, rotating the body and exposing the back.

### Force model

```rust
struct KineticChainForce {
    /// Each link in the chain: joint → force magnitude → direction.
    /// Applied sequentially with timing offsets.
    links: [(BodyPoint, f32, f32); 6],  // foot, hip, spine, shoulder, elbow, wrist
    /// Timing: each link fires N ticks after the previous.
    link_delay_ticks: u16,
    /// Current active link index.
    active_link: usize,
}
```

Tip velocity calculation:
```
v_tip = v_hip_rotation × r_hip_to_shoulder
      + v_shoulder_rotation × r_shoulder_to_elbow
      + v_elbow_extension × r_elbow_to_hand
      + v_wrist_snap × r_hand_to_tip
```

A skilled fighter has better timing between links (higher peak velocity).
Combat skill affects the delay between link activations — tighter = faster
tip speed.

## Hitbox geometry

Each body segment is a capsule (swept sphere along the segment line).

| Segment     | Radius (m) | Points                 |
|-------------|------------|------------------------|
| Head        | 0.12       | head (sphere)          |
| Neck        | 0.06       | head ↔ neck            |
| Upper torso | 0.18       | neck ↔ upper spine     |
| Lower torso | 0.16       | upper spine ↔ lower spine |
| Upper arm   | 0.05       | shoulder ↔ elbow       |
| Forearm     | 0.04       | elbow ↔ hand           |
| Thigh       | 0.08       | hip ↔ knee             |
| Shin        | 0.06       | knee ↔ foot            |
| Sword blade | 0.02       | hand ↔ sword tip       |

### Hit detection

Per tick, the sword tip traces a path from prev_pos to pos (a line segment).
Test this segment against all defender body-segment capsules. The first
intersection is the hit. The zone is determined by which capsule was hit —
no probability roll needed.

```rust
fn sweep_test(
    sword_prev: Vec3, sword_pos: Vec3,   // swept sword tip
    seg_a: Vec3, seg_b: Vec3, radius: f32 // defender capsule
) -> Option<(f32, Vec3)>  // (t along sweep, hit point)
```

Guard effectiveness: the defender's sword/arm capsules are between the
attacker's sword path and the body. A proper guard geometrically blocks
because the attacker's sweep hits the defender's sword capsule first.

## Impact changes

Currently: `resolve_melee()` does a range check and computes kinetic energy
from weapon weight × base swing speed.

After: kinetic energy comes from actual sword-tip velocity (Verlet-derived).
Impact force = 0.5 × weapon_mass × tip_velocity². The hit zone is determined
by which capsule was intersected. Armor lookup uses the hit zone as before.

The 7-step damage pipeline (damage.rs) stays mostly intact:
1. ~~Hit location roll~~ → **Hit location from geometry** (step 1 changes)
2. Block check → **Geometric block** (sword-on-sword intersection)
3-7: Armor, angle of incidence, coverage, penetration, wound — **unchanged**

## Protocol extension

Add to SpectatorEntityInfo:

```rust
/// Body point positions relative to entity center, in world units.
/// Sent only when body model is active (combatants in close engagement).
/// 16 points × 3 floats × 4 bytes = 192 bytes per entity.
pub body_points: Option<Vec<[f32; 3]>>,
```

Delta encoding: only send body_points when they change (which is every tick
during combat, but not during idle/marching — then a compact stance ID
suffices).

Bandwidth optimization: during idle, send `stance_id: u8` instead of 16
points. Client reconstructs from stance template. During combat, send full
body points (192 bytes, delta-compressed to ~60-80 bytes for incremental
changes).

## Engine module structure

### New files

| File | Purpose |
|------|---------|
| `crates/engine/src/v3/body.rs` | BodyModel struct, Verlet points, constraints, stance definitions |
| `crates/engine/src/v3/body_physics.rs` | Integration, constraint solver, substep loop |
| `crates/engine/src/v3/kinetic_chain.rs` | Force transfer model, swing mechanics, tip velocity |
| `crates/engine/src/v3/hitbox.rs` | Capsule geometry, sweep tests, body-segment collision |

### Modified files

| File | Changes |
|------|---------|
| `state.rs` | Add `body: Option<BodyModel>` to Entity |
| `sim.rs` | Add body physics substep phase between movement and combat |
| `weapon.rs` | Replace range-check resolve with geometric sweep test |
| `damage.rs` | Step 1 (hit location) from geometry; step 2 (block) from geometry |
| `movement.rs` | Root position drives body root; foot placement from stance |
| `v3_protocol.rs` | Add body_points/stance_id to SpectatorEntityInfo |
| `v3_drill.rs` | Drill commands to set stance, trigger swing, inspect body state |

### Frontend (later, separate plan)

| File | Changes |
|------|---------|
| `entities.ts` | Draw shapes at body-point positions instead of fixed glyph |
| `v3types.ts` | Add body_points field to SpectatorEntityInfo |

## Implementation sequence

### Wave 1: Body model + Verlet solver (no combat integration)
- `body.rs`: BodyModel struct, point masses, constraint definitions
- `body_physics.rs`: Verlet integration, distance constraints, angular constraints
- `state.rs`: Add `body: Option<BodyModel>` to Entity
- `sim.rs`: Call body physics substep after movement, before combat
- Stance definitions (neutral, high/mid/low guard)
- Unit tests: constraint satisfaction, stance relaxation, energy conservation

### Wave 2: Kinetic chain + swing mechanics
- `kinetic_chain.rs`: Sequential force application, link timing, tip velocity
- Modify `weapon.rs`: AttackState drives kinetic chain instead of tick counter
- Swing phases (windup, commit, recovery) mapped to body mechanics
- Tip velocity derived from Verlet (replaces BASE_SWING_SPEED constant)
- Unit tests: tip velocity for various stances, timing vs skill

### Wave 3: Geometric hit detection
- `hitbox.rs`: Capsule-capsule sweep test, segment intersection
- Modify `damage.rs`: Hit zone from geometry (replace probability roll)
- Geometric block detection (sword-on-sword/arm interception)
- Guard stance effectiveness emerges from body configuration
- Unit tests: hit detection accuracy, block geometry, edge cases

### Wave 4: Protocol + rendering
- `v3_protocol.rs`: Send body_points, delta encoding
- `v3_drill.rs`: Drill commands for stance control, swing inspection
- Frontend: entities.ts draws at body points
- Visual verification via drill pad zoo

### Wave 5: Movement integration + footwork
- Foot placement drives stance width
- Lunge extends lead foot (increases reach temporarily)
- Dodge = lateral foot shift
- Leg wounds reduce stance stability (constraint solver can't maintain width)
- Wound effects on body mechanics (arm wound = slower swing, leg wound = narrow stance)

## Verification criteria

- [ ] Verlet solver conserves approximate energy (no explosion, no collapse)
- [ ] Distance constraints maintain limb proportions within 1% after 1000 ticks
- [ ] Angular constraints prevent hyperextension
- [ ] Stance transitions complete in 2-4 ticks with no oscillation
- [ ] Sword tip velocity matches expected range (8-15 m/s for longsword)
- [ ] Geometric hit detection matches manual calculation for known configurations
- [ ] Guard stance blocks attacks from guarded direction
- [ ] Lunge increases reach by ~0.3m vs neutral stance
- [ ] Leg wound visibly narrows stance and reduces lunge distance
- [ ] Protocol body_points delta encoding <100 bytes/entity/tick during combat
- [ ] 200 entities with body models at 20 ticks/sec < 5ms total body physics

## Open questions

1. **2D or 3D body points?** The engine is 3D (Vec3) but combat is
   predominantly planar. Body points need z for height (head vs legs) but
   most constraint solving is 2D. Consider: full 3D Verlet with gravity,
   or 2D planar + height offset per point.

2. **Substep count tuning.** 4 substeps is a starting guess. May need 6-8
   for stability with angular constraints. Profile.

3. **Non-combatant bodies.** Do farmers/builders get body models? Probably
   not initially — body model only activates when entity enters combat
   engagement. Reduces physics cost for large populations.

4. **Two-handed weapons.** Both hands constrained to weapon grip.
   Affects shoulder/arm configuration. Need a "two-hand grip" constraint.

5. **Shield as body part.** Shield-bearing arm has different constraint
   targets (shield covers body zone arc). Shield block = shield capsule
   intercepts attack sweep.
