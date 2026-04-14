# Spec: V3 Entity Unification

Updated: 2026-04-13 (revision 2 — continuous spatial model, material-interaction
damage, unified attack pipeline, multi-resolution time, z-axis)

## Vision

Replace the V2 engine's four separate entity types (Unit, Convoy, Population,
Settlement) with a single composable Entity primitive. Every mobile thing is an
entity. Every static thing is an entity. Entities contain entities. A soldier is
a person with combat skill, wearing armor, carrying a weapon. A convoy is a person
leading pack animals carrying cargo. A settlement is a structure containing
population. Composition determines capability — unit types emerge from properties,
not from hardcoded categories.

Entities live in **continuous 3D space**. The hex grid is a spatial acceleration
structure — a projection derived from each entity's real position, not the position
itself. Movement uses steering behaviors in continuous space. Nothing teleports.

Combat resolves through **material-interaction physics**. There are no health bars.
A sword hits a body zone, the impact is resolved against whatever armor covers that
zone, and the result is a wound with a bleed rate — or a deflection. Most deaths
are from accumulated bleeding. The same pipeline resolves a fist, a spear thrust,
an arrow, a sling stone, and (in future versions) a cannon ball. Weapons and armor
are defined by physical properties (material, sharpness, thickness, weight), not
by abstract damage/defense numbers.

Restructure the agent architecture from "military agents + autonomous city AI" to
three coordinated layers: Strategy (grand vision, posture, priorities), Operations
(theater allocation, logistics, production), and Tactical (per-stack combat
decisions, weapon-armor matchup reasoning). All three layers are shared
infrastructure; agent personalities (Spread, Striker, Turtle) differentiate at the
Strategy layer only.

Replace the SVG frontend renderer with PixiJS (WebGL) to support the target scale
of 100k tiles and 10k entities with zoom/pan, viewport culling, entity
interpolation between ticks, and LOD tiers. Primary viewport is top-down; future
isometric toggle to visualize height.

## Supersedes

- `docs/plans/v2-remaining-systems.md` — economy, population, convoys, roads, terrain.
  Concepts retained, implementation replaced by entity-based approach.
- `docs/plans/frontend-rendering-overhaul.md` — PixiJS migration. Folded into this spec.
- `docs/plans/svg-quick-fixes.md` — SVG renderer improvements. Moot; SVG replaced.
- `docs/plans/agent-intelligence-pipeline.md` — agent improvements. Superseded by
  three-layer architecture.
- Revision 1 of this spec — combat geometry, combat resolution, entity component
  details, movement model, and simulation tick sections are replaced.

## Use Cases

### 1. Two armies meet

A stack of 15 swordsmen moves toward a stack of 10 spearmen. Both stacks move
through continuous space — no teleportation. As they close to spear range (~3m),
the spearmen's tactical layer orders engagement: spear thrusts target the
approaching swordsmen. Spears have longer reach, so the spearmen get first strikes.
Each thrust resolves: hit location on the target's body, check armor at that zone,
compute penetration from spear hardness/sharpness vs armor material/thickness at
the impact angle. Some thrusts pierce leather, causing puncture wounds that bleed.
Some deflect off iron breastplates.

The swordsmen close to melee range (~1.5m). Now swords engage — slashing attacks
against the spearmen, who must block with their shield or parry with the spear
shaft. Each block costs stamina. Sustained pressure exhausts the defenders. A
flanking group of swordsmen arrives from the side — the spearmen can't face both
directions. Side and rear attacks bypass shields entirely.

Wounded soldiers bleed. Blood loss accumulates across all wounds. When vitality
drops below threshold, the person collapses. The tactical layer decides when to
retreat — retreating soldiers take rear hits as they withdraw through continuous
space, damage proportional to how long their backs are turned.

### 2. Archer volley over friendly lines

