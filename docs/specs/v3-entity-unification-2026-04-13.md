# Spec: V3 Entity Unification

## Vision

Replace the V2 engine's four separate entity types (Unit, Convoy, Population, Settlement)
with a single composable Entity primitive. Every mobile thing is an entity. Every static
thing is an entity. Entities contain entities. A soldier is a person with combat skill.
A convoy is a person leading pack animals carrying cargo. A settlement is a structure
containing population. Composition determines capability — unit types emerge from
properties, not from hardcoded categories.

Simultaneously, restructure the agent architecture from "military agents + autonomous
city AI" to three coordinated layers: Strategy (grand vision, posture, priorities),
Operations (theater allocation, logistics, production), and Tactical (per-stack combat
decisions). All three layers are shared infrastructure; agent personalities (Spread,
Striker, Turtle) differentiate at the Strategy layer only. Each layer operates on a
compute budget — allocating intelligence across entities is itself a strategic decision.

Replace the SVG frontend renderer with PixiJS (WebGL) to support the target scale of
100k tiles and 10k entities with zoom/pan, viewport culling, entity interpolation
between ticks, and LOD tiers.

This is V3 of the engine. V2's company-level abstractions (strength 0-100, hex-edge
engagement, aggregate combat) are replaced by individual-level simulation where each
person is one entity. "Strength loss" is people dying. Combat resolves per-individual
with continuous facing angles. Stacking is a visual/command grouping layer, not an
entity abstraction. The game is algorithm competition — the quality of your formation,
retreat, supply, and engagement algorithms is what wins.

## Supersedes

- `docs/plans/v2-remaining-systems.md` — economy, population, convoys, roads, terrain.
  Concepts retained, implementation replaced by entity-based approach.
- `docs/plans/frontend-rendering-overhaul.md` — PixiJS migration. Folded into this spec
  as the frontend half of V3.
- `docs/plans/svg-quick-fixes.md` — SVG renderer improvements. Moot; SVG replaced by PixiJS.
- `docs/plans/agent-intelligence-pipeline.md` — agent improvements. Superseded by
  three-layer architecture.

## Use Cases

### 1. Two armies meet on a hex

A stack of 15 soldiers (individual entities) controlled by Player 0 moves to a hex
occupied by 10 soldiers of Player 1. The tactical layer for each side decides engagement:
which individuals engage which. Combat resolves per-tick per-pair. Individuals die
(entity removed). The stack shrinks. When one side is eliminated or retreats, the
survivors hold the hex.

*Implementation: Individual combat resolution in sim tick. Tactical layer assigns
engagements based on local force assessment. No hex-edge abstraction — individuals
face a direction and fight what's in front of them. Flanking = multiple attackers
from different directions on one defender.*

### 2. Settlement produces soldiers

Population entities (people) live in a settlement structure. The operations layer
assigns some population to "soldier training" role. After training completes, those
population entities transition to soldier status (gain Combatant component, lose
productivity role). Operations forms them into a stack and routes them to a front.

*Implementation: Role assignment changes components on existing entities. No "unit
production cost" — the cost is the person's labor time diverted from farming/building.
The operations layer manages this tradeoff based on strategic directives.*

### 3. Supply convoy runs food to front line

Operations identifies that a forward stack needs food. It assigns a person + pack
animal (both entities) to convoy duty. Population loads food (resource entities) into
the pack animal's container. The convoy moves along roads toward the front. Food
entities are consumed by the convoy members en route — transport eats what it
transports. Remaining food is delivered.

*Implementation: Containment system — pack animal entity contains food entities.
Movement consumes food from contained resources. A* pathfinding with road preference
(already implemented). Convoy = just entities moving together with cargo.*

### 4. Building a structure

Operations directs population to build a farmhouse on an unoccupied hex. Builder
entities travel to the hex and begin construction (incrementing build_progress).
The structure entity exists from the start at 0% progress. Once complete, population
can move in and begin farming. The structure provides shelter (future: weather
protection, defense bonus).

