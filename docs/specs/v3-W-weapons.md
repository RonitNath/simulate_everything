# Spec: V3 Domain W — Weapons and Attack Pipeline

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2)
Sequencing: `docs/plans/v3-sequencing.md`
Dependencies: D (damage pipeline), S (spatial), M (movement)

## Vision

Weapons, armor, and projectiles are entities with material properties. A sword
swing and an arrow flight both produce the same Impact struct that enters the
damage pipeline. There is no separate code path for melee vs ranged — the attack
pipeline is unified, parameterized by weapon properties.

## Use Cases

### UC1: Melee Attack (Swordsman vs Unarmored)

1. Tactical layer issues `Attack { attacker: swordsman, target: farmer }`
2. Attack system checks: swordsman has equipped weapon (iron sword), target is
   within weapon.reach of swordsman's continuous position — **not yet**, starts windup
3. Attack state set: `progress: 0, committed: false, target, weapon_ref`
4. Each tick: progress increments. At commitment threshold (fraction of windup),
   `committed = true`
5. At windup completion: range check against target's **current** position
   - If target moved out of reach: attack whiffs, cooldown begins
   - If in range: construct Impact from weapon properties + attacker state
6. Impact enters D2's 7-step pipeline → wound on target
7. Cooldown computed: `base_recovery * (weight / weight_ref) * (1.0 / stamina)`
8. Attacker cannot attack again until cooldown expires

### UC2: Ranged Attack (Archer vs Moving Target)

1. Tactical layer issues `Attack { attacker: archer, target: spearman }`
2. Attack system checks: archer has equipped weapon (bow, hands_required=2, no
   shield allowed), target is within max range
3. Windup begins (draw bow). Uncommitted → committed transition during draw
4. At windup completion: compute aim direction
   - `aim_pos = lerp(target.pos, target.pos + target.vel * flight_time, combat_skill)`
   - Low skill aims at current position, high skill leads the target
   - Compute launch angle for ballistic arc to aim_pos
5. Spawn projectile entity: `pos=archer.pos, vel=launch_velocity, Projectile{...}`
6. Each tick: projectile substeps (dt_sub=0.1 within tick dt)
   - `vel.z -= GRAVITY * dt_sub` (GRAVITY=10.0)
   - `pos += vel * dt_sub`
   - Check entity collision at each substep via spatial index
   - Check ground collision: `pos.z <= terrain_height_at(pos.xy)`
7. On entity hit: construct Impact from projectile properties → D2 pipeline
8. On ground hit: projectile becomes inert entity (no Mobile, no Projectile component,
   keeps position). Future: salvageable

### UC3: Stagger During Attack

1. Swordsman is mid-swing (committed=true), takes a hit that triggers stagger
2. Attack does NOT cancel — it resolves with degraded parameters:
   - Accuracy penalty: dispersion increases (hit location more random)
   - Force reduction: effective swing speed reduced
3. If swordsman was uncommitted (early windup): attack cancels entirely, cooldown
   begins without resolving

### UC4: Shield Behavior

1. Swordsman equips iron sword (weapon slot, hands_required=1) and wooden shield
   (weapon slot → active blocking with block_arc, block_efficiency)
2. During combat: shield provides blocking via D2's block check step
3. Swordsman drops shield or slings it: shield moves to back slot → functions as
   torso armor (ArmorProperties active, WeaponProperties inactive)
4. Archer equips bow (hands_required=2): cannot equip shield in weapon slot.
   Equipment system rejects the equip command

### UC5: Entity Death and Persistence

1. Swordsman bleeds out (blood <= 0.0 from D pipeline)
2. Entity is NOT removed from SlotMap
3. Active components stripped: Mobile, Combatant, Vision removed
4. Entity retains: position, Equipment (sword + shield contained_in corpse),
   Body (wounds), owner
5. Corpse remains on battlefield as inert entity. Equipment stays.
   Looting deferred past V3.0

### UC6: Equipment Spawning (V3.0 Bootstrap)

1. No production system in V3.0. God/debug command spawns equipment entities
2. Mapgen creates weapon/armor entities and equips them to soldiers during
   scenario initialization