An archer line stands behind a friendly spear wall. Operations has positioned them
10 hexes (~1-2km) behind the front. The tactical layer orders volley fire at the
enemy formation visible 15 hexes away. Each arrow is an entity: it launches at a
high arc (projectile with Vec3 velocity, gravity applied each tick). The arrows
rise above the friendly spearmen (z > friendlies' height at that x,y position),
arc over them, and descend into the enemy formation.

Each arrow that reaches ground level checks for entity collisions at impact. An
arrow hitting an unarmored target: pierce damage type, high sharpness — penetrates
cloth/leather, deep wound, high bleed rate. The same arrow hitting plate armor:
penetration check fails at typical angles, deflects. Against chain mail: may
penetrate at perpendicular impact, deflects at glancing angles.

Crossbow bolts travel flat (no arc) — they can't fire over friendly lines but hit
harder at shorter range. The tactical layer must position crossbowmen with clear
sight lines.

### 3. Slingers vs plate infantry

The agent's tactical layer sends slingers against plate-armored infantry. Sling
stones are crush damage against plate: the penetration check fails (stone can't
pierce iron), but the impact force transmits through the armor — stagger effect,
stamina drain, possible concussion at head zone. The plate infantry is barely
wounded but fatigued. The agent's observation journal records: "sling vs plate →
low wound rate, moderate stagger." The empirical damage table updates. Next
engagement, the tactical layer assigns slingers to softer targets.

### 4. Settlement produces soldiers

Same as revision 1: population entities in settlement structures. Operations
assigns roles, soldiers train (gain combat_skill). Additionally, soldiers are
equipped from stockpile — weapon entities and armor entities are assigned to body
slots. A soldier without armor fights in cloth. A soldier with a bronze sword and
leather cuirass fights differently from one with an iron spear and chain mail. The
operations layer's equipment decisions affect what the tactical layer can do.

### 5. Convoy under raid

A supply convoy (person + pack animals + food/material entities) moves through
continuous space along a road. An enemy cavalry stack approaches from a
perpendicular direction. The cavalry entities have high speed — they close the
continuous-space distance quickly. The convoy has no combatant components; the
escorts (if any) engage. If the escorts are overwhelmed, the cavalry reaches the
pack animals. Pack animals die (from combat) and drop their contained cargo
entities onto the ground. The raider can pick up cargo or destroy it.

### 6. Multi-resolution battle observation

The map runs at strategic resolution (1 tick = 1 hour) — armies march, population
grows. Two armies make contact in the eastern theater. The engine switches that
region to tactical resolution (1 tick = 1 second) — individual combat resolves.
The spectator clicks on a specific duel. The engine switches that micro-region to
cinematic resolution (1 tick = 10ms) — you watch a spear thrust connect, see the
angle of incidence against sloped plate, watch the point skip off the curved
surface. The rest of the battle continues at tactical resolution. The western
provinces stay strategic.

---

## Architecture

### Spatial Model

**The hex grid is not the world. It is a lens over the world.**

Entities exist in continuous 3D space. The hex grid is a spatial acceleration
structure — a projection computed from each entity's position, used for O(1)
proximity queries and for game-mechanical bucketing (territory, resource collection,
structure interaction).

```
World space: Vec3 (x: f64, y: f64, z: f64)
  x, y: continuous horizontal position
  z: altitude/depth
    negative = underground (tunnel depth)
    0.0 = surface (adjusted by terrain height at that x,y)
    positive = above surface (scouting balloon, bird, future air unit)

Hex projection: pixel_to_hex(pos.xy) -> Axial
  Recomputed each tick. Not stored as authoritative state.
  Uses cube_round algorithm for O(1) conversion.

Hex column: (Axial, layer) where layer = Underground | Surface | Air
  Derived from hex projection + z relative to terrain height.
```

One hex ≈ 150 meters across (flat-to-flat). A 30×30 map ≈ 4.5 km per side. A
200×200 map ≈ 30 km per side (theater of war). A person walks across one hex in
~2 minutes real time (≈20 ticks at 1 tick/sec default).

**Structures snap to hex centers.** A farmhouse, a wall segment, a depot — these
are fixed infrastructure anchored to the hex grid. Their position is a hex
coordinate, not continuous. This is intentional: infrastructure defines the grid's
game-mechanical meaning.

**Entities have continuous positions.** A person, a horse, a wagon, a projectile —
these live in continuous (x, y, z) space. Their hex membership is derived.

### Movement

Movement uses **steering behaviors** in continuous space. No teleportation between
hex centers. No "on an edge" intermediate state.

Three-layer movement architecture:

1. **Pathfinding** — A* on hex graph produces a waypoint sequence of hex centers.
   For mass movement, flow fields: Dijkstra fills the hex grid with direction
   vectors, entities sample via barycentric interpolation of 3 nearest hex centers.

2. **Path smoothing** — String-pulling / funnel algorithm on the hex center polyline
   removes unnecessary waypoints, producing a smooth continuous-space path.

3. **Steering** — Craig Reynolds behaviors operate in continuous space:
   - `Seek` — steer toward next waypoint
   - `Arrive` — decelerate approaching destination
   - `Separation` — maintain personal spacing (prevents overlap)
   - `Cohesion` — stay with formation group
   - `Obstacle avoidance` — steer around impassable terrain
   - `FlowFieldFollow` — sample flow field at continuous position

   Output: acceleration vector. Integration: `vel += accel * dt`, `pos += vel * dt`.

```
Mobile
  vel: Vec3              // current velocity
  max_speed: f32         // base maximum speed
  steering_force: f32    // maximum acceleration
  radius: f32            // collision radius (person ~10m, ox cart ~30m)
  waypoints: Vec<Vec3>   // smoothed path waypoints in continuous space
```

**Entities have a radius, not a footprint.** A person: ~10m radius (0.07 hex). An
ox cart: ~30m (0.2 hex). A marching column is N entities in formation, each with
their own position. The column "spans multiple hexes" because its members are in
multiple hexes — no special multi-hex entity logic.

**Hex boundary hysteresis.** An entity near a hex boundary would otherwise flicker
between hexes tick-to-tick. Once assigned to a hex, an entity only changes hex
membership when its continuous position is past the center of the new hex (> 50%
of the way across). Prevents oscillation.

### Collision

Continuous positions require collision resolution. The model matches the
simulation's grain — people moving through space, not rigid body physics.

**Entity-entity separation.** Soft collision via Reynolds' `Separation` steering
behavior. Entities within each other's radius experience a repulsive steering
force. Entities pushed together (bottleneck, retreat) compress but don't phase
through each other. Formations manage spacing; separation enforces it.

**Entity-terrain.** Hard boundary. Before committing a position update, check
destination hex passability. If impassable (mountain, deep water, wall), clamp
velocity to the boundary. Walls on hex edges block movement across specific edges.

**Entity-structure.** Structures occupy hex centers. Walled structures block entry
unless the entity has permission (friendly) or the wall is breached.

### Entity Model

One `SlotMap<EntityKey, Entity>` replaces all V2 entity storage.

```
Entity
  id: u32                       // Public monotonic ID
  pos: Option<Vec3>             // None if contained in another entity
  owner: Option<u8>             // Player owner

  // Containment
  contained_in: Option<EntityKey>
  contains: Vec<EntityKey>

  // Components (presence = capability)
  person: Option<Person>         // Is a living being
  mobile: Option<Mobile>         // Can move through space
  vision: Option<Vision>         // Can see
  combatant: Option<Combatant>   // Can fight
  body: Option<Body>             // Has body zones, takes wounds
  equipment: Option<Equipment>   // Wears armor, carries weapons
  resource: Option<Resource>     // Is a material/food quantity
  structure: Option<Structure>   // Is a building/fortification
  projectile: Option<Projectile> // Is a projectile in flight
```

#### Component Details

```
Person
  blood: f32             // 0.0 dead, 1.0 full. Drains from wounds.
  stamina: f32           // 0.0 exhausted, 1.0 full. Drains from blocking, sprinting.
  combat_skill: f32      // Training level 0.0-1.0. Affects aim, block timing.
  role: Role             // Idle, Farmer, Worker, Soldier, Builder

Mobile
  vel: Vec3              // Current velocity
  max_speed: f32         // Base max speed (terrain, encumbrance modify)
  steering_force: f32    // Max acceleration magnitude
  radius: f32            // Collision radius in world units
  waypoints: Vec<Vec3>   // Smoothed path in continuous space

Vision
  radius: f32            // Base vision distance (modified by height, weather)

Combatant
  facing: f32            // Radians. 0 = east, PI/2 = north.
  stance: Stance         // Affects block arc and mobility
  target: Option<EntityKey>  // Current engagement target

Body
  zones: [BodyZone; ZONE_COUNT]  // Head, Torso, LeftArm, RightArm, Legs
  wounds: Vec<Wound>             // Active wounds with bleed rates

BodyZone
  armor_slot: Option<EntityKey>  // Armor entity covering this zone
  surface_normal: f32            // Angle of surface (for ricochet calc)

Wound
  zone: usize            // Which body zone
  bleed_rate: f32        // Blood loss per tick
  severity: Severity     // Scratch, Laceration, Puncture, Fracture, Severed

Equipment
  weapon: Option<EntityKey>      // Wielded weapon entity
  shield: Option<EntityKey>      // Held shield entity
  armor_slots: [Option<EntityKey>; ZONE_COUNT]  // Per-zone armor

Resource
  resource_type: ResourceType   // Food, Material, Ore, Wood, Stone
  amount: f32

Structure
  structure_type: StructureType // Farm, Village, City, Depot, Wall, Tower
  build_progress: f32           // 0.0 - 1.0
  integrity: f32                // Structural health (material-dependent)
  capacity: usize               // Max contained entities
  material: MaterialType        // What it's built from (affects integrity)

Projectile
  damage_type: DamageType       // Pierce, Crush (no Slash for projectiles)
  sharpness: f32
  hardness: f32
  mass: f32                     // For kinetic energy at impact
  arc: bool                     // true = parabolic (arrow), false = flat (bolt)
  source_owner: u8              // Who fired it
```

#### Weapon and Armor as Entities

Weapons and armor are entities with specialized components. They exist in the
world — on the ground, in a stockpile, equipped by a person. Equipment is not
abstract stats; it's a physical object with material properties.

```
WeaponProperties
  damage_type: DamageType    // Slash, Pierce, Crush
  sharpness: f32             // Edge quality. Degrades with use.
  hardness: f32              // Material hardness (bronze < iron < steel)
  weight: f32                // Affects swing speed, force, stamina cost
  reach: f32                 // Engagement distance in world units
  block_arc: f32             // Radians of coverage when parrying
  block_efficiency: f32      // Stamina cost multiplier when blocking
  projectile_speed: Option<f32>  // None = melee. Some(v) = ranged.
  projectile_arc: bool       // Whether projectiles arc over obstacles
  accuracy_base: f32         // Base hit probability at range 0

ArmorProperties
  material: MaterialType     // Cloth, Leather, Chain, Plate
  hardness: f32              // Resists penetration
  thickness: f32             // Depth of material
  coverage: f32              // 0.0-1.0 fraction of zone covered (gaps in plate)
  weight: f32                // Affects wearer stamina drain, speed
```

**Damage type interactions:**
- **Slash** — effective against unarmored/leather, deflected by metal. Slashes
  cause lacerations (high bleed rate, moderate depth).
- **Pierce** — concentrates force on a point. Gets through chain links, finds
  gaps in plate. Puncture wounds (moderate bleed rate, deep).
- **Crush** — ignores surface hardness, transmits force through armor. Causes
  concussions, fractures, stagger. Low bleed but high incapacitation.

#### Composition Examples

**Swordsman**: Entity { person(blood=1.0, stamina=1.0, skill=0.6, Soldier),
  mobile(speed=3.0, radius=10), vision(150), combatant, body(5 zones),
  equipment(iron sword, leather cuirass, wooden shield) }

**Spearman**: Entity { person(..., Soldier), mobile(speed=2.8, radius=10),
  vision(150), combatant, body, equipment(iron spear, chain hauberk, no shield) }
  - Longer reach than swordsman. First strike advantage. Vulnerable if flanked.

**Archer**: Entity { person(..., Soldier), mobile(speed=3.0, radius=10),
  vision(200), combatant, body, equipment(bow, cloth tunic, no shield) }
  - Ranged. Arc projectiles over friendlies. Useless in melee. Unarmored.

**Slinger**: Entity { person(..., Soldier), mobile(speed=3.5, radius=10),
  vision(150), combatant, body, equipment(sling, cloth tunic, no shield) }
  - Crush damage. Cheap. Effective against unarmored. Useless against plate.

**Farmer**: Entity { person(blood=1.0, stamina=1.0, skill=0.1, Farmer),
  mobile(speed=2.5, radius=10), vision(100) }
  - No combatant or body components by default. Can be drafted: add combatant +
    body, fights poorly (low skill, no equipment).

**Pack animal**: Entity { person(blood=1.0, stamina=0.8, skill=0.0),
  mobile(speed=2.0, radius=25), contains=[cargo entities] }
  - Larger radius. Can't fight. Carries cargo.

**Arrow in flight**: Entity { pos: Vec3, mobile(vel=Vec3(40, 0, 15)),
  projectile(Pierce, sharpness=0.7, hardness=0.5, mass=0.05, arc=true) }
  - Lightweight entity. No person, no body. Exists during flight only.
  - Gravity applied: vel.z -= 9.8 * dt each tick.

**Iron breastplate (on ground)**: Entity { pos: Vec3, resource-like but with
  ArmorProperties(Plate, hardness=0.8, thickness=2mm, coverage=0.85, weight=15) }
  - Sits in a stockpile or on the ground until equipped.

### Stacking

A "stack" is NOT an entity. It's a command grouping: entities near each other in
continuous space, belonging to the same player, grouped by the operations layer.
Stacks are identified by explicit StackId assigned by operations.

The tactical layer reasons about stacks. The frontend renders stacks as grouped
icons at mid zoom. Individual entities within a stack are visible at close zoom.

Moving a stack sets waypoints for all entities in it. Formation algorithms
distribute entities around the stack's center of mass. Splitting and merging are
operations-layer commands.

### Damage Model

**No health bars.** Combat resolves through material-interaction physics. The same
pipeline handles every scale from fist to future cannon.

#### Impact Resolution Pipeline

Every attack — melee swing, arrow hit, sling stone, ram against wall — passes
through this pipeline:

**Step 1: Hit Location**

Determine where on the target the attack lands.

```
hit_location: f32 = aim_center + dispersion * random
```

`aim_center` is attacker intent (head, torso, etc). `dispersion` decreases with
combat_skill and increases with distance, target speed, and attacker fatigue.

The hit_location value maps to a body zone:

| Range | Zone | Notes |
|-------|------|-------|
| 0.00 - 0.10 | Head | Small target, high lethality |
| 0.10 - 0.50 | Torso | Largest target, vital organs |
| 0.50 - 0.65 | Left Arm | Reduces grip / shield use |
| 0.65 - 0.80 | Right Arm | Reduces weapon use |
| 0.80 - 1.00 | Legs | Reduces movement speed |

Zone boundaries are thresholds on a continuous value — extending to 20 zones or
continuous coverage is just changing the lookup table, not the resolution logic.

**Step 2: Block Check**

Before impact resolution, check if the defender blocks.

- Is the attack within the defender's block arc? (Depends on weapon/shield held
  and current facing relative to attack angle.)
