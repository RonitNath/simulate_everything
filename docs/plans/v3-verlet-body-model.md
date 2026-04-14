# V3 Verlet Body Model — Plan

Status: **design-complete, ready for eng-lead decomposition**

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

**As the simulation platform**, I want body physics to be a fidelity tier
that activates and deactivates based on observation context, so the same
world can run at 1 game-hour ticks (aggregate statistics) or sub-second
ticks (individual body simulation) depending on where the player is looking.

## Simulation LOD model

The body model is part of a broader **simulation fidelity LOD system**:

### Strategic tier (1 game-hour ticks, zoomed out)
No body models. Combat resolves via aggregate statistics learned from prior
high-fidelity simulations. "100 swordsmen vs 80 spearmen on flat terrain"
→ lookup table derived from thousands of simulated engagements. Civilians
produce/consume at aggregate rates. Fast, statistically accurate.

### Tactical tier (1 game-second ticks, mid zoom)
Body models activate for entities in active physical tasks. Combat is
geometric. Civilian labor is modeled (tool swings, carrying). Movement
has momentum. This is the training-data tier — results feed the aggregate
tables used at strategic zoom.

### Close tier (sub-second ticks, zoomed in)
Full body physics with substeps. Smooth animation-quality constraint
solving. Every joint, every swing phase visible. This is what the player
watches at maximum zoom.

### Activation trigger
Body model activates when an entity is **performing a physical action in
an observed region at sufficient tick resolution**. This includes:
- Combat (melee engagement, blocking, dodging)
- Labor (tool use, carrying heavy loads, construction)
- Athletics (climbing, jumping, swimming)
- Injury response (limping, crawling, favoring a wounded arm)

When tick resolution is too coarse or the region is unobserved, body state
collapses to a stance ID or is absent entirely. Transition between tiers
must be seamless — an entity entering observation reconstructs its body
model from its current stance and activity state.

## Current engine state

Entities are single-point bodies (Entity.pos: Vec3) with a scalar facing
(Combatant.facing: f32). Hit location is a probability roll biased by height
difference. Weapon reach is a flat distance check
(`edge_distance_2d <= weapon.reach`). The renderer draws a fixed-shape glyph
at the entity center.

## Target state

Each entity performing a physical action gets a skeletal body model — 16
Verlet point masses in full 3D, connected by distance and angular constraints.
Weapons are physics objects on the arm chain. Shields are disc hitboxes on
the off-hand chain. Combat resolution is geometric: sword-tip sweep vs
limb-segment capsule/disc hitboxes. Reach emerges from body configuration.
The protocol sends body-chunk positions; the renderer draws shapes at those
positions.

## Resolved design decisions

### Full 3D Verlet (not 2D planar + height offset)
The z-axis is a real tactical dimension: overhead vs low sweeps, spear
thrusts angled up vs down, height advantage on terrain, crouching to shrink
profile, parabolic arrow trajectories hitting standing vs crouching targets.
2D planar would require special-casing all of these. Full 3D costs one extra
dimension in the constraint solver and a gravity + ground-contact pin on
feet — modest cost for correct vertical attack angles.

### Activity-triggered body models (not combat-only)
Body models activate for any physical action at sufficient tick resolution,
not just combat. A farmer swinging a scythe, a builder hammering, a person
climbing — all benefit from limb simulation. Activation is gated by
observation context (tick resolution + player zoom), not entity state.
At coarse ticks, even a farmer is an aggregate stat.

### Shield as disc hitbox with bash capability (option 3)
Shields are flat circular discs attached to the off-hand chain, with a
normal vector (facing direction). Blocking is geometric: attacker's sword
sweep intersects shield disc before body capsules. Deflection angle depends
on angle of incidence against disc normal. Shield bash uses the disc as an
offensive impact surface. The disc has mass, hardness, and radius. ~30 lines
of intersection math beyond capsule primitives.

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
              |                        (or shield disc)
          [shield disc]
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

