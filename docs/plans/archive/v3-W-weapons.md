# V3 Domain: W — Weapons and Attack Pipeline

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Attack Pipeline section)
Sequencing: `docs/plans/v3-sequencing.md`

## Purpose

Define weapons, armor, and projectiles as entities with material properties.
Implement the unified attack pipeline where melee and ranged are the same system
with different parameters. A sword swing and an arrow flight both produce an
Impact that enters the damage pipeline (domain D).

## Design Questions

### W.1 Weapon Properties

- The spec defines WeaponProperties with damage_type, sharpness, hardness, weight,
  reach, block_arc, block_efficiency, projectile_speed, projectile_arc, accuracy_base.
  Are these all floats, or should some be enums with float parameters?
- Weapon degradation: sharpness decreases with use. Per-hit? Per-tick of combat?
  Rate of degradation? Is this V3.0 or V3.1? The spec says V3.1 but the sharpness
  field exists from V3.0.
- Starting weapon set for V3.0: what's the minimum viable set?
  - Melee: Sword (slash, medium reach), Spear (pierce, long reach), Mace (crush, short reach)
  - Ranged: Bow (pierce, arc, long range), Sling (crush, arc, medium range),
    Crossbow (pierce, flat, medium range, high penetration)
  - Shield: not a weapon per se but shares block_arc and block_efficiency.
    Is a shield a weapon entity with damage_type=None? Or a separate ArmorProperties
    entity equipped to a special "shield hand" slot?
- Two-handed weapons: a pike or greataxe prevents using a shield. Is this enforced
  by equipment slots (two-handed weapon fills both weapon + shield slots)? Or by a
  `hands_required: u8` field?

### W.2 Armor Properties

- ArmorProperties: material, hardness, thickness, coverage, weight. The spec defines
  MaterialType as Cloth, Leather, Chain, Plate. Are there gradations within each?
  Bronze plate vs iron plate vs steel plate? Or is that captured by hardness alone?
- Starting armor set for V3.0:
  - Cloth tunic (no protection, light)
  - Leather cuirass (low hardness, high coverage, light)
  - Chain mail hauberk (medium hardness, very high coverage, medium weight)
  - Iron breastplate (high hardness, medium coverage for joints, heavy)
- Per-zone armor: the spec has `armor_slots: [Option<EntityKey>; ZONE_COUNT]` on
  Equipment. Can one armor entity cover multiple zones (a hauberk covers torso +
  arms)? Or does each zone need its own entity? Multi-zone coverage seems cleaner
  — one chain mail entity covers torso + both arms.
- Armor as entity: an iron breastplate on the ground is an Entity with
  ArmorProperties. When equipped, it's contained_in the wearer? Or referenced by
  EntityKey in the Equipment component? Containment seems right — the armor is
  physically on the person.

### W.3 Attack Execution (Melee)

- Attack timing: the spec defines `windup` (ticks before damage). A sword swing
  starts, takes windup ticks, then resolves. During windup, can the attack be
  interrupted (attacker takes a hit, staggers)? Can the attacker cancel?
- Attack frequency: how often can an entity attack? Is there a cooldown after each
  attack? Derived from weapon weight and entity stamina?
- Attack selection: the tactical layer issues `Attack { attacker, target }`. The
  attack profile comes from the attacker's equipped weapon. Does the attacker
  choose aim_center (which zone to target)? Or is that determined by the tactical
  layer command? Simpler: tactical says "attack entity X," the aim_center is
  computed from height difference and attacker skill.
- Melee range check: the attack resolves only if the attacker's continuous position
  is within `weapon.reach` of the target's continuous position. Check at windup
  start or at resolution time? Resolution time makes more sense — the target might
  move away during windup.

### W.4 Projectile Entities

- Projectile lifecycle: spawned when a ranged attack fires, moves each tick,
  removed on impact or when it hits ground with no target.
- Projectile physics: `pos += vel * dt`, if arc: `vel.z -= GRAVITY * dt`. What's
  GRAVITY in game units? If 1 hex ≈ 150m and 1 tick = 1 second, then gravity =
  9.8 m/s². But positions are in world units (presumably meters). So GRAVITY = 9.8.
- Aim computation: the archer targets an enemy's continuous position. The tactical
  layer computes launch angle for the projectile to arrive at the target's
  **predicted** position (current pos + vel * flight_time). This is a ballistic
  intercept problem. How sophisticated should V3.0 be? Simple: aim at current
  position (arrows miss moving targets more). Advanced: predict target position
  (archers "lead" their targets). Combat_skill could interpolate between simple
  and advanced.
- Projectile count: at 200 archers firing every 3 seconds, that's ~70 projectile
  entities in flight at any given time. At 500 archers, ~170. Manageable? Yes at
  10k total entities. But do we need to worry about projectile cleanup (arrows on
  the ground after impact — remove immediately, or leave as inert entities)?
- Arrow salvage: in reality, some arrows can be recovered. V3.0: remove on impact
  or miss. Future: leave recoverable arrows as inert resource entities.

### W.5 Line of Sight

- Flat trajectory projectiles (crossbow bolts) need LOS from source to target.
  LOS check: ray-march through hexes between source and target. At each hex, check
  if terrain height or entity height occludes the ray.
- Arc trajectory projectiles need LOS only for the initial arc check: can the
  arrow clear nearby obstacles? At long range, arc goes high enough to clear
  anything. At short range with a low angle, it might hit a friendly or obstacle.
- Height advantage: an elevated archer can shoot further (less arc needed) and
  has better LOS. The ballistic intercept computation accounts for this via the
  z-component of the launch position.
- Friendly fire: any entity in the projectile's path. For arc projectiles, the
  path is a parabola — compute z at each x,y point along the trajectory and check
  if any entity's (x, y, z) is within the projectile's impact radius at that
  point. This is the expensive part. Optimization: only check entities in hexes
  the trajectory passes through.

### W.6 Equipment Slots and Entity Lifecycle

- Equipment as contained entities: weapon and armor entities are contained_in the
  wearer. When the wearer dies, equipment entities are ejected (contained_in = None,
  pos = wearer's pos). They become lootable / recoverable.
- Equipment production: operations layer directs workshops (structure entities) to
  produce equipment. A workshop contains workers + raw material resources → produces
  weapon/armor entities over time. What's the production model? Tick-based progress
  like structure construction?
- Equipment distribution: operations layer assigns equipment to entities via
  `EquipEntity` command. The equipment entity moves from stockpile (contained in
  structure) to person (contained in person). Requires the person to be at the
  structure's hex.

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| W1 | 0 | — | Weapon/Armor property structs, DamageType, MaterialType |
| W2 | 1 | D2, S1 | Melee attack resolution (windup, range check, → impact pipeline) |
| W3 | 2 | W2, M1, S1 | Projectile entities, arc/flat physics, LOS, impact detection |

## Key Files (Expected)

- `crates/engine/src/v3/weapon.rs` — WeaponProperties, attack profiles, melee resolution
- `crates/engine/src/v3/armor.rs` — ArmorProperties, material types, coverage
- `crates/engine/src/v3/projectile.rs` — projectile entity lifecycle, ballistics, LOS
- `crates/engine/src/v3/equipment.rs` — equipment slots, equip/unequip, production

## Constraints

- Projectile physics must be deterministic (seeded RNG for accuracy dispersion).
- Melee and ranged attacks must produce the same Impact struct that feeds into
  domain D's pipeline. No separate code paths for melee vs ranged damage.
- At 500 entities in combat, attack resolution (including projectile spawning and
  impact detection) must complete in < 1ms per tick.