- Does the defender have stamina to execute the block?

Successful block: **full negation** of the attack. No partial blocks. Stamina cost
to blocker proportional to attack force (weight × speed). Heavy weapons drain
blocker stamina faster. A shield blocks a wider arc but is heavier. A sword parry
blocks a narrow arc but costs less stamina.

When stamina is depleted, the defender **cannot block**. This is how sustained
pressure breaks a defense — not by penetrating the shield, but by exhausting the
person holding it. Shield walls work because defenders rotate — fresh arms replace
tired ones.

Stamina recovers slowly when not blocking or sprinting. Recovery rate decreases
with wound count and blood loss.

**Step 3: Surface Lookup**

What covers the hit zone? Look up the armor entity (if any) equipped at that zone.
Get its material properties: hardness, thickness, coverage.

Coverage < 1.0 means gaps. Roll against coverage to determine if the attack hits
armor or finds a gap. Chain mail: high coverage (0.95) but low thickness. Plate:
lower coverage (0.85 — joints, visor slits) but high thickness.

**Step 4: Angle of Incidence**

Compute the attack angle relative to the armor surface normal at the hit zone.

```
angle = |attack_direction - surface_normal|
```

Glancing blows (high angle) deflect. Perpendicular hits (low angle) penetrate.
This is the ricochet primitive: a sloped breastplate deflects at shallow angles
exactly like sloped tank armor. The surface_normal per zone captures the body's
curvature — a torso hit from the side strikes at a different angle than from the
front.