*Implementation: Structure entities with build_progress. Builder entities tick
construction forward. Structure starts providing benefits at 100% progress. Structures
can be damaged (health decreases) and destroyed (entity removed, contents ejected).*

### 5. Strategy agent sets grand direction

The Striker agent's strategy layer observes: strong food surplus, enough military,
enemy settlement visible to the east. It emits: SetPosture(Attack),
PrioritizeRegion(east, 0.9), SetEconomicFocus(Military). Operations receives these
directives and reallocates: shifts population from farming to soldier training, forms
a strike stack, routes it east. Tactical takes over when the stack contacts enemies.

*Implementation: Strategy emits typed directives. Operations consumes them and
translates to entity-level commands. Tactical activates per-stack near enemies.*

### 6. Player observes a battle in detail (future)

A player clicks on a hex where combat is happening. The UI zooms to show individual
soldiers, their facing, their equipment. In the future, the engine can switch to
granular physics-level ticks for this specific battle while the rest of the map
continues at strategic tick rate.

*V1: Frontend shows stacked entities on a hex with count and aggregate health.
Individual rendering is deferred. Multi-resolution switching is deferred.*

## Architecture

### Entity Model

One `SlotMap<EntityKey, Entity>` replaces all four current SlotMaps.

```
Entity
  id: u32                       // Public monotonic ID
  pos: Option<Axial>            // None if contained in another entity
  owner: Option<u8>             // Player owner

  // Containment
  contained_in: Option<EntityKey>
  contains: Vec<EntityKey>

  // Components (presence = capability)
  person: Option<Person>         // Is a living being
  mobile: Option<Mobile>         // Can move
  vision: Option<Vision>         // Can see
  combatant: Option<Combatant>   // Can fight
  resource: Option<Resource>     // Is a material/food quantity
  structure: Option<Structure>   // Is a building/fortification
```

#### Component Details

```
Person
  health: f32          // 0.0 dead, 1.0 full
  combat_skill: f32    // Base ability (everyone can fight; training improves this)
  role: Role           // Current assignment: Idle, Farmer, Worker, Soldier, Builder

Mobile
  speed: f32
  move_cooldown: u8
  destination: Option<Axial>
  route: Vec<Axial>    // Cached A* waypoints

Vision
  radius: i32          // Modified by terrain height, structure bonuses (future)

Combatant
  engaged_with: Vec<EntityKey>
  facing: f32          // Radians, continuous. 0 = east, PI/2 = north. Shield arc centered here.

Resource
  resource_type: ResourceType   // Food, Material
  amount: f32

Structure
  structure_type: StructureType // Farm, Village, City, Depot (future: Wall, Tower, etc.)
  build_progress: f32           // 0.0 - 1.0
  health: f32                   // Damageable
  capacity: usize               // Max contained entities
```

#### Composition Examples

**Soldier**: Entity { person(health=1.0, combat_skill=0.6, role=Soldier), mobile, vision(5), combatant }

**Farmer**: Entity { person(health=1.0, combat_skill=0.1, role=Farmer), mobile, vision(3) }
  - Can be drafted: add combatant, change role to Soldier. Fights poorly (low combat_skill).

**Pack animal**: Entity { person(health=1.0, combat_skill=0.0), mobile(speed=0.7), contains=[food entities] }
  - No vision. Can't fight. Carries cargo.

**Food stockpile**: Entity { resource(Food, 50.0) }
  - No mobility, no vision. Sits in a structure or on the ground.

**Farmhouse**: Entity { structure(Farm, progress=1.0, health=1.0, capacity=10) }
  - Contains farmer entities. Farmers inside produce food.

**Settlement**: Multiple structure entities on adjacent hexes, containing population entities.
  Not a single entity — it's an emergent cluster.

### Stacking

A "stack" is NOT an entity. It's a command grouping: entities on the same hex,
belonging to the same player, grouped by the operations layer. Stacks are identified
by (hex, player) tuple or by explicit stack ID assigned by operations.

The tactical layer reasons about stacks. The frontend renders stacks as grouped
icons with count labels. Individual entities within a stack are not individually
visible in the strategic map view (future: zoom to see individuals).