16 body points + equipment points (sword tip, shield disc center). Each
point has:
- Position (Vec3, absolute world coordinates)
- Previous position (for Verlet integration: velocity = pos - prev_pos)
- Mass (kg, for force response and inertia)
- Constraint attachment list (implicit from topology)

## Constraint types

### Distance constraints
Enforce limb segment lengths. Each constraint: (point_a, point_b, rest_length).

| Segment          | Points                    | Rest length (m) |
|------------------|---------------------------|------------------|
| Neck             | head ↔ neck               | 0.15             |
| L upper arm      | L shoulder ↔ L elbow      | 0.30             |
| R upper arm      | R shoulder ↔ R elbow      | 0.30             |
| L forearm        | L elbow ↔ L hand          | 0.28             |
| R forearm        | R elbow ↔ R hand          | 0.28             |
| Upper torso      | neck ↔ upper spine        | 0.25             |
| Lower torso      | upper spine ↔ lower spine | 0.25             |
| L shoulder mount | neck ↔ L shoulder         | 0.20             |
| R shoulder mount | neck ↔ R shoulder         | 0.20             |
| L thigh          | L hip ↔ L knee            | 0.45             |
| R thigh          | R hip ↔ R knee            | 0.45             |
| L shin           | L knee ↔ L foot           | 0.42             |
| R shin           | R knee ↔ R foot           | 0.42             |
| L hip mount      | lower spine ↔ L hip       | 0.15             |
| R hip mount      | lower spine ↔ R hip       | 0.15             |
| Weapon           | R hand ↔ sword tip        | weapon.reach     |
| Shield mount     | L hand ↔ shield center    | 0.15             |

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
Gravity (9.81 m/s² downward) acts on all points; grounding prevents sinking.
This naturally gives weight to the simulation — arms fall when relaxed,
head bobs on impact.

## Verlet integration

Per physics substep (multiple per sim tick for stability):

```
for each point:
    velocity = point.pos - point.prev_pos
    point.prev_pos = point.pos
    point.pos += velocity * damping + (gravity + external_forces) * dt²

for N constraint iterations (3-5):
    for each distance constraint:
        delta = b.pos - a.pos
        dist = |delta|
        correction = (dist - rest_length) / dist * 0.5
        a.pos += delta * correction * (b.mass / total_mass)
        b.pos -= delta * correction * (a.mass / total_mass)

    for each angular constraint:
        // project points to satisfy min/max angle at pivot

    for each grounding constraint:
        if foot.pos.z < terrain_height:
            foot.pos.z = terrain_height
            foot.prev_pos.z = terrain_height  // kill vertical velocity
```

Damping: 0.98 (slight energy loss prevents oscillation).
Substeps per tick: 4 initial guess (at 20 ticks/sec = 80 Hz physics).
May need 6-8 for stability with angular constraints — profile and tune.

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

### Capsule hitboxes (limb segments)

Each body segment is a capsule (swept sphere along the segment line).

| Segment     | Radius (m) | Points                    |
|-------------|------------|---------------------------|
| Head        | 0.12       | head (sphere)             |
| Neck        | 0.06       | head ↔ neck               |
| Upper torso | 0.18       | neck ↔ upper spine        |
| Lower torso | 0.16       | upper spine ↔ lower spine |
| Upper arm   | 0.05       | shoulder ↔ elbow          |
| Forearm     | 0.04       | elbow ↔ hand              |
| Thigh       | 0.08       | hip ↔ knee                |
| Shin        | 0.06       | knee ↔ foot               |
| Sword blade | 0.02       | hand ↔ sword tip          |

### Disc hitbox (shield)

Shield is a flat circular disc on the off-hand chain:
- Center: L hand position (or offset from L hand along arm facing)
- Normal: perpendicular to forearm direction, angled by guard stance
- Radius: shield size (0.2m buckler to 0.5m kite shield)
- Thickness: for edge-on collision (0.02-0.05m)

Intersection test: sword sweep line vs disc plane, then check if
intersection point is within disc radius.

Deflection: impact angle against disc normal determines whether the
blow is absorbed (perpendicular), deflected (glancing), or slides past
(edge-on). Deflected blows may redirect into another body zone.