3. Operations layer's `EquipEntity` command moves equipment between entities
   (pick up weapon from ground). `ProduceEquipment` does not exist in V3.0

## Data Model

### MaterialType

The physical substance. Determines base hardness, density, and physical
interaction properties.

```
enum MaterialType {
    Iron,
    Steel,
    Bronze,
    Leather,
    Wood,
    Bone,
    Cloth,
    Stone,
}
```

### ArmorConstruction

How the armor is shaped/assembled. Determines coverage pattern, flex behavior,
and how damage types interact structurally.

```
enum ArmorConstruction {
    Plate,    // Rigid sheets. Deflects slash (angle matters), vulnerable at joints
    Chain,    // Interlocking rings. Disperses pierce point loads, flexible
    Padded,   // Layered soft material. Absorbs crush energy, light
    Layered,  // Multiple materials bonded. Hybrid properties
}
```

Penetration modifiers are a two-axis lookup: `(DamageType, MaterialType,
ArmorConstruction) → modifier`. Chain disperses pierce, plate deflects slash,
padded absorbs crush. Material determines resistance magnitude within
construction type.

### DamageType

```
enum DamageType {
    Slash,   // Edge cuts. Effective vs unarmored/leather. Lacerations, high bleed
    Pierce,  // Point focus. Gets through chain, exploits plate gaps. Punctures
    Crush,   // Blunt force. Ignores surface hardness. Concussions, stagger
}
```

### WeaponProperties

Attached to weapon entities. MaterialType on the weapon determines its hardness
and edge quality (an iron sword vs steel sword).

```
struct WeaponProperties {
    material: MaterialType,
    damage_type: DamageType,
    sharpness: f32,          // Edge/point quality. Degrades with use (V3.1)
    hardness: f32,           // Derived from material, resistance to deformation
    weight: f32,             // kg. Affects swing speed, cooldown, stamina cost
    reach: f32,              // meters. Melee range check distance
    hands_required: u8,      // 1 or 2. Equipment validation constraint
    block_arc: f32,          // radians. Angular coverage when blocking (shields)
    block_efficiency: f32,   // 0-1. Stamina cost multiplier when blocking
    // Ranged-only fields (zero/None for melee weapons)
    projectile_speed: f32,   // m/s. Launch velocity for spawned projectile
    projectile_arc: bool,    // true=parabolic (bow), false=flat (crossbow, deferred)
    accuracy_base: f32,      // 0-1. Base accuracy before skill modifier
    // Attack timing
    windup_ticks: u16,       // Ticks before attack resolves
    commitment_fraction: f32,// 0-1. Fraction of windup at which attack becomes committed
    base_recovery: f32,      // Base cooldown ticks after attack. Modified by weight/stamina
}
```

### ArmorProperties

Attached to armor entities. One armor entity can cover multiple body zones
(chain hauberk covers torso + both arms). Multiple Equipment slots point to
the same EntityKey.

```
struct ArmorProperties {
    material: MaterialType,
    construction: ArmorConstruction,
    hardness: f32,       // Derived from material. Resistance to penetration
    thickness: f32,      // mm. Thicker = more resistance, more weight
    coverage: f32,       // 0-1. Fraction of zone covered. Gaps at < 1.0
    weight: f32,         // kg. Affects encumbrance/movement speed
    zones_covered: Vec<BodyZone>,  // Which body zones this armor covers
}
```

### Shield Entity

A shield entity carries **both** WeaponProperties and ArmorProperties. Behavior
depends on equipment slot:

