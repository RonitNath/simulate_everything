# Stream A: Verlet Body Model Implementation

Status: **ready for implementation**
Depends on: Phase 0 (protocol crate for body point types)
Design spec: `docs/plans/v3-verlet-body-model.md`
Linear: reference IA issue if one exists

## Goal

Add a 16-point 3D Verlet skeletal body model to the V3 engine. Sword
reach, guard effectiveness, and hit location emerge from physics geometry
rather than probability tables and flat distance checks.

## Waves

### A1: Body model + Verlet solver

**Files created:**
- `crates/engine/src/v3/body.rs` — `BodyModel` struct, 16 `BodyPoint`s,
  `BodyPointId` enum, constraint definitions, stance catalog
- `crates/engine/src/v3/body_physics.rs` — 3D Verlet integration,
  distance constraints, angular constraints, grounding, gravity, substep loop

**Files modified:**
- `crates/engine/src/v3/state.rs` — Add `body: Option<BodyModel>` to `Entity`
- `crates/engine/src/v3/sim.rs` — Add body physics substep phase between
  movement and combat
- `crates/engine/src/v3/mod.rs` — Declare new modules

**Skeleton topology (16 points):**
```
head, neck, L/R shoulder, L/R elbow, L/R hand,
upper spine, lower spine, L/R hip, L/R knee, L/R foot,
+ sword tip (equipment point on weapon hand)
```

**Constraint types:**
- Distance constraints: 17 segments (see design spec for rest lengths)
- Angular constraints: 5 joints with min/max angle limits
- Grounding: feet pinned to terrain height, gravity on all points

**Stance system:**
- `Stance` struct: target offsets per body point + stiffness
- Core stances: neutral, high/mid/low guard, lunge, withdraw
- Transitions: spring forces toward target positions, solver relaxes
  naturally over 2-4 ticks

**Substep config:**
- 4 substeps per tick initially (80 Hz at 20 ticks/sec)
- 3-5 constraint iterations per substep
- Damping: 0.98
- Profile and tune — may need 6-8 substeps for angular constraint stability

**Body model activation:**
- Body model activates when entity is performing a physical action at
  sufficient tick resolution (combat, labor, athletics, injury response)
- Idle entities: stance ID only, no physics
- Activation reconstructs body from stance template

**Tests:**
- Constraint satisfaction: limb proportions within 1% after 1000 ticks
- No explosion: energy doesn't diverge over 10k ticks
- Angular constraints prevent hyperextension
- Gravity grounds idle entities, arms hang naturally
- Stance transitions complete in 2-4 ticks without oscillation
- Activation/deactivation preserves approximate body configuration

### A2: Kinetic chain + swing mechanics

**Files created:**
- `crates/engine/src/v3/kinetic_chain.rs` — Sequential force application,
  link timing, tip velocity calculation

**Files modified:**
- `crates/engine/src/v3/weapon.rs` — `AttackState` drives kinetic chain
  instead of tick counter. Tip velocity from Verlet replaces `BASE_SWING_SPEED`.

**Kinetic chain links (sequential activation):**
1. Rear foot push (ground reaction force)
2. Hip rotation toward target
3. Torso follows hips (spine twist)
4. Shoulder accelerates arm
5. Elbow extends
6. Wrist snap

Each link applies force to the next. Combat skill affects link timing
(tighter = higher peak tip velocity). Expected tip velocity: 8-15 m/s
for longsword.

**Two-handed weapons:** Both hands constrained to weapon grip. Different
shoulder/arm force transfer path.

**Tests:**
- Tip velocity in expected range for various stances and skill levels
- Two-handed vs one-handed velocity difference
- Missed swing carries momentum past target (body rotates, back exposed)
- Recovery time proportional to swing momentum
- Skill 0.2 vs skill 0.8 produces measurably different tip speeds

### A3: Geometric hit detection + shield

**Files created:**
- `crates/engine/src/v3/hitbox.rs` — Capsule sweep test, disc sweep test,
  segment-segment intersection

**Files modified:**
- `crates/engine/src/v3/damage.rs` — Step 1 (hit location) from geometry
  instead of probability roll. Step 2 (block) from geometric interception.