**Step 5: Penetration Check**

Does the attack's penetration exceed the surface's resistance?

```
kinetic_energy = 0.5 * mass * speed²
penetration_factor = kinetic_energy * sharpness / cross_section
resistance = armor_hardness * armor_thickness / sin(angle)

if penetration_factor > resistance:
  penetrate → wound
else:
  deflect → stagger (force transmitted, no wound)
```

Damage type modifiers:
- **Slash**: high cross_section (edge), effective against soft materials, deflected
  by hard surfaces. Multiplier: 1.0x against cloth/leather, 0.3x against chain, 0.1x against plate.
- **Pierce**: low cross_section (point), concentrates force. 1.0x against all
  materials. Finds chain links, exploits plate gaps.
- **Crush**: ignores surface hardness entirely. Force transmits through armor.
  No penetration → no wound, but always applies stagger and stamina damage.
  Against head zone: concussion (temporary incapacitation).

**Step 6: Wound Application**

If penetration succeeds:

```
Wound {
  zone: usize,
  severity: computed from penetration_depth,
  bleed_rate: computed from severity and zone vascularity,
}
```

| Severity | Bleed Rate | Effect |
|----------|-----------|--------|
| Scratch | 0.001/tick | Cosmetic. Accumulates over many hits. |
| Laceration | 0.005/tick | Painful. Reduces combat effectiveness at that zone. |
| Puncture | 0.01/tick | Deep. Significant bleed. Organ risk at torso. |
| Fracture | 0.003/tick | Low bleed but zone disabled. Crush damage specialty. |