Moving a stack moves all entities in it. Splitting a stack creates two stacks.
Merging puts entities together.

### Agent Architecture

Three layers, each at a different time horizon and abstraction level.

#### Strategy Layer (~50 ticks, agent personality lives here)

Each agent personality (Spread, Striker, Turtle) implements this differently.
Receives full observation. Emits strategic directives:

```
StrategicDirective
  SetPosture(Posture)                              // Expand, Defend, Attack, Consolidate
  PrioritizeRegion { center: Axial, priority: f32 }
  SetEconomicFocus(EconomicFocus)                  // Growth, Military, Infrastructure
  RequestStackFormation { size: usize, role: StackRole }
  SetExpansionTarget { hex: Axial }
```

The strategy layer does NOT issue entity-level commands. It sets intent.

#### Operations Layer (~5 ticks, shared across all agent personalities)

Receives strategic directives + observation. Translates to entity commands:

```
OperationalCommand
  AssignRole { entity: EntityKey, role: Role }
  FormStack { entities: Vec<EntityKey> }
  RouteStack { stack: StackId, destination: Axial, via: Option<Vec<Axial>> }
  DisbandStack { stack: StackId }
  BuildStructure { hex: Axial, structure_type: StructureType }
  EstablishSupplyRoute { from: Axial, to: Axial }
  ProducePerson { settlement_hex: Axial }          // Population growth directive
```

Operations is the current city_ai promoted and generalized. It manages:
- Population role assignment (who farms, who trains, who builds)
- Stack formation and routing
- Infrastructure decisions (roads, structures)
- Supply line management (convoy assignment and routing)
- Settlement expansion (settler dispatch)

All agent personalities share the same operations layer implementation.
The difference is what strategic directives drive it.

#### Tactical Layer (~1 tick, per stack near enemies)

Activates only for stacks within engagement range of enemies.
Receives local observation (entities on this hex and adjacent hexes).
Emits per-entity combat commands:

```
TacticalCommand
  Engage { attacker: EntityKey, target: EntityKey }
  Disengage { entity: EntityKey }
  SetFacing { entity: EntityKey, angle: f32 }    // Continuous radians
  Retreat { entity: EntityKey, toward: Axial }
  Hold { entity: EntityKey }                       // Stay and fight
```

The tactical layer assesses local force ratio, decides engagements,
manages retreat. V1 implementation is simple (engage when advantaged,
retreat when outnumbered). The architecture supports sophisticated
tactical AI in the future.

### Combat Geometry

**Hex grid for position, continuous angles for combat.**

Entities occupy hexes (discrete positions for spatial indexing at 100k tile scale).
Facing is continuous (f32 radians) — not discretized to 6 hex directions. Attack
angles are computed from hex center-to-center vectors via atan2. Shield arcs are
continuous cones centered on facing direction.

This hybrid gives: O(1) spatial queries from hex grid, realistic combat geometry
from continuous angles, and natural support for V5 ranged combat (projectile
trajectories use the same atan2 angles).

**Sub-hex rendering interpolation** solves the "teleporting between tiles" visual
problem. Entities at hex A moving to hex B are rendered at the interpolated position
between hex centers based on movement cooldown progress. Zero simulation cost —
purely a frontend lerp.

### Combat Resolution

Per-individual, per-tick. No aggregate strength abstractions. No engagement lock —
combat is moment-to-moment, not a persistent state.

Each tick, for each attacker/defender pair in contact:
1. Compute attack angle: `atan2(defender.y - attacker.y, defender.x - attacker.x)`
   using hex center pixel coordinates
2. Compute facing difference: `|attack_angle - defender.facing|`
3. Apply facing modifier based on shield arc coverage:
   - Within shield arc (front): damage × 0.3 (shield blocks most)
   - Within PI/2 of facing (side): damage × 0.7
   - Beyond PI/2 (rear): damage × 1.5
4. `damage = attacker.combat_skill * DAMAGE_PER_TICK * facing_modifier`
5. Apply to defender health. At 0, entity dies.