- **Weapon slot**: WeaponProperties active (block_arc, block_efficiency used by
  D2's block check). ArmorProperties ignored.
- **Back slot**: ArmorProperties active (covers torso zone as passive armor).
  WeaponProperties ignored.

### Projectile Component

Lightweight component on in-flight projectile entities. Removed on impact
(entity becomes inert).

```
struct Projectile {
    damage_type: DamageType,
    sharpness: f32,
    hardness: f32,
    mass: f32,            // kg. For kinetic energy calculation
    arc: bool,            // Parabolic or flat trajectory
    source_owner: u8,     // Player who fired. For kill attribution + friendly fire
}
```

### AttackState Component

Temporary component on entities currently executing an attack. Removed when
attack resolves or cancels.

```
struct AttackState {
    target: EntityKey,
    weapon: EntityKey,
    progress: u16,        // Ticks elapsed since attack started
    committed: bool,      // Past commitment threshold — cannot cancel
}
```

### Equipment Component

On entities that can carry equipment. References weapon/armor entities by
EntityKey. Those entities are `contained_in` the wearer.

```
struct Equipment {
    weapon: Option<EntityKey>,       // Active weapon (melee or ranged)
    shield: Option<EntityKey>,       // Shield in weapon slot (blocking) or None
    back: Option<EntityKey>,         // Shield slung on back (passive armor) or other
    armor_slots: [Option<EntityKey>; ZONE_COUNT],  // Per-zone armor references
}
```

Validation: if `weapon` has `hands_required >= 2`, `shield` must be None.

## V3.0 Starting Equipment

Minimal set to validate the pipeline:

| Entity | Material | Type | Notes |
|--------|----------|------|-------|
| Iron Sword | Iron | Weapon (Slash) | hands=1, reach ~1.5m, medium windup |
| Wooden Bow | Wood | Weapon (Pierce, ranged) | hands=2, arc=true, projectile_speed ~50m/s |
| Leather Cuirass | Leather | Armor (Padded) | Covers torso. Soft, light, low hardness |
| Bronze Breastplate | Bronze | Armor (Plate) | Covers torso. Hard, rigid, heavy |

Arrow projectiles inherit Pierce damage type, low mass (~0.05kg), arc=true.

## Attack Pipeline

### Melee Resolution (W2, Wave 1)

Per tick, for each entity with AttackState:

1. **Increment progress.** `attack.progress += 1`
2. **Check commitment.** If `progress >= windup_ticks * commitment_fraction` and
   not yet committed: set `committed = true`
3. **Check windup complete.** If `progress < windup_ticks`: continue (not ready)
4. **Range check.** Compute continuous distance between attacker.pos and
   target.pos. If `distance > weapon.reach`: attack whiffs. Remove AttackState,
   begin cooldown. Done.
5. **Compute aim_center.** From height difference (D.6 bias) and attacker's
   physical context. Not a tactical directive — derived from geometry.
6. **Compute attack parameters.** `swing_speed = base_swing / (weight / weight_ref)`.
   If staggered during committed phase: apply accuracy penalty (increase
   dispersion) and force reduction (reduce swing_speed).
7. **Construct Impact.** `Impact { damage_type, sharpness, hardness, mass: weight,
   speed: swing_speed, aim_center, dispersion, attacker, defender }`
8. **Submit to D2 pipeline.** Impact → 7-step resolution → wound or deflection.
9. **Begin cooldown.** `recovery_ticks = base_recovery * (weight / weight_ref) *
   (1.0 / attacker.stamina)`. Entity cannot start new attack until cooldown
   expires.

### Ranged Resolution (W3, Wave 2)

**Firing:**

1. Windup completes (same AttackState progression as melee)
2. **Aim computation.**
   - Estimate flight_time from distance and projectile_speed
   - `aim_pos = lerp(target.pos, target.pos + target.vel * flight_time, combat_skill)`
   - Compute launch angle for ballistic intercept to aim_pos
   - Apply accuracy: `aim_pos += dispersion * random_offset` where dispersion
     decreases with accuracy_base and combat_skill
3. **Spawn projectile entity.** Position = archer's pos + height offset.
   Velocity = computed launch vector. Projectile component from weapon stats.
4. Remove AttackState, begin cooldown.

**Projectile tick (substep integration):**

Projectiles integrate at a smaller timestep within each simulation tick to
prevent clipping. For dt=1.0, substep at dt_sub (e.g., 0.1 = 10 substeps).

Per substep:
1. If arc: `vel.z -= GRAVITY * dt_sub` (GRAVITY = 10.0 m/s²)
2. `pos += vel * dt_sub`
3. **Entity collision.** Query spatial index at pos. For each entity in range:
   check if projectile pos is within entity's collision radius. On hit:
   construct Impact from Projectile component → D2 pipeline. Remove Projectile
   and Mobile components (entity becomes inert at impact point).
4. **Ground collision.** If `pos.z <= terrain_height_at(pos.xy)`: remove
   Projectile and Mobile components. Entity stays as inert arrow on ground.

**Friendly fire:** Emerges naturally. Projectile collision checks all entities
in path, not just enemies. An arrow that hits a friendly unit produces the same
Impact as one hitting an enemy.

### Stagger Interaction

When D's stagger system triggers on an entity with AttackState:

- **Uncommitted** (`committed == false`): Attack cancels. Remove AttackState.
  Begin cooldown (no impact).
- **Committed** (`committed == true`): Attack resolves with penalties:
  - Dispersion multiplied by stagger_accuracy_penalty (e.g., 2.0x)
  - Swing speed / draw force multiplied by stagger_force_penalty (e.g., 0.5x)
  - These degrade the resulting Impact, but it still enters D2

## Entity Lifecycle

### Death

When blood <= 0.0 (from D pipeline bleed accumulation):

1. Strip active components: remove Mobile, Combatant, Vision, AttackState
2. Retain: position (Vec3), Equipment (all contained weapon/armor entities),
   Body (wound record), owner, Resource (if carrying)
3. Entity stays in SlotMap as inert corpse
4. Equipment entities remain `contained_in` the corpse
5. No entity removal in V3.0. Entities only grow over a match.

### Inert Projectiles

Projectile entities that hit ground or resolve impact:

1. Remove Projectile component and Mobile component
2. Retain: position (where it landed/stuck)
3. Entity stays in SlotMap as inert arrow/stone on ground
4. Future: recoverable by operations command

### Equipment Spawning (V3.0)

No production system. Mapgen/god commands create equipment:

1. Create weapon/armor entity with position and properties
2. `EquipEntity { person, equipment }` command: moves equipment entity to
   `contained_in` person, updates person's Equipment component slots
3. Validation: check hands_required vs shield slot, check zone coverage conflicts
4. Operations layer's `EquipEntity` works for reassignment (pick up from ground
   or transfer between entities). `ProduceEquipment` does not exist.