Zone-specific effects:
- **Head wound**: any severity → accuracy penalty. Puncture+ → unconsciousness risk.
- **Arm wound**: laceration+ → reduced block/attack speed on that side.
- **Leg wound**: laceration+ → reduced movement speed. Fracture → immobile.
- **Torso wound**: puncture+ → accelerated bleed (vital organs).

**Step 7: Bleed Accumulation**

Each tick, total bleed across all wounds drains the person's blood pool:

```
blood -= sum(wound.bleed_rate for wound in wounds)
```

When blood < 0.5: combat effectiveness reduced (slower, less accurate).
When blood < 0.2: collapse (entity falls, can't act, continues bleeding).
When blood <= 0.0: death.

Most combat deaths are from accumulated bleeding over many ticks, not instant
kills. A wounded soldier fights at reduced capacity for a long time before dying.
This creates "walking wounded" — historically the majority of casualties.

If penetration fails (deflection), the impact force still applies:
- Stamina drain on the defender (proportional to kinetic energy)
- Stagger: if force exceeds a threshold, defender is briefly unable to act
  (~2-5 ticks). A mace hit on a helmet concusses through the steel.

### Attack Pipeline

Melee and ranged are the **same system** with different parameters. A sword swing
and an arrow flight both produce an Impact that enters the damage pipeline.

```
AttackProfile
  damage_type: DamageType     // Slash, Pierce, Crush
  sharpness: f32              // Edge/point quality
  hardness: f32               // Material (bronze < iron < steel)
  mass: f32                   // Projectile/weapon mass
  speed: f32                  // Impact speed (swing speed or projectile velocity)
  reach: f32                  // Max engagement distance
  windup: f32                 // Ticks before damage resolves
  projectile_speed: Option<f32>  // None = melee (instant at reach). Some = ranged.
  arc: bool                   // Parabolic trajectory (arrows) vs flat (bolts)
  accuracy_base: f32          // Hit probability at range 0, falls off with distance
```

**Melee** (sword, fist): reach < 2m, no projectile, instant at contact. Swing
speed derived from weapon weight and person's stamina. Must be within reach
distance in continuous space.

**Reach weapons** (spear, pike): reach 2-5m, no projectile. Spear wall holds
enemies at arm's length — spearmen strike before swordsmen can close. Swordsmen
must get inside spear range (flanking, or absorbing the first spear strikes).

**Thrown** (javelin, sling stone): reach 15-40m, has projectile speed, moderate
accuracy. One or two volleys before melee contact. Skirmisher weapon.

**Ranged** (bow, crossbow): reach 80-200m, has projectile speed, accuracy falls
off with distance. Bow: arc=true (arcs over obstacles and friendly lines), fast
rate of fire, lower damage. Crossbow: arc=false (flat trajectory, needs clear line
of sight), slow rate of fire, higher penetration.

#### Projectile Entities

When a ranged attack fires, a lightweight Projectile entity is spawned:

```
Entity {
  pos: Vec3(source_pos),
  mobile: Mobile { vel: Vec3(aim_direction * projectile_speed) },
  projectile: Projectile { damage_type, sharpness, hardness, mass, arc, source_owner },
}
```

Each tick: `pos += vel * dt`. If arc: `vel.z -= GRAVITY * dt` (parabolic).
If not arc: flat trajectory (vel.z stays ~0).

**Impact detection**: when `pos.z <= terrain_height_at(pos.xy)`, the projectile
has reached ground level. Check for entity collisions at the impact point using
the spatial index — any entity whose continuous position is within the projectile's
impact radius. If hit, run the full impact resolution pipeline. If miss, the
projectile entity is removed.

**Friendly fire**: projectiles are entities in continuous space. A flat-trajectory
bolt fired over a friendly line at low angle may intersect a friendly entity's
position. Arc projectiles (arrows) rise above nearby friendlies — but a bad angle
or short range can still hit friendlies. This falls out naturally from the physics.

### Height

Height matters for combat, vision, and movement without requiring full 3D
rendering. Height is a **modifier on the 2D combat and movement math**.

Each entity's effective height comes from two sources:

```
effective_z = terrain_height_at(pos.xy) + personal_elevation
```

Where personal_elevation = standing (1.7m), crouching (1.0m), mounted (2.5m),
on a wall (wall_height), on a siege tower (tower_height).

**Height effects on combat:**
- **Hit location distribution**: Attacking uphill biases hits toward legs and
  shield (attacker swings up). Attacking downhill biases toward head and shoulders
  (attacker swings down). Implemented as an offset on the hit_location roll.
- **Projectile range**: Elevated archers have longer effective range (gravity
  assists downhill shots). Shooting uphill reduces range.
- **Block angle**: Defending against a downhill attacker means blocking above you.
  Shield must be raised — wider exposure, higher stamina cost.

**Height effects on vision:**
- Higher position → larger vision radius. `effective_radius = base_radius + height_bonus(z)`.
- Line-of-sight occlusion: intervening terrain height can block vision to distant
  hexes. Computed by ray-marching through hex columns.

**Height effects on movement:**
- Steep slope → reduced speed. `speed_modifier = 1.0 - slope_penalty * gradient`.
- Very steep → impassable without engineering (stairs, ramp, road).

**Rendering**: Top-down view shows height as terrain shading (darker = lower,
lighter = higher) plus contour lines. Future isometric toggle shows actual
elevation difference. Combat effects of height are visible in consequences —
uphill defenders take fewer casualties, downhill charges are devastating — even
without 3D rendering.

### Z-Axis: Underground and Air

The z-coordinate is designed from day one to support future underground and air
layers. In V3, z = terrain_height for all surface entities. The infrastructure
is in place for later expansion.

**Underground** (future):
- Negative z relative to terrain height. Tunnels, mines, siege sapping.
- Entities transition via tunnel entrances (structure entities on surface hexes).
- Underground movement in continuous space, hex grid applies at each depth layer.
- Vision: hidden from surface unless detection (counter-mining, vibration).

**Air** (future):
- Positive z above terrain. Scouting balloons, messenger birds, (much later)
  flying units.
- Air entities visible from surface. Surface entities can target air with ranged.
- Air entities ignore surface terrain for movement but are affected by it for
  combat (mountains bring surface closer to air layer).

**Spatial indexing for z**: The hex grid gains a layer dimension:
`(Axial, Layer)` where Layer = Underground(depth) | Surface | Air(altitude).
Proximity queries can span layers when relevant (ranged combat from surface to
air, detection from surface to underground).

### Multi-Resolution Time

The engine separates **simulation time** from **tick rate**. Each tick advances
simulation time by `tick_duration` game-seconds.

```
tick_duration: f64   // game-seconds per tick

Modes:
  Strategic:  tick_duration = 3600.0  (1 tick = 1 hour)
  Tactical:   tick_duration = 1.0     (1 tick = 1 second) — DEFAULT
  Cinematic:  tick_duration = 0.01    (1 tick = 10ms)
```

**Strategic mode** (1 tick = 1 hour): Armies march. Population grows. Seasons
change. Convoys traverse the map. No individual combat — engagements in contact
zones are resolved statistically using precomputed damage lookup tables (expected
casualties given force composition, terrain, fortification). Agents run strategy
layer only.

**Tactical mode** (1 tick = 1 second): The default. Individual entities move,
fight, bleed. Full material-interaction combat. All three agent layers active.

**Cinematic mode** (1 tick = 10ms or less): Bullet time. Every arrow is tracked.
Individual weapon swings resolve at high temporal resolution. Same physics, smaller
dt. Useful for observing specific engagements in detail, or (much later) for
resolving fast projectiles like sling stones or bolts precisely.

**Per-region resolution** (future): Different regions of the map can run at
different tick_duration values. A battle runs at tactical while distant provinces
run at strategic. The boundary is a hex ring where entities transition between
resolution levels. Entity state is resolution-independent (continuous positions,
velocity) — only the dt changes.

All physics formulas use `dt` (tick_duration), not tick count. Velocity
integration: `pos += vel * dt`. Bleed: `blood -= bleed_rate * dt`. Stamina
recovery: `stamina += recovery_rate * dt`. This makes resolution switching
seamless — same formulas, different timestep.

### Agent Architecture

Three layers, each at a different time horizon and abstraction level. Layer
cadences scale with tick_duration.

#### Strategy Layer (every ~50 game-seconds)

Each agent personality (Spread, Striker, Turtle) implements this differently.
Receives full observation. Emits strategic directives:

```
StrategicDirective
  SetPosture(Posture)                              // Expand, Defend, Attack, Consolidate
  PrioritizeRegion { center: Axial, priority: f32 }
  SetEconomicFocus(EconomicFocus)                  // Growth, Military, Infrastructure
  RequestStackFormation { size: usize, role: StackRole, equipment_profile: EquipmentProfile }
  SetExpansionTarget { hex: Axial }
```

The strategy layer does NOT issue entity-level commands. It sets intent.

#### Operations Layer (every ~5 game-seconds)

Receives strategic directives + observation. Translates to entity commands:

```
OperationalCommand
  AssignRole { entity: EntityKey, role: Role }
  FormStack { entities: Vec<EntityKey> }
  RouteStack { stack: StackId, destination: Vec3, via: Option<Vec<Vec3>> }
  DisbandStack { stack: StackId }
  EquipEntity { entity: EntityKey, weapon: EntityKey, armor: Vec<EntityKey> }
  BuildStructure { hex: Axial, structure_type: StructureType }
  EstablishSupplyRoute { from: Axial, to: Axial }
  ProducePerson { settlement_hex: Axial }
  ProduceEquipment { settlement_hex: Axial, item_type: EquipmentType }
```

Operations manages:
- Population role assignment (who farms, who trains, who builds)
- Equipment production and distribution (what weapons and armor to make, who gets them)
- Stack formation, composition, and routing
- Infrastructure decisions (roads, structures)
- Supply line management (convoy assignment and routing)
- Settlement expansion (settler dispatch)

All agent personalities share the same operations layer implementation.

#### Tactical Layer (every tick, per stack near enemies)

Activates only for stacks within engagement range of enemies. Receives local
observation. Emits per-entity commands:

```
TacticalCommand
  Attack { attacker: EntityKey, target: EntityKey }
  SetFacing { entity: EntityKey, angle: f32 }
  Block { entity: EntityKey }                      // Enter blocking stance
  Retreat { entity: EntityKey, toward: Vec3 }
  Hold { entity: EntityKey }
  SetFormation { stack: StackId, formation: FormationType }
```

The tactical layer's key responsibility: **weapon-armor matchup reasoning**. It
must assess "my slingers are ineffective against their plate; redirect them to
the unarmored archers on the flank." This uses the damage lookup table.

#### Damage Lookup Table

Agents need fast approximate combat assessment without simulating every impact.

```
DamageLookup: Cache<(WeaponType, ArmorType), ExpectedOutcome>
  ExpectedOutcome {
    wound_rate: f32,      // expected wounds per attack
    avg_severity: f32,    // average wound severity
    stagger_rate: f32,    // probability of stagger
    stamina_drain: f32,   // expected stamina cost to defender
  }
```

Initialized from theoretical material physics. Updated from the agent's
**observation journal** — each observed combat outcome is logged, and empirical
results update the cache. Uses `moka` for time-based eviction (old observations
age out, keeping the table current as equipment and tactics evolve).

The tactical layer consults the lookup table for fast matchup assessment. The sim
runs the full material physics. Over time, the table converges toward ground truth
for the actual combat conditions the agent encounters.

This is the foundation for future learned agents: the observation journal is
training data. When eventually switching to NN-based tactical decisions, the
journal format is the dataset.

### Simulation Tick

```
tick(state, dt):
  // Spatial
  rebuild_spatial_index()             // Recompute hex membership from positions
  compute_territory()                 // From structures + entity presence

  // Agent layers (cadences scale with dt)
  if game_time % 50.0 < dt: run_strategy(state)
  if game_time % 5.0 < dt: run_operations(state)
  run_tactical(state)                 // Every tick for stacks near enemies

  // Simulation
  execute_commands(state)             // Apply agent commands
  produce_resources(state, dt)        // Farmers/workers in structures
  consume_food(state, dt)             // Every person eats
  recover_stamina(state, dt)          // Stamina regeneration (reduced by wounds)
  apply_steering(state, dt)           // Compute steering forces
  integrate_movement(state, dt)       // vel += accel*dt, pos += vel*dt
  resolve_collisions(state)           // Separation, terrain clamping
  advance_projectiles(state, dt)      // Move projectiles, check impacts
  resolve_combat(state, dt)           // Melee impact resolution for entities in range
  apply_bleed(state, dt)              // Wounds drain blood
  update_structures(state, dt)        // Construction progress, decay
  cleanup_dead(state)                 // Remove dead entities, drop equipment
  check_elimination(state)            // Player eliminated when all structures lost

  state.game_time += dt
  state.tick += 1
```

### Frontend: PixiJS Renderer

**Replace SVG (HexBoard.tsx) with PixiJS WebGL (HexCanvas.tsx).** SVG caps at ~5k
elements at 60fps. V3 targets 100k tiles and 10k entities.

Architecture: PixiJS renders the map canvas. SolidJS renders UI panels (score bar,
controls, tooltips, inspector) as HTML overlaid via CSS positioning.

**Rendering layers (bottom to top):**
1. Hex Grid — biome-colored hex sprites, chunked at far zoom
2. Height — terrain shading (lighter = higher) + contour lines
3. Territory — player-colored overlay
4. Infrastructure — roads, walls, structures
5. Entity — unified sprites for all entity types at continuous positions
6. Projectiles — arrows, stones in flight (small, fast-moving sprites)
7. UI Overlay — count badges, wound indicators, facing arrows

**Entity rendering uses continuous positions directly.** No interpolation between
hex centers — the entity's pos is already continuous. The renderer just converts
world-space Vec3 to screen-space Vec2 (projecting z as a height shadow or offset
in future isometric mode).

**Zoom / LOD tiers:**
| Zoom  | Hex rendering      | Entity rendering                    |
|-------|--------------------|-------------------------------------|
| Close | Individual sprites | Individual entities, facing, equipment visible |
| Mid   | Individual sprites | Stack badges ("x15"), structure icons |
| Far   | Chunk textures     | Density heatmap, settlement dots    |

**Spatial indexing:**
- Flatbush (static R-tree) for hex grid viewport queries
- RBush (dynamic R-tree) for entity click/hover queries

### Observation / Protocol

```
EntityInfo
  id: u32
  owner: Option<u8>
  x: f64, y: f64, z: f64       // Continuous position
  hex_q: i32, hex_r: i32       // Derived hex (for convenience)
  facing: Option<f32>
  role: Option<Role>
  blood: Option<f32>
  stamina: Option<f32>
  wound_count: u8
  weapon_type: Option<String>
  armor_type: Option<String>    // Dominant armor (for quick assessment)
  resource_type: Option<ResourceType>
  resource_amount: Option<f32>
  structure_type: Option<StructureType>
  build_progress: Option<f32>
  contains_count: usize
```

Agents see:
- All own entities (full detail including exact wounds, equipment, blood)
- Visible enemy entities (position, owner, approximate wound state, equipment visible)
- Visible structures (type, integrity, material)
- Scouted terrain (permanent)
- Projectiles in flight (position, velocity — can't identify damage properties)

---

## Scope

### V3.0 (ship this)

**Engine:**
- [ ] Single Entity type with component bag replacing Unit/Convoy/Population/Settlement
- [ ] Continuous Vec3 positions with hex grid as derived projection
- [ ] Steering-based movement (seek, arrive, separation, obstacle avoidance)
- [ ] Collision system (entity-entity separation, entity-terrain hard boundary)
- [ ] Body zone system (Head, Torso, LeftArm, RightArm, Legs)
- [ ] Material-interaction damage pipeline (hit location → block check → surface
      lookup → angle of incidence → penetration check → wound → bleed)
- [ ] Weapon and armor entities with material properties
- [ ] Bleed/blood system replacing health bars
- [ ] Stamina system for blocking
- [ ] Unified attack pipeline (melee and ranged as parameter differences)
- [ ] Projectile entities (arrows, sling stones, javelins)
- [ ] Projectile arc physics (parabolic for bows, flat for crossbows)
- [ ] Height effects on combat (hit location bias, range, block angle)
- [ ] Height effects on vision (elevated = further sight, LOS occlusion)
- [ ] Strategy layer (posture, priorities, economic focus, equipment profiles)
- [ ] Operations layer (role assignment, equipment production/distribution, stack
      formation, routing, infrastructure, supply lines)
- [ ] Tactical layer (weapon-armor matchup reasoning, engagement assignment,
      facing, formation, retreat)
- [ ] Damage lookup table with moka caching
- [ ] Observation journal (log combat outcomes for empirical table updates)
- [ ] Resource production via person-in-structure
- [ ] Food consumption per-person
- [ ] Structure construction (build progress, material-dependent integrity)
- [ ] A* pathfinding on hex graph + path smoothing
- [ ] Default tick_duration = 1.0 game-second
- [ ] Small scale: 30×30 map, 2 players, ~500 entities

**Frontend:**
- [ ] PixiJS WebGL renderer replacing SVG HexBoard.tsx
- [ ] Continuous-position entity rendering (no hex-center snapping)
- [ ] Zoom/pan camera with LOD tiers
- [ ] Viewport culling via Flatbush
- [ ] Projectile rendering (arrows in flight)
- [ ] Height visualization (terrain shading + contour lines)
- [ ] Wound indicators at close zoom
- [ ] SolidJS UI panels overlaid on canvas

**Integration:**
- [ ] Round-robin, spectator, replay preserved
- [ ] Updated wire protocol (continuous positions, entity info with wound/equipment)

### V3.1

- Equipment as entities with full material properties (done in V3.0 for basic
  weapons/armor, extended here for variety)
- Carrying capacity tradeoffs — rations vs weapons vs cargo
- Equipment degradation (sharpness decreases with use, armor dents)
- More weapon types (mace, axe, halberd, pike)
- More armor types (scale mail, brigandine, full plate with articulation)

### V3.2

- Multi-resolution time (strategic 1hr / tactical 1s / cinematic 10ms)
- Per-region resolution switching
- Statistical combat resolution for strategic mode
- Flow field pathfinding for mass movement

### V3.3

- Underground layer (tunnels, mines, siege sapping)
- Air layer (observation balloons, messenger birds)
- Z-axis combat (surface-to-air targeting, tunnel collapse)

### Deferred to V4+

- Morale, loyalty, factions, politics
- Commander delegation and field officers
- Technology tree
- Fortification and siege equipment
- Water transport
- NN-based agent learning
- Environmental effects (weather, seasons)

## Resolved Questions

1. **Settlers as entities**: Yes. Population entities physically walk to a new hex
   and build a structure.

2. **Pack animals**: Treated as Person entities with skill=0 in V3.0. Future:
   distinct Animal component with different body zones.

3. **Structure ownership transfer**: Structures become unowned when all owner's
   entities on that hex die. Any player's entities can then occupy.

4. **Population growth**: New person entity spawns at settlement when food surplus
   sustained. Future: more nuanced.

5. **Movement model**: Continuous Vec3 positions with steering behaviors. Hex grid
   is spatial indexing only.

6. **Damage model**: Material-interaction physics with body zones, wounds, and
   bleed. No health bars.

7. **Ranged combat**: In V3.0 scope. Same attack pipeline as melee with projectile
   parameters.

8. **Height**: Modifier on 2D math. Z-coordinate from day one for future air/underground.

9. **Time resolution**: Default 1 tick = 1 second. Multi-resolution in V3.2.

## Security

Game engine, no network attack surface beyond existing WebSocket spectator.
No new endpoints. No PII.

## Verification

### Build and Test
```bash
cargo build --release
cargo test -p simulate-everything-engine
```

### Gameplay Verification
```bash
# ASCII simulation
cargo run --release --bin simulate_everything_cli -- v3bench \
  --agents spread,striker --seeds 0-4 --size 30x30

# Round-robin
curl -s http://localhost:3333/api/v3/rr/status

# Visual — browser to localhost:3333, observe V3 RR game
```

### Architecture Verification
- [ ] All entities use single EntityKey with Vec3 positions
- [ ] Hex membership derived from continuous position (not stored)
- [ ] Movement is continuous — no teleportation between hex centers
- [ ] Combat resolves through material physics — no health bars
- [ ] Projectiles are entities that fly through space with gravity
- [ ] Body zones, wounds, and bleed produce realistic casualty patterns
- [ ] Weapon-armor interactions match physical expectations (sling vs plate = stagger, not wound)
- [ ] Archers fire over friendly lines via parabolic arc
- [ ] Height affects hit location, range, vision
- [ ] Tactical layer uses damage lookup table for matchup decisions
- [ ] Git clean: no stale worktrees, no stale branches, all committed

### Completion State
- All changes committed to main
- No open worktrees
- `cargo build --release` clean
- `cargo test` all green
- Round-robin playable with new systems
- Frontend renders correctly with continuous positions
- Combat produces plausible casualty patterns

## Convention Observations

- The V2 engine module structure (one file per concern) scales well. New modules
  needed: `damage.rs` (impact pipeline), `steering.rs` (movement behaviors),
  `projectile.rs` (projectile physics), `equipment.rs` (weapon/armor properties).

- `moka` crate for agent damage lookup table caching. Time-based eviction keeps
  empirical tables current.

- All physics formulas use `dt` parameter, never raw tick counts. This makes
  multi-resolution switching in V3.2 zero-cost.

## Adjacent Observations

- The material-interaction damage pipeline is domain-general. The same code that
  resolves "iron sword vs leather armor at 30° incidence" resolves "AP shell vs
  sloped steel plate at 45° incidence." The pipeline doesn't know what era it's in.

- The observation journal + moka cache pattern for agent learning is
  domain-general. Any system that observes outcomes and updates its decision
  confidence can use this architecture — the combat agent is one instance.