**No engagement lock.** V2's "engaged on hex edge, 30% penalty to disengage" is
gone. Retreating under attack means taking hits to the back (1.5x damage). The
cost of retreat is the damage you take while your back is turned, not an arbitrary
penalty. Attackers can also freely walk away.

**No 1/sqrt(N) formula.** V2's multi-edge effectiveness penalty was abstracting
"a unit can only face one direction." With individual facing, this IS the simulation.
A defender facing north who is attacked from north and south can only block the
north attacker — the south attacker gets the rear bonus naturally.

**Flanking emerges from geometry:** Two attackers from different directions means the
defender can only face one. The tactical layer's job is to coordinate approaches so
attackers arrive from multiple directions simultaneously.

**Formation is the core tactical mechanic.** A line of defenders facing the same
direction creates a wall — their shield arcs collectively cover the front. A bad
formation algorithm leaves gaps. A good one overlaps arcs. Historical formations
(phalanx, shield wall, wedge) emerge as algorithmically optimal configurations for
specific combat situations.

### Simulation Tick

```
tick(state):
  rebuild_spatial_index()
  compute_territory()           // From structures + entity presence
  
  // Agent layers (at their cadences)
  if tick % 50 == 0: run_strategy(state)
  if tick % 5 == 0: run_operations(state)
  run_tactical(state)           // Every tick for engaged stacks
  
  // Simulation
  execute_commands(state)       // Apply agent commands
  produce_resources(state)      // Farmers/workers in structures generate resources
  consume_food(state)           // Every person eats (from local resources or carried)
  resolve_combat(state)         // Per-individual damage
  move_entities(state)          // Pathfinding and movement
  update_structures(state)      // Construction progress, damage
  cleanup_dead(state)           // Remove dead entities
  check_elimination(state)      // Player eliminated when all structures lost
  
  state.tick += 1
```

### Frontend: PixiJS Renderer

**Replace SVG (HexBoard.tsx) with PixiJS WebGL (HexCanvas.tsx).** SVG caps at ~5k
elements at 60fps. V3 targets 100k tiles, 10k entities. PixiJS with sprite batching
handles this.

Architecture: PixiJS renders the map canvas. SolidJS renders UI panels (score bar,
controls, tooltips) as HTML overlaid via CSS positioning.

**Rendering layers (bottom to top):**
1. Hex Grid — biome-colored hex sprites, chunked at far zoom
2. Territory — player-colored overlay, recomputed on territory change
3. Infrastructure — roads as line segments
4. Entity — unified sprites for all entity types (people, structures, resources)
5. UI Overlay — count badges, health indicators, facing arrows

**Entity interpolation:** Buffer two server ticks. Each render frame, lerp entity
positions: `pos = lerp(state[t-1].pos, state[t].pos, elapsed / tickInterval)`.
Eliminates hex-to-hex teleporting.

**Zoom / LOD tiers:**
| Zoom  | Hex rendering      | Entity rendering                    |
|-------|--------------------|-------------------------------------|
| Close | Individual sprites | Individual entities, facing arrows  |
| Mid   | Individual sprites | Stack badges ("x15"), structure icons|
| Far   | Chunk textures     | Density heatmap, settlement dots    |

**Spatial indexing:**
- Flatbush (static R-tree) for hex grid viewport queries
- RBush (dynamic R-tree) for entity click/hover queries