**Hitbox primitives:**
- **Capsule** (limb segments): swept sphere along segment line. 9 body
  segment capsules per entity (head, neck, upper/lower torso, upper arm,
  forearm, thigh, shin) + sword blade capsule.
- **Disc** (shield): flat circular disc on off-hand chain with normal vector
  and radius. ~30 lines of intersection math beyond capsule.

**Hit detection:**
- Sword tip traces prev_pos → pos each tick (line segment)
- Test against all defender capsules + disc
- First intersection (lowest t parameter) = hit
- Zone determined by which hitbox was intersected — no probability roll

**Guard/block:**
- Defender's sword/arm capsules and shield disc physically between attacker
  and body
- Proper guard geometrically blocks because attacker's sweep hits defensive
  hitbox first
- Shield deflection angle from angle of incidence against disc normal

**Shield bash:**
- Disc thrust forward as impact surface
- Kinetic energy from disc mass × velocity
- Causes stagger, not wounds

**Changes to damage pipeline:**
- Step 1: ~~probability roll~~ → geometry determines zone
- Step 2: ~~stamina/facing/arc check~~ → geometric interception
- Steps 3-7: unchanged (armor, angle of incidence, coverage, penetration, wound)

**Tests:**
- Known configurations: sword sweep hits expected body zone
- Guard stance blocks from guarded direction
- Shield disc deflects at correct angles; edge-on hits pass through
- Shield bash causes stagger proportional to mass × velocity
- Overhead swing → head zone; low sweep → legs zone
- Height advantage produces correct downward thrust angles

### A4: Footwork + civilian actions

**Files modified:**
- `crates/engine/src/v3/movement.rs` — Foot placement from stance, lunge
  as lead-foot extension, dodge as lateral foot shift
- `crates/engine/src/v3/body.rs` — Civilian action stances (tool swing,
  carrying, climbing)

**Footwork:**
- Stance width from foot placement (L foot, R foot positions)
- Lunge: extend lead foot forward, increases reach ~0.3m
- Dodge: shift hip/foot points laterally
- Leg wounds reduce stance stability: constraint solver can't maintain width
  → narrower stance, shorter lunge, slower recovery

**Civilian body actions (reuse kinetic chain with different targets):**
- Scythe swing (farmer): similar chain to sword, different stance/grip
- Hammer (builder): overhead strike pattern
- Carry heavy load: constrained arm positions, reduced mobility
- Climbing: sequential hand/foot target repositioning

**Wound effects on body mechanics:**
- Arm wound: slower kinetic chain activation → reduced tip speed
- Leg wound: narrower stance, reduced lunge distance
- Torso wound: reduced spine twist range → less rotational force transfer

**Tests:**
- Lunge increases reach by ~0.3m vs neutral stance
- Leg wound measurably narrows stance
- Leg wound reduces lunge distance
- Arm wound reduces tip velocity
- Civilian tool swing produces correct force transfer

## Verification criteria (full stream)

- [ ] Verlet solver conserves approximate energy (no explosion, no collapse)
- [ ] Distance constraints maintain limb proportions within 1% after 1000 ticks
- [ ] Angular constraints prevent hyperextension in all joint configurations
- [ ] Gravity grounds idle entities; arms hang naturally when relaxed
- [ ] Stance transitions complete in 2-4 ticks with no oscillation
- [ ] Sword tip velocity 8-15 m/s for longsword swing
- [ ] Geometric hit detection matches manual calculation for known configs
- [ ] Guard stance blocks attacks from guarded direction via geometry
- [ ] Shield disc deflects at correct angles
- [ ] Shield bash causes stagger proportional to mass × velocity
- [ ] Lunge increases reach by ~0.3m vs neutral stance
- [ ] Leg wound narrows stance and reduces lunge distance
- [ ] Vertical attack angles work: overhead → head, low sweep → legs
- [ ] Height advantage produces correct downward thrust angles
- [ ] 200 entities with body models at 20 ticks/sec < 5ms total body physics
- [ ] Body model activates/deactivates cleanly on LOD tier transition