## Line of Sight

**Deferred.** Required when flat-trajectory weapons (crossbow) are added. Arc
projectiles in V3.0 fly parabolic paths — no terrain/obstacle occlusion check.
Friendly fire still occurs via projectile-entity collision along the arc path.

When implemented (with flat-trajectory weapons):
- Ray-march through hexes between source and target
- Check terrain height and entity height for occlusion at each hex
- Arc projectiles: check initial clearance only (can arrow clear nearby
  obstacles at launch angle?)
- Height advantage: elevated archer has better LOS naturally via z-component

## Security

No new attack surfaces. This is engine-internal game logic with no network
exposure. All inputs come from the agent system (trusted, deterministic).
Equipment spawning is god-command only (no player-facing API in V3.0).

## Privacy

No PII handled. Game simulation entities only.

## Audit

Kill attribution tracked via `source_owner` on Projectile and `attacker` on
Impact. Wound records on Body track which entity inflicted each wound. Sufficient
for post-match replay and scoreboard.

## Convention Observations

- V2 uses `Option<Component>` on Entity struct for composition. V3 continues
  this pattern. If entity count grows past 10k, this becomes memory-inefficient
  vs a proper ECS (archetypal or sparse-set). Not a problem for V3.0 scope
  (500 entities + ~1200 inert), but worth noting for V3.2+ scaling.

- Shield dual-role (weapon slot vs back slot) means one entity needs both
  WeaponProperties and ArmorProperties. This is the first entity type with two
  property components. The pattern may generalize (e.g., a torch is both a
  light source and a weapon).

## Scope

### V3.0 (ship this)