**What's NOT in V1 frontend:**
- Texture atlas optimization (use simple colored shapes first)
- Chunk pre-rendering for far zoom (20x20 map doesn't need it)
- Delta sync protocol (full snapshot per tick at V1 scale)
- Animated route lines

### Observation / Protocol

```
EntityInfo (replaces UnitInfo + ConvoyInfo + PopulationInfo)
  id: EntityKey
  owner: Option<u8>
  q: i32, r: i32
  health: Option<f32>           // If person
  role: Option<Role>            // If person
  combat_skill: Option<f32>     // If combatant
  engaged: bool
  resource_type: Option<ResourceType>  // If resource
  resource_amount: Option<f32>
  structure_type: Option<StructureType> // If structure
  contains_count: usize         // How many entities inside
```

Agents see:
- All own entities (full detail)
- Visible enemy entities (position, owner, health, combat status)
- Visible structures (type, health)
- Scouted terrain (permanent)

## Scope

### V1 (ship this)

**Engine:**
- [ ] Single Entity type with component bag replacing Unit/Convoy/Population/Settlement
- [ ] Containment system (entities in entities)
- [ ] Individual-level entities (1 person = 1 entity)
- [ ] Stacking as visual/command grouping
- [ ] Per-individual combat resolution with continuous facing (f32 radians)
- [ ] Strategy layer (posture, priorities, economic focus)
- [ ] Operations layer (role assignment, stack formation, routing, infrastructure)
- [ ] Tactical layer (per-stack engagement, facing, formation decisions)
- [ ] Resource production via person-in-structure
- [ ] Food consumption per-person
- [ ] Structure construction (build progress)
- [ ] Structure damage and destruction
- [ ] A* pathfinding (reuse existing)
- [ ] Small scale: 20x20 map, 2 players, ~200-300 entities, 10 ticks/sec

**Frontend:**
- [ ] PixiJS WebGL renderer replacing SVG HexBoard.tsx
- [ ] Entity interpolation between ticks (lerp positions)
- [ ] Zoom/pan camera with LOD tiers (close: individuals, mid: stacks, far: density)
- [ ] Viewport culling via Flatbush spatial index
- [ ] Unified entity rendering (no separate unit/convoy/settlement visuals)
- [ ] SolidJS UI panels overlaid on canvas (score bar, controls)

**Integration:**
- [ ] All existing functionality preserved (round-robin, spectator, replay)
- [ ] Updated wire protocol (SpectatorEntity replaces SpectatorUnit/SpectatorConvoy)

### Deferred

- Body slots and equipment as entities (V3.1)
- Carrying capacity tradeoffs — rations vs weapons vs cargo (V3.1)
- Training and skill progression (V3.2)
- Injury system and body part damage (V3.2)
- Durability and wear on items (V3.2)
- Physics-level resolution layer (V3.3)
- Multi-resolution simulation switching (V3.3)
- Data-driven aggregate resolution (V3.4)
- Environmental effects (weather, clothing) (V4)
- Morale, loyalty, factions, politics (V4)
- Technology tree unlocking new compositions (V4)
- Commander delegation and field officers (V4)
- Ministers and advisors (V4)
- Texture atlas and chunk pre-rendering (scale optimization)
- Delta sync protocol (scale optimization)
- Large-scale maps (100x100+) with optimized entity processing (ongoing)

## Security

Game engine, no network attack surface beyond existing WebSocket spectator.
No new endpoints. No PII.

## Verification

### Build and Test
```bash
cargo build --release
cargo test -p simulate-everything-engine -- v2  # (becomes v3)
```

### Gameplay Verification
```bash
# ASCII simulation — entities visible as individual counts
cargo run --release --bin simulate_everything_cli -- v2bench \
  --agents spread,striker --seeds 0-4 --size 20x20

# Round-robin — all three agent layers running
curl -s http://localhost:3333/api/v2/rr/status

# Visual verification — frontend renders entity stacks
# Open browser to localhost:3333, observe V2 RR game
```

### Architecture Verification
- [ ] No remaining references to old Unit/Convoy/Population/Settlement types
- [ ] All entities use single EntityKey
- [ ] Containment works: create structure, put person inside, verify vision/production
- [ ] Combat resolves per-individual: two stacks meet, individuals die, stack shrinks
- [ ] Strategy layer emits directives, operations translates, tactical engages
- [ ] Transport eats cargo: food convoy loses food en route (even without equipment system)
- [ ] Git clean: no stale worktrees, no stale branches, all committed

### Completion State
- All changes committed to main
- No open worktrees
- No stale branches
- `cargo build --release` clean
- `cargo test` all green
- Round-robin playable with new entity system
- Frontend renders correctly

## Files Modified

### Engine (complete rewrite of most files)
- `crates/engine/src/v2/state.rs` — Entity struct replaces Unit/Convoy/Population/Settlement
- `crates/engine/src/v2/mod.rs` — Updated constants, removed company-level constants
- `crates/engine/src/v2/sim.rs` — New tick loop with per-individual resolution
- `crates/engine/src/v2/combat.rs` — Individual combat with facing
- `crates/engine/src/v2/agent.rs` — Three-layer architecture, strategy/operations/tactical
- `crates/engine/src/v2/city_ai.rs` — Becomes operations layer (may rename)
- `crates/engine/src/v2/directive.rs` — Strategic/Operational/Tactical command types
- `crates/engine/src/v2/observation.rs` — EntityInfo replaces UnitInfo/ConvoyInfo/PopInfo
- `crates/engine/src/v2/spectator.rs` — SpectatorEntity replaces SpectatorUnit/SpectatorConvoy
- `crates/engine/src/v2/replay.rs` — EntitySnapshot replaces UnitSnapshot/ConvoySnapshot
- `crates/engine/src/v2/vision.rs` — Simplified: vision from individual entities
- `crates/engine/src/v2/mapgen.rs` — Generate individual entities, structures
- `crates/engine/src/v2/pathfinding.rs` — Mostly unchanged (A* reused)
- `crates/engine/src/v2/integration_tests.rs` — Complete rewrite for entity system

### Frontend (PixiJS migration)
- `frontend/src/HexCanvas.tsx` — NEW: PixiJS WebGL renderer replacing HexBoard.tsx
- `frontend/src/HexBoard.tsx` — DELETED: SVG renderer replaced
- `frontend/src/v2types.ts` — V3EntitySnapshot replaces V2UnitSnapshot/V2ConvoySnapshot
- `frontend/src/V2SimApp.tsx` — Wire HexCanvas instead of HexBoard
- `frontend/src/V2App.tsx` — Wire HexCanvas instead of HexBoard
- `frontend/src/styles/app.css.ts` — Canvas layout, UI panel positioning
- `frontend/package.json` — Add pixi.js, flatbush, rbush dependencies

### CLI
- `crates/cli/src/bin/v2_scaling_bench.rs` — Updated for entity system

### Web
- `crates/web/src/v2_protocol.rs` — Updated wire types
- `crates/web/src/v2_roundrobin.rs` — May need agent initialization changes

## Convention Observations

- The V2 engine module structure (one file per concern) scales well for this refactor.
  city_ai.rs naturally becomes the operations layer. agent.rs naturally splits into
  strategy + tactical. No new module organization needed.

- The SlotMap pattern (typed keys into arenas) works for the unified entity model.
  One SlotMap<EntityKey, Entity> with component data inline is the simplest approach.
  If performance requires it later, components can be split into separate SlotMaps
  (SoA pattern) keyed by the same EntityKey.

## Adjacent Observations

- **Individual-level simulation at scale is a Rust performance showcase.** If we hit
  10,000+ entities at 10 ticks/sec, that's a compelling demo of what Rust enables
  for game simulation that would be painful in other languages.

- **The three-layer agent architecture (strategy/operations/tactical) is a general
  pattern that could apply to [redacted]'s AI operating partner.** A domain-specific AI
  has strategic goals (grow practice, improve patient outcomes), operational execution
  (scheduling, billing, supply ordering), and tactical responses (handle this specific
  patient interaction). Same layered reasoning, different domain.

## Open Questions

1. **Settlers as entities**: When a settlement "sends settlers," is that population
   entities physically walking to a new hex and building a structure? (Assumed: yes.)

2. **Pack animals**: Are pack animals separate entity types (with Person component but
   combat_skill=0), or should there be an Animal component distinct from Person?
   V1 can treat them as persons with low stats. Future: distinct Animal component.

3. **Structure ownership transfer**: If all defenders die, does the attacker capture
   structures or must they be explicitly claimed? (Suggest: structures become unowned
   when all owner's entities on that hex die. Any player's entities can then occupy.)

4. **Population growth**: Currently driven by food surplus. With individual entities,
   is growth "new person entity spawns at settlement" or something more biological?
   V1: spawn new person entity when food surplus sustained. Future: more nuanced.