Shield bash: disc is thrust forward as an impact surface. Uses disc
mass × velocity for kinetic energy. Causes stagger, not wounds.

### Hit detection

Per tick, the sword tip (and blade segment) traces a path from prev_pos
to pos. Test this swept segment against all defender hitboxes (capsules +
disc). The first intersection (lowest t parameter) is the hit. The zone
is determined by which hitbox was intersected — no probability roll.

```rust
fn capsule_sweep_test(
    sword_prev: Vec3, sword_pos: Vec3,     // swept sword tip
    seg_a: Vec3, seg_b: Vec3, radius: f32  // defender capsule
) -> Option<(f32, Vec3)>                   // (t along sweep, hit point)

fn disc_sweep_test(
    sword_prev: Vec3, sword_pos: Vec3,     // swept sword tip
    center: Vec3, normal: Vec3, radius: f32 // shield disc
) -> Option<(f32, Vec3)>                   // (t along sweep, hit point)
```

Guard effectiveness: the defender's sword/arm capsules and shield disc are
between the attacker's sword path and the body. A proper guard geometrically
blocks because the attacker's sweep hits the defensive hitbox first.

## Impact changes

Currently: `resolve_melee()` does a range check and computes kinetic energy
from weapon weight × base swing speed.

After: kinetic energy comes from actual sword-tip velocity (Verlet-derived).
Impact force = 0.5 × weapon_mass × tip_velocity². The hit zone is determined
by which hitbox was intersected. Armor lookup uses the hit zone as before.

The 7-step damage pipeline (damage.rs) stays mostly intact:
1. ~~Hit location roll~~ → **Hit location from geometry** (step 1 changes)
2. ~~Block check (stamina, facing, arc)~~ → **Geometric block** (sword/shield intersection)
3-7: Armor, angle of incidence, coverage, penetration, wound — **unchanged**

## Protocol extension

Add to SpectatorEntityInfo:

```rust
/// Body point positions relative to entity center, in world units.
/// Sent when body model is active (entity performing physical action
/// in an observed region at sufficient tick resolution).
/// 16 points × 3 floats × 4 bytes = 192 bytes per entity.
pub body_points: Option<Vec<[f32; 3]>>,

/// Shield disc state (if equipped).
pub shield: Option<ShieldState>,  // { center: [f32;3], normal: [f32;3], radius: f32 }

/// Compact stance ID for idle entities without full body points.
/// Client reconstructs approximate body configuration from stance template.
pub stance_id: Option<u8>,
```

Delta encoding: only send body_points when they change (every tick during
combat/labor, but not during idle march — then stance_id suffices).

Bandwidth optimization: during idle, send `stance_id: u8` instead of 16
points. Client reconstructs from stance template. During combat, send full
body points (192 bytes, delta-compressed to ~60-80 bytes for incremental
changes).

## Engine module structure

### New files

| File | Purpose |
|------|---------|
| `crates/engine/src/v3/body.rs` | BodyModel struct, Verlet points, constraint definitions, stance catalog |
| `crates/engine/src/v3/body_physics.rs` | 3D Verlet integration, distance/angular/grounding constraints, substep loop |
| `crates/engine/src/v3/kinetic_chain.rs` | Force transfer model, swing mechanics, sequential link activation, tip velocity |
| `crates/engine/src/v3/hitbox.rs` | Capsule geometry, disc geometry, sweep tests, body-segment + shield collision |

### Modified files

| File | Changes |
|------|---------|
| `state.rs` | Add `body: Option<BodyModel>` to Entity |
| `sim.rs` | Add body physics substep phase between movement and combat; LOD tier gating |
| `weapon.rs` | Replace range-check resolve with geometric sweep test using body points |
| `damage.rs` | Step 1 (hit location) from geometry; step 2 (block) from shield/sword geometry |
| `movement.rs` | Root position drives body root; foot placement from stance; lunge/dodge via feet |
| `v3_protocol.rs` | Add body_points, shield state, stance_id to SpectatorEntityInfo + delta encoding |
| `v3_drill.rs` | Drill commands: set stance, trigger swing, inspect body state, set LOD tier |