- MaterialType enum (Iron, Steel, Bronze, Leather, Wood, Bone, Cloth, Stone)
- ArmorConstruction enum (Plate, Chain, Padded, Layered)
- DamageType enum (Slash, Pierce, Crush)
- WeaponProperties struct with material, damage_type, timing, reach, ranged fields
- ArmorProperties struct with material, construction, coverage, zones_covered
- Projectile component struct
- AttackState component struct
- Equipment component with validation (hands_required)
- Shield dual-role by slot
- Melee attack resolution: windup → commitment → range check → Impact
- Two-state commitment model (uncommitted=cancelable, committed=degrades)
- Computed cooldown from weight and stamina
- Aim center from physics (height diff, skill), not tactical directive
- Projectile spawning, substep integration, gravity (10.0 m/s²)
- Skill-interpolated aim (lead targets by combat_skill)
- Projectile-entity collision detection per substep
- Friendly fire via physics
- Inert projectile entities (persist on ground)
- Inert corpse entities (persist with equipment)
- Stagger interaction with attack commitment
- God-command equipment spawning for mapgen
- Starting set: iron sword, wooden bow, leather cuirass, bronze breastplate
- `EquipEntity` operations command

### Deferred

- **LOS** — required for flat-trajectory weapons (crossbow). Explicitly deferred,
  not forgotten. Needed for W3 when crossbow/sling added.
- **Weapon degradation** — sharpness field exists but doesn't change. V3.1.
- **Additional weapons** — spear, mace, crossbow, sling, shield-as-weapon.
  Data entry once pipeline proven. V3.1.
- **Additional armor** — chain mail, cloth tunic, iron plate. Same. V3.1.
- **Equipment production** — workshops, recipes, raw materials. No production
  system in V3.0.
- **Looting** — extract equipment from corpses/ground. Operations command. V3.1.
- **Arrow salvage** — recover inert arrows. V3.1.
- **Inert entity LOD culling** — renderer concern (R domain).
- **Per-weapon commitment curves** — V3.0 uses binary threshold. Smooth curves
  per weapon type deferred.

## Verification

- [ ] Iron sword melee attack produces Impact with DamageType::Slash
- [ ] Wooden bow ranged attack spawns projectile entity with arc physics
- [ ] Projectile substep integration prevents clipping (arrow doesn't skip
      through entities at dt=1.0)
- [ ] Skill=0 archer misses moving target; skill=1 archer leads and hits
- [ ] Target moving out of sword reach during windup → attack whiffs
- [ ] Stagger during uncommitted windup → attack cancels
- [ ] Stagger during committed windup → attack resolves with degraded accuracy
- [ ] Shield in weapon slot provides blocking (D2 block check uses block_arc)
- [ ] Shield in back slot provides torso armor (D2 surface lookup finds it)
- [ ] Equipment validation rejects shield equip when bow (hands_required=2) equipped
- [ ] Dead entity retains position and equipment, loses Mobile/Combatant/Vision
- [ ] Projectile hitting ground becomes inert entity (no Mobile/Projectile)
- [ ] Friendly fire: arrow hits friendly entity in path
- [ ] Penetration lookup uses both MaterialType and ArmorConstruction
- [ ] Cooldown increases with weapon weight, decreases with stamina
- [ ] Two players, 50 soldiers each, melee + ranged combat resolves < 1ms/tick

## Implementation Waves

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| W1 | 0 | — | MaterialType, ArmorConstruction, DamageType, WeaponProperties, ArmorProperties, Projectile component, Equipment component with validation |
| W2 | 1 | D2, S1 | AttackState, melee attack resolution (windup, commitment, range check → Impact), computed cooldown, stagger interaction |
| W3 | 2 | W2, M1, S1 | Projectile spawning, substep integration, gravity, skill-interpolated aim, collision detection, inert lifecycle |

## Files Modified

### New files
- `crates/engine/src/v3/weapon.rs` — WeaponProperties, AttackState, melee resolution, attack profiles
- `crates/engine/src/v3/armor.rs` — ArmorProperties, MaterialType, ArmorConstruction, penetration modifier lookup
- `crates/engine/src/v3/projectile.rs` — Projectile component, substep integration, ballistic aim computation, collision detection
- `crates/engine/src/v3/equipment.rs` — Equipment component, slot validation, equip/unequip commands, shield dual-role logic

### Modified files
- `crates/engine/src/v3/mod.rs` — register new modules
- `crates/engine/src/v3/damage.rs` (D domain) — Impact struct must accept inputs from both melee and projectile sources
- `crates/engine/src/v3/body.rs` (D domain) — death handling strips active components (W defines which components to strip)
- `crates/engine/src/v3/spatial.rs` (S domain) — projectile substep queries spatial index