### Frontend (separate plan, deferred until engine is verified)

| File | Changes |
|------|---------|
| `entities.ts` | Draw shapes at body-point positions instead of fixed glyph |
| `v3types.ts` | Add body_points, shield, stance_id to SpectatorEntityInfo |

## Implementation sequence

### Wave 1: Body model + Verlet solver (no combat integration)
- `body.rs`: BodyModel struct, 16 point masses, constraint definitions
- `body_physics.rs`: 3D Verlet integration, distance constraints, angular constraints, grounding, gravity
- `state.rs`: Add `body: Option<BodyModel>` to Entity
- `sim.rs`: Call body physics substep after movement, before combat
- Stance definitions (neutral, high/mid/low guard, lunge, withdraw)
- Unit tests: constraint satisfaction, stance relaxation, energy conservation, no explosion after 10k ticks

### Wave 2: Kinetic chain + swing mechanics
- `kinetic_chain.rs`: Sequential force application, link timing, tip velocity
- Modify `weapon.rs`: AttackState drives kinetic chain instead of tick counter
- Swing phases (windup, commit, recovery) mapped to body mechanics
- Tip velocity derived from Verlet (replaces BASE_SWING_SPEED constant)
- Two-handed weapon support (both hands constrained to weapon grip)
- Unit tests: tip velocity for various stances, timing vs skill, two-hand vs one-hand

### Wave 3: Geometric hit detection + shield
- `hitbox.rs`: Capsule sweep test, disc sweep test, segment intersection
- Modify `damage.rs`: Hit zone from geometry (replace probability roll)
- Geometric block detection (sword-on-sword/arm interception, shield disc interception)
- Shield bash as offensive disc impact
- Guard stance effectiveness emerges from body configuration
- Unit tests: hit detection accuracy, block geometry, shield deflection angles, edge cases

### Wave 4: Protocol + rendering
- `v3_protocol.rs`: Send body_points + shield state, delta encoding, stance_id optimization
- `v3_drill.rs`: Drill commands for stance control, swing inspection, LOD tier control
- Frontend: entities.ts draws shapes at body-point positions
- Visual verification via drill pad zoo with all stances + swing phases

### Wave 5: Movement integration + footwork + civilian actions
- Foot placement drives stance width
- Lunge extends lead foot (increases reach temporarily)
- Dodge = lateral foot shift
- Leg wounds reduce stance stability (constraint solver can't maintain width)
- Wound effects on body mechanics (arm wound = slower swing, leg wound = narrow stance)
- Civilian physical actions: tool swings, carrying, climbing (reuse kinetic chain with different targets)
- LOD tier transitions: smooth activation/deactivation of body model based on observation context

## Verification criteria

- [ ] Verlet solver conserves approximate energy (no explosion, no collapse)
- [ ] Distance constraints maintain limb proportions within 1% after 1000 ticks
- [ ] Angular constraints prevent hyperextension in all joint configurations
- [ ] Gravity grounds idle entities; arms hang naturally when relaxed
- [ ] Stance transitions complete in 2-4 ticks with no oscillation
- [ ] Sword tip velocity matches expected range (8-15 m/s for longsword swing)
- [ ] Geometric hit detection matches manual calculation for known configurations
- [ ] Guard stance blocks attacks from guarded direction via geometry
- [ ] Shield disc deflects attacks at correct angles; edge-on hits pass through
- [ ] Shield bash causes stagger proportional to disc mass × velocity
- [ ] Lunge increases reach by ~0.3m vs neutral stance
- [ ] Leg wound visibly narrows stance and reduces lunge distance
- [ ] Vertical attack angles work: overhead hit → head zone, low sweep → legs zone
- [ ] Height advantage produces correct downward thrust angles
- [ ] Protocol body_points delta encoding <100 bytes/entity/tick during combat
- [ ] 200 entities with body models at 20 ticks/sec < 5ms total body physics
- [ ] Body model activates/deactivates cleanly on LOD tier transition
- [ ] Stance_id reconstruction matches full body_points within acceptable tolerance
