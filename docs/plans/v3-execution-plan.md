# V3 Execution Plan — Dependency Graph and Agent Prompts

Created: 2026-04-13
Source spec: `docs/specs/v3-entity-unification-2026-04-13.md`

## Dependency Graph

```
                    ┌─────────────────────────────────────────────────┐
                    │              V2 CLEANUP (do first)              │
                    │                                                 │
                    │  V2-1: V2App controls (pause, compaction, play) │
                    │  V2-2: Live status WS streaming                 │
                    │         (both port directly to V3)              │
                    └─────────────────────────────────────────────────┘

═══════════════════════════════════════════════════════════════════════

                    ┌──────────────────────────┐
                    │  A1: Entity Model + State │
                    │                          │
                    │  Entity struct            │
                    │  Component bags           │
                    │  Containment system       │
                    │  Mapgen spawns entities   │
                    │  SlotMap<EntityKey,Entity> │
                    └────────────┬─────────────┘
                                 │
                 ┌───────────────┼───────────────────────┐
                 │               │                       │
                 ▼               ▼                       ▼
    ┌────────────────┐  ┌───────────────┐  ┌─────────────────────┐
    │  A2: Sim Tick  │  │ A3: Combat    │  │ D1: Wire Protocol   │
    │                │  │               │  │                     │
    │  Resource prod │  │ Individual    │  │ SpectatorEntity     │
    │  Food consume  │  │ Facing-based  │  │ v2_protocol.rs      │
    │  Structure     │  │ Shield arcs   │  │ v2types.ts          │
    │  construction  │  │ Flanking      │  │ EntityInfo           │
    └───────┬────────┘  └──────┬────────┘  └──────────┬──────────┘
            │                  │                       │
            │                  │              ┌────────┴────────┐
            ▼                  │              ▼                 ▼
    ┌───────────────┐          │     ┌──────────────┐  ┌──────────────┐
    │ B2: Ops Layer │          │     │ D2: RR Loop  │  │ C2: Entity   │
    │               │          │     │              │  │ Rendering    │
    │ Role assign   │          │     │ Agent init   │  │              │
    │ Stack form    │          │     │ Entity snaps │  │ Sprites      │
    │ Routing       │          │     │ Replay       │  │ Stack badges │
    │ Supply lines  │          │     └──────┬───────┘  └──────┬───────┘
    │ Infrastructure│          │            │                  │
    └───────┬───────┘          │            ▼                  ▼
            │                  │     ┌──────────────┐  ┌──────────────┐
            ▼                  ▼     │ D3: Live WS  │  │ C3: Interp   │
    ┌───────────────┐  ┌────────────┐│ Status Port  │  │              │
    │ B3: Strategy  │  │B4: Tactical││              │  │ Tick buffer  │
    │               │  │            ││ Port V2-2    │  │ Lerp pos     │
    │ Spread impl   │  │ Engage     ││ to V3 proto  │  │ Smooth move  │
    │ Striker impl  │  │ Facing     │└──────────────┘  └──────┬───────┘
    │ Turtle impl   │  │ Formation  │                         │
    └───────────────┘  │ Retreat    │                         ▼
                       └────────────┘                  ┌──────────────┐
                                                       │ C4: LOD +    │
                                                       │ Culling      │
                                                       │              │
                                                       │ Flatbush     │
                                                       │ RBush        │
                                                       │ Chunk tex    │
                                                       └──────────────┘

    ┌─────────────────────────────────────────────┐
    │  B1: Agent Layer Types (can start with A1)  │
    │                                             │
    │  StrategicDirective                         │
    │  OperationalCommand                         │
    │  TacticalCommand                            │
    │  Strategy / Operations / Tactical traits    │
    └─────────────────────────────────────────────┘

    ┌─────────────────────────────────────────────┐
    │  C1: PixiJS Scaffold (fully independent)    │
    │                                             │
    │  PixiJS Application + camera                │
    │  Hex grid with biome colors                 │
    │  SolidJS overlay panels                     │
    │  Wire to existing V2 protocol               │
    └─────────────────────────────────────────────┘
```

### Parallelism summary

| Phase | After | Parallelizable with |
|-------|-------|---------------------|
| V2-1 | — | V2-2, C1 |
| V2-2 | — | V2-1, C1 |
| A1 | V2 cleanup | C1 |
| B1 | — (types only) | A1, C1 |
| A2 | A1 | A3, D1, C1 |
| A3 | A1 | A2, D1, C1 |
| D1 | A1 | A2, A3 |
| B2 | A2 | B4, D2, C2 |
| B4 | A3 | B2, D2, C2 |
| B3 | B2 | C3, D3 |
| D2 | D1 | B2, B4, C2 |
| D3 | D2 | B3, C3 |
| C1 | — | everything |
| C2 | D1 | B2, B4, D2 |
| C3 | C2 | B3, D3 |
| C4 | C3 | — |

### Critical path

```
A1 → A2 → B2 → B3     (engine + agent personality)
A1 → A3 → B4           (combat + tactical)
A1 → D1 → D2 → D3     (protocol + web)
C1 → C2 → C3 → C4     (frontend, joins at C2 needing D1)
```

Longest path: **A1 → A2 → B2 → B3** (entity → sim → operations → strategy).
Frontend is off critical path unless C2 stalls waiting for D1.

---

## Agent Prompts

### V2-1: Finish V2App Controls

```
## Task: Finish V2App.tsx controls

Project: ~/dev/personal/simulate_everything
Key files:
- frontend/src/V2App.tsx — main V2 round-robin app
- frontend/src/HexBoard.tsx — SVG renderer (read-only, don't modify)
- crates/web/src/v2_roundrobin.rs — server RR loop (has pause/resume/reset endpoints)
- docs/handoff-ui-debug-apr13.md — handoff with exact status of each item

Context: A prior session started but did not finish several V2App.tsx features.
The handoff doc has full details. The changes are in V2App.tsx on main — they're
partially implemented but never built or tested.

### What to finish

1. **Server pause button** — State (serverPaused, toggleServerPause) is wired up
   but the button JSX was never added. Add it in the speed controls area. It calls
   POST /api/v2/rr/pause and POST /api/v2/rr/resume.

2. **Frame compaction** — Logic was added to keep max 600 frames and compact older
   frames by keeping every 5th. Needs testing: verify scrubbing still works after
   compaction, no off-by-one on viewIdx.

3. **Play at speed** — Reworked playback interval with a `playing` signal separate
   from `following`. Play button should advance at tickMs speed, not jump to live.
   Skip-to-end (⏭) sets following(true). Needs testing.

### What NOT to do

- Do not touch HexBoard.tsx — SVG renderer is being replaced
- Do not fix the adjacency standoff engine bug — V3 removes that combat model
- Do not add death animations — requires HexBoard changes that are moot

### Verification

Build the frontend:
```bash
cd frontend && bun run build
```

Then verify the service is running:
```bash
systemctl is-active simulate_everything
```

If running, restart to pick up the new frontend build:
```bash
sudo systemctl restart simulate_everything
```

Open http://localhost:3333/v2/rr in a browser and verify:
- Play/pause/step controls work correctly
- Play advances at server tick speed, not jumping to live
- Skip-to-end goes to live following mode
- Server pause stops new ticks from the server
- Server resume continues
- Restart button starts a new game
- Long games (2000+ ticks) don't freeze the tab (frame compaction works)

Commit when done.
```

### V2-2: Live Status WS Streaming

```
## Task: Replace HTTP status polling with WebSocket status messages

Project: ~/dev/personal/simulate_everything
Spec: docs/plans/v2-rr-live-status-hover-followup.md (read this first — it's the
complete implementation plan with acceptance criteria)

Key files:
- crates/web/src/v2_protocol.rs — WS message types
- crates/web/src/v2_roundrobin.rs — RR loop, spectator broadcast
- frontend/src/V2App.tsx — app shell, currently polls /api/v2/rr/status every 2s
- frontend/src/v2types.ts — TypeScript types
- docs/architecture.md — update V2 WebSocket Protocol section

### Summary

Add a `v2_rr_status` WebSocket message to the existing ws/v2/rr socket. Broadcast
it from the RR loop on: tick, pause, resume, reset, config change, flag, capture
start/stop. Include it in spectator catchup. Remove the 2-second HTTP status poll
from the frontend.

The plan also specifies tick-gated hover (pendingHoverHex resolved on next tick
instead of on every mousemove). Implement this too.

Keep /api/v2/rr/status as a debug endpoint. Keep review list fetching as explicit
HTTP calls (initial load, after flag, after capture, after delete).

### Verification

```bash
cargo test -p simulate-everything-web
cd frontend && bun run build
```

Then restart the service and verify in browser:
- Capturable range updates live without visible lag
- Pausing RR still updates status immediately
- Rapid mouse movement over hexes doesn't cause UI jank during live play
- Paused hover is still immediate
- Start/stop capture updates status without waiting for poll
- New tab joining gets current status on connect

Commit when done. Update docs/architecture.md V2 WebSocket Protocol table
with the new message type.
```

### C1: PixiJS Scaffold

```
## Task: Scaffold PixiJS WebGL renderer for V2 hex board

Project: ~/dev/personal/simulate_everything
Spec: docs/specs/v3-entity-unification-2026-04-13.md (read the "Frontend: PixiJS
Renderer" section)

Key files to read first:
- frontend/src/HexBoard.tsx — current SVG renderer, understand what it renders
- frontend/src/V2App.tsx — how HexBoard is wired in
- frontend/src/v2types.ts — frame/unit/hex data types
- frontend/package.json — current dependencies
- crates/engine/src/v2/hex.rs — hex coordinate system (axial, flat-top, even-r offset)
- docs/research/hexboard-rendering-analysis.md — complete analysis of current renderer

### Goal

Create HexCanvas.tsx as a PixiJS WebGL replacement for HexBoard.tsx. Wire it into
V2App.tsx behind a flag or as the default. The existing V2 protocol is the data
source — this renders the same data as HexBoard but with WebGL.

### Scope

Phase 1 only — get a working hex grid on screen:

1. Add pixi.js (v8) to frontend/package.json
2. Create frontend/src/HexCanvas.tsx:
   - PixiJS Application with a canvas element
   - Camera: zoom (mouse wheel) and pan (click-drag)
   - Render hex grid from frame terrain data as colored hex sprites or graphics
   - Render territory overlay (player colors)
   - Render units as simple colored circles with count labels
   - Render the score bar and basic game info as SolidJS HTML overlay
3. Wire HexCanvas into V2App.tsx (replace HexBoard import)
4. Handle resize/cleanup properly (PixiJS destroy on unmount)

### What NOT to do

- No entity interpolation yet (C3)
- No LOD tiers or viewport culling yet (C4)
- No texture atlas — use simple Graphics objects
- No new protocol messages — render from existing V2 snapshot data
- Do not delete HexBoard.tsx yet — keep it around until HexCanvas is stable

### Hex geometry reference

The engine uses axial coordinates (q, r) with flat-top hexes and even-r offset
storage. HexBoard.tsx has the pixel conversion math — port it. Key formulas:

- Flat-top hex: width = size * 2, height = size * sqrt(3)
- Pixel x = size * 3/2 * q
- Pixel y = size * sqrt(3) * (r + 0.5 * (q & 1))

### Verification

```bash
cd frontend && bun install && bun run build
```

Open http://localhost:3333/v2/rr in browser:
- Hex grid renders with terrain colors
- Territory ownership visible
- Units visible as colored markers
- Zoom in/out with mouse wheel works smoothly
- Pan with click-drag works
- Score/game info visible
- No console errors
- Performance: 60fps on a 30x30 grid

Commit when done.
```

### A1: Entity Model and State

```
## Task: Implement V3 unified Entity model

Project: ~/dev/personal/simulate_everything
Spec: docs/specs/v3-entity-unification-2026-04-13.md (read the full "Architecture"
section — Entity Model, Component Details, Composition Examples, Stacking)

Key files:
- crates/engine/src/v2/state.rs — current state (Unit, Convoy, Population, Settlement)
- crates/engine/src/v2/mod.rs — constants
- crates/engine/src/v2/mapgen.rs — spawns units/population/settlements
- crates/engine/src/v2/sim.rs — tick loop (read, don't rewrite yet)
- crates/engine/src/v2/observation.rs — current observation types
- crates/engine/Cargo.toml — dependencies (slotmap already in use)

### Goal

Replace the four separate entity SlotMaps (units, convoys, population, settlements)
with a single SlotMap<EntityKey, Entity> using component bags. Migrate mapgen to
spawn entities. Provide accessor methods so the existing sim loop can be adapted
incrementally.

### Scope

1. Define the Entity struct with component bags exactly as specified:
   - Person, Mobile, Vision, Combatant, Resource, Structure components
   - Containment (contained_in, contains)
   - Public monotonic id, optional pos, optional owner

2. Replace GameState's four SlotMaps with one:
   ```rust
   pub entities: SlotMap<EntityKey, Entity>,
   ```

3. Add convenience query methods on GameState:
   - units() -> iterator over entities with Combatant + Person + Mobile
   - structures() -> iterator over entities with Structure
   - resources_at(hex) -> iterator over Resource entities at a hex
   - entities_at(hex) -> iterator over all entities at a hex
   - contained_in(key) -> iterator over entities inside another

4. Update mapgen.rs to spawn entities instead of Unit/Convoy/Population/Settlement:
   - General = Entity { person, mobile, vision, combatant } with a flag or special structure
   - Starting soldiers = Entity { person(role=Soldier), mobile, vision, combatant }
   - Starting population = Entity { person(role=Farmer/Worker/Idle) } contained in a
     settlement structure entity
   - Settlement = Entity { structure(Farm/Village/City), capacity }

5. Update SpatialIndex to work with EntityKey instead of UnitKey.

6. DO NOT rewrite sim.rs yet. Add a thin compatibility layer if needed so the
   existing tick loop can access entity data through the old field names. The sim
   rewrite is A2/A3.

7. DO NOT change the wire protocol yet (D1). DO NOT change agents yet (B*).

### Testing

All existing V2 engine tests must pass after this change. The entity model is a
data migration — behavior should be identical. Add new tests for:
- Entity creation with various component combinations
- Containment: put person in structure, verify contained_in/contains
- Query methods return correct subsets
- Mapgen produces valid entity state

```bash
cargo test -p simulate-everything-engine
cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --seeds 0-4 --ticks 500 --size 30x30 --agents spread,striker
```

Games must still play to completion with the same winners for the same seeds.

Commit when done.
```

### A2: Sim Tick Rewrite (Economy)

```
## Task: Rewrite V2 sim tick for entity-based economy

Project: ~/dev/personal/simulate_everything
Spec: docs/specs/v3-entity-unification-2026-04-13.md (Simulation Tick section)
Depends on: A1 (entity model) must be complete — Entity struct with component bags,
single SlotMap, mapgen spawning entities.

Key files:
- crates/engine/src/v2/sim.rs — current tick loop (rewrite target)
- crates/engine/src/v2/state.rs — Entity with components (from A1)
- crates/engine/src/v2/mod.rs — constants
- crates/engine/src/v2/directive.rs — current directives (extend for new commands)

### Goal

Rewrite the sim tick to operate on individual entities instead of aggregate
Unit/Population/Convoy types. Economy is per-person: farmers in structures produce
food, workers produce material, every person eats, structures have build progress.

### Scope

1. Resource production: Person entities with role=Farmer inside a Structure entity
   produce food proportional to terrain fertility. Workers produce material. Output
   goes to hex stockpile (already exists from V2).

2. Food consumption: Every Person entity consumes food from local hex stockpile.
   If stockpile empty, health degrades (starvation).

3. Structure construction: Person entities with role=Builder at a hex with a
   Structure entity at build_progress < 1.0 increment build_progress per tick.
   Structure provides benefits only at 100%.

4. Convoy movement: Entities with Resource component contained in a Mobile entity
   move along routes. Transport eats cargo en route.

5. Role assignment: New directive AssignRole { entity, role } changes a Person's
   role. Soldier training: combat_skill increases while role=Soldier.

6. Unit production: Trained soldiers (combat_skill > threshold) gain Combatant +
   Vision components, becoming military entities. Population consumed.

7. Territory: computed from structure ownership + entity presence, same as V2 but
   from entity positions.

### What NOT to do

- Combat resolution (that's A3)
- Agent changes (that's B2/B3)
- Wire protocol changes (that's D1)

### Verification

```bash
cargo test -p simulate-everything-engine
```

Write tests for:
- Farmer-in-structure produces food
- Person eats food, starves if none
- Builder advances structure progress
- Role assignment works
- Soldier training increases combat_skill
- Convoy delivers resources

Commit when done.
```

### A3: Combat Resolution

```
## Task: Implement individual facing-based combat

Project: ~/dev/personal/simulate_everything
Spec: docs/specs/v3-entity-unification-2026-04-13.md (Combat Resolution and
Combat Geometry sections)
Depends on: A1 (entity model) — entities have Combatant component with facing
and engaged_with fields.

Key files:
- crates/engine/src/v2/combat.rs — current edge-based engagement (replace)
- crates/engine/src/v2/state.rs — Entity with Combatant component
- crates/engine/src/v2/sim.rs — tick loop combat phase
- crates/engine/src/v2/hex.rs — hex coordinate math, pixel conversion

### Goal

Replace V2's edge-based engagement lock system with individual per-tick combat.
No engagement lock — combat is moment-to-moment. Facing is continuous (f32 radians).
Damage depends on attack angle relative to defender facing.

### Scope

1. Remove the engagement system (EngagementMap, edge-based locks, disengage penalty).

2. Per-tick combat resolution for entities on the same hex or adjacent hexes:
   - Attack angle: atan2 from attacker hex center to defender hex center
   - Facing difference: |attack_angle - defender.facing|
   - Shield arc modifiers:
     - Front (within shield arc): damage * 0.3
     - Side (within PI/2): damage * 0.7
     - Rear (beyond PI/2): damage * 1.5
   - damage = attacker.combat_skill * DAMAGE_PER_TICK * facing_modifier
   - Apply to defender Person.health. At 0, entity dies.

3. No engagement lock. Retreating means taking rear damage (1.5x). The cost of
   retreat is organic, not an arbitrary penalty.

4. Flanking: multiple attackers from different directions naturally overwhelm a
   defender who can only face one direction.

5. Entity cleanup: dead entities (health <= 0) removed in cleanup phase.

6. Update sim.rs resolve_combat() to use the new system.

### What NOT to do

- Tactical layer AI (that's B4) — for now, entities engage whatever is adjacent
  using a simple heuristic (face nearest enemy, attack)
- Formation logic — that's B4
- Wire protocol changes — that's D1

### Verification

```bash
cargo test -p simulate-everything-engine
```

Write tests for:
- Head-on combat: equal units, symmetric damage
- Flanking: two attackers from different angles, defender takes more total damage
- Rear attack: 1.5x modifier applied
- Shield arc: frontal attack reduced to 0.3x
- Retreat under fire: moving unit takes rear damage
- Death: entity removed when health hits 0
- Same-hex combat (melee): attack angle from facing, not hex direction

Commit when done.
```

### B1: Agent Layer Types

```
## Task: Define three-layer agent architecture types

Project: ~/dev/personal/simulate_everything
Spec: docs/specs/v3-entity-unification-2026-04-13.md (Agent Architecture section)
Can start alongside A1 — this is types and traits only, no behavior.

Key files:
- crates/engine/src/v2/agent.rs — current Agent trait
- crates/engine/src/v2/directive.rs — current Directive enum
- crates/engine/src/v2/state.rs — Entity, EntityKey

### Goal

Define the type system for the three-layer agent architecture: Strategy, Operations,
Tactical. Each layer has its own directive/command type and trait. Agent personalities
differentiate only at the Strategy layer.

### Scope

1. Define StrategicDirective enum:
   - SetPosture(Posture) where Posture = Expand | Defend | Attack | Consolidate
   - PrioritizeRegion { center: Axial, priority: f32 }
   - SetEconomicFocus(EconomicFocus) where EconomicFocus = Growth | Military | Infrastructure
   - RequestStackFormation { size: usize, role: StackRole }
   - SetExpansionTarget { hex: Axial }

2. Define OperationalCommand enum:
   - AssignRole { entity: EntityKey, role: Role }
   - FormStack { entities: Vec<EntityKey> }
   - RouteStack { stack: StackId, destination: Axial }
   - DisbandStack { stack: StackId }
   - BuildStructure { hex: Axial, structure_type: StructureType }
   - EstablishSupplyRoute { from: Axial, to: Axial }
   - ProducePerson { settlement_hex: Axial }

3. Define TacticalCommand enum:
   - Engage { attacker: EntityKey, target: EntityKey }
   - Disengage { entity: EntityKey }
   - SetFacing { entity: EntityKey, angle: f32 }
   - Retreat { entity: EntityKey, toward: Axial }
   - Hold { entity: EntityKey }

4. Define traits:
   - StrategyLayer: fn plan(&mut self, obs: &Observation) -> Vec<StrategicDirective>
   - OperationsLayer: fn execute(&mut self, obs: &Observation, directives: &[StrategicDirective]) -> Vec<OperationalCommand>
   - TacticalLayer: fn decide(&mut self, local_obs: &LocalObservation) -> Vec<TacticalCommand>

5. Define StackId, StackRole, and the Stack bookkeeping struct (entities grouped
   by hex+player, with an assigned role like Assault, Garrison, Scout, Supply).

6. Keep the existing Agent trait as a wrapper that owns all three layers and
   dispatches at the right cadences (strategy every ~50 ticks, operations every
   ~5 ticks, tactical every tick for engaged stacks).

### What NOT to do

- Implement any behavior — this is types and traits only
- Change the existing SpreadAgent/StrikerAgent — they'll be migrated in B2/B3
- Change the sim loop — directive application comes later

### Verification

```bash
cargo test -p simulate-everything-engine
cargo build -p simulate-everything-engine
```

Types compile. Existing tests still pass (nothing behavioral changed).

Commit when done.
```

### D1: Wire Protocol Update

```
## Task: Update V2 wire protocol for unified entities

Project: ~/dev/personal/simulate_everything
Depends on: A1 (entity model) — EntityKey and Entity struct exist.

Key files:
- crates/web/src/v2_protocol.rs — server-to-spectator message types
- crates/web/src/v2_roundrobin.rs — builds spectator snapshots
- frontend/src/v2types.ts — TypeScript types
- docs/architecture.md — protocol documentation

### Goal

Replace SpectatorUnit + SpectatorConvoy + SpectatorPopulation with a single
SpectatorEntity type in the wire protocol. Update both Rust serialization and
TypeScript deserialization.

### Scope

1. Define SpectatorEntity in v2_protocol.rs:
   ```rust
   pub struct SpectatorEntity {
       pub id: u32,
       pub owner: Option<u8>,
       pub q: i32,
       pub r: i32,
       pub health: Option<f32>,
       pub role: Option<String>,
       pub combat_skill: Option<f32>,
       pub engaged: bool,
       pub facing: Option<f32>,
       pub resource_type: Option<String>,
       pub resource_amount: Option<f32>,
       pub structure_type: Option<String>,
       pub build_progress: Option<f32>,
       pub contains_count: usize,
   }
   ```

2. Update V2Snapshot to use `entities: Vec<SpectatorEntity>` replacing the
   separate unit/convoy/population/settlement vecs.

3. Update v2_roundrobin.rs snapshot building to produce SpectatorEntity from
   the engine's Entity type.

4. Update frontend/src/v2types.ts with matching TypeScript types.

5. Update V2App.tsx and HexCanvas.tsx (or HexBoard.tsx if HexCanvas doesn't
   exist yet) to consume the new entity format.

6. Update docs/architecture.md V2 WebSocket Protocol section.

### Verification

```bash
cargo test -p simulate-everything-web
cd frontend && bun run build
```

Restart service, open browser, verify the V2 RR game renders correctly with
the new protocol. Units, convoys, settlements should all appear.

Commit when done.
```

### B2: Operations Layer

```
## Task: Implement shared Operations layer for V3 agents

Project: ~/dev/personal/simulate_everything
Spec: docs/specs/v3-entity-unification-2026-04-13.md (Operations Layer section)
Depends on: A2 (sim tick with entity economy) — role assignment, structure
construction, and convoy movement work.

Key files:
- crates/engine/src/v2/city_ai.rs — current autonomous city AI (becomes operations)
- crates/engine/src/v2/agent.rs — current agents
- crates/engine/src/v2/directive.rs — OperationalCommand from B1

### Goal

Implement the Operations layer that all agent personalities share. This is the
current city_ai promoted and generalized. It receives StrategicDirectives and
translates them into entity-level OperationalCommands.

### Scope

1. Population role assignment: given economic focus (Growth/Military/Infrastructure),
   decide farmer/worker/soldier/builder ratios per settlement.

2. Stack formation: group military entities into stacks by proximity and purpose.
   Assign StackRole (Assault, Garrison, Scout, Supply) based on strategic directives.

3. Stack routing: pathfind stacks toward prioritized regions or expansion targets.

4. Infrastructure: decide where to build structures and roads based on strategic
   posture. Expand builds farms, Attack builds roads toward enemy, Defend builds
   at chokepoints.

5. Supply lines: identify settlements that need resources, assign convoy entities
   to transport food/material.

6. Settler dispatch: when strategy says expand, identify good settlement sites and
   dispatch settler groups.

### What NOT to do

- Strategy layer personalities (B3)
- Tactical combat decisions (B4)
- Engine sim changes — emit OperationalCommands, let the existing command
  application machinery execute them

### Verification

```bash
cargo test -p simulate-everything-engine
```

Write tests for:
- Given Expand posture + Growth focus, operations assigns mostly farmers
- Given Attack posture + Military focus, operations trains soldiers and forms stacks
- Supply routes established between settlements and forward stacks
- Stack formation groups nearby military entities

Run a full game and verify agents produce a functioning economy:
```bash
cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --seeds 0-9 --ticks 1000 --size 20x20 --agents spread,striker
```

Commit when done.
```

### B3: Strategy Layer

```
## Task: Implement agent personality Strategy layers

Project: ~/dev/personal/simulate_everything
Spec: docs/specs/v3-entity-unification-2026-04-13.md (Strategy Layer section)
Depends on: B2 (operations layer) — operations can receive and execute strategic
directives.

Key files:
- crates/engine/src/v2/agent.rs — agent definitions, current SpreadAgent/StrikerAgent
- crates/engine/src/v2/directive.rs — StrategicDirective from B1

### Goal

Implement three distinct Strategy layer personalities. Each emits different
StrategicDirectives that the shared Operations layer executes.

### Scope

1. **SpreadAgent strategy**: Favors Expand posture early, transitions to
   Consolidate once territory is large. EconomicFocus = Growth. Prioritizes
   frontier regions for expansion. Defensive — attacks only when significantly
   stronger.

2. **StrikerAgent strategy**: Favors Attack posture early. EconomicFocus =
   Military. Prioritizes regions near enemy. Aggressive — forms assault stacks
   and routes them toward enemy settlements.

3. **TurtleAgent strategy**: Favors Defend posture. EconomicFocus = Infrastructure.
   Prioritizes own core regions. Builds up economy and defenses, attacks only
   when overwhelmingly advantaged.

4. Each strategy runs every ~50 ticks. It receives the full observation and
   emits a Vec<StrategicDirective>. The operations layer handles translation.

5. Posture transitions: each personality should have conditions for switching
   posture (e.g., Spread switches from Expand to Attack if enemy is weak;
   Striker switches to Defend if losing territory).

### Verification

```bash
cargo test -p simulate-everything-engine
```

Run round-robin across all three personalities:
```bash
cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --matchups all --seeds 0-49 --ticks 2000 --size 20x20
```

Verify:
- All three personalities produce distinct behavior visible in ASCII output
- Spread expands territory broadly
- Striker concentrates military force
- Turtle builds dense infrastructure in core territory
- Games complete without crashes

Commit when done.
```

### B4: Tactical Layer

```
## Task: Implement Tactical layer for per-stack combat decisions

Project: ~/dev/personal/simulate_everything
Spec: docs/specs/v3-entity-unification-2026-04-13.md (Tactical Layer section)
Depends on: A3 (individual facing-based combat) — combat resolution works.

Key files:
- crates/engine/src/v2/agent.rs — agent traits from B1
- crates/engine/src/v2/combat.rs — individual combat from A3
- crates/engine/src/v2/state.rs — Entity with Combatant component

### Goal

Implement the Tactical layer that activates per-stack when enemies are nearby.
It assigns individual engagements, manages facing, and orders retreats.

### Scope

1. Activation: for each stack within 2 hexes of enemy entities, run tactical.

2. Engagement assignment: decide which friendly entities engage which enemies.
   Prefer: concentrate on weak targets, flank when possible (attack from hex
   opposite to where other friendlies are engaging).

3. Facing management: set each entity's facing toward its assigned target.
   If no target, face the nearest threat.

4. Force assessment: compare local strength. If outnumbered significantly,
   order retreat toward nearest friendly settlement/stack.

5. Retreat: entities ordered to retreat move away from enemies. They take rear
   damage while retreating (the organic disengage cost from A3).

6. Hold: entities ordered to hold stay and fight to the death. Used for
   garrison stacks defending structures.

### V1 simplicity

The V1 tactical layer should be simple:
- Engage nearest enemy with best angle
- Retreat if local ratio < 0.5
- Hold if garrisoned
- No formation logic yet (that's an optimization within this layer later)

### Verification

```bash
cargo test -p simulate-everything-engine
```

Write tests for:
- Stack near enemy: tactical activates and assigns engagements
- Outnumbered stack: tactical orders retreat
- Flanking: when two stacks attack from different directions, engagements
  distribute to exploit facing weakness
- Retreat under fire: retreating entities take rear damage

Commit when done.
```

### D2: RR Loop Adaptation

```
## Task: Update V2 round-robin loop for entity-based engine

Project: ~/dev/personal/simulate_everything
Depends on: D1 (wire protocol with SpectatorEntity)

Key files:
- crates/web/src/v2_roundrobin.rs — RR loop
- crates/web/src/v2_protocol.rs — updated protocol types (from D1)
- crates/engine/src/v2/runner.rs — game runner
- crates/engine/src/v2/replay.rs — replay recording

### Goal

Update the RR loop to work with the V3 entity-based engine. Agent initialization,
snapshot building, replay recording, and review capture all need to use the new
entity types.

### Scope

1. Agent initialization: create agents with the three-layer architecture (if B1
   types exist) or the compatibility wrapper.

2. Snapshot building: produce SpectatorEntity from engine entities for each tick.

3. Replay recording: EntitySnapshot replaces UnitSnapshot/ConvoySnapshot.

4. Review capture: update review bundle format to include entity data instead of
   separate unit/convoy/population sections.

5. Catchup: late-joining spectators get entity-based init + snapshot.

6. Keep all existing RR features: pause/resume/reset, flag/capture, review
   persistence.

### Verification

```bash
cargo test -p simulate-everything-web
cd frontend && bun run build
```

Restart service, open browser at /v2/rr:
- Game runs continuously
- Entities render on the board
- Pause/resume/reset work
- Flag and capture work
- Review list shows and opens saved bundles

Commit when done.
```

### D3: Port Live WS Status to V3

```
## Task: Port V2 live status WS streaming to V3 protocol

Project: ~/dev/personal/simulate_everything
Depends on: D2 (RR loop updated for entities) and V2-2 (live status WS implemented)

Key files:
- crates/web/src/v2_protocol.rs — v2_rr_status message (from V2-2)
- crates/web/src/v2_roundrobin.rs — status broadcast points (from V2-2)
- frontend/src/V2App.tsx — WS status consumption (from V2-2)

### Goal

Verify that the live status WS streaming (implemented in V2-2) still works
correctly after the D1/D2 entity migration. Fix any breakage.

### Scope

This should be a small task — V2-2 implements the feature, D1/D2 change the
entity types. The status message itself (v2_rr_status) doesn't contain entity
data, so it should mostly work. But verify:

1. Status broadcasts still fire on all trigger points
2. Catchup includes current status
3. Frontend receives and applies status correctly
4. Tick-gated hover still works with entity-based frames

### Verification

Same as V2-2 acceptance criteria, but on the V3 engine:
- Capturable range updates live
- Pause/resume updates immediately
- Hover is tick-gated during live play, immediate when paused

Commit if any fixes needed.
```

### C2: Entity Rendering

```
## Task: Update PixiJS renderer for unified entity protocol

Project: ~/dev/personal/simulate_everything
Depends on: C1 (PixiJS scaffold exists) and D1 (SpectatorEntity wire type)

Key files:
- frontend/src/HexCanvas.tsx — PixiJS renderer (from C1)
- frontend/src/v2types.ts — updated types (from D1)

### Goal

Update the PixiJS renderer to consume SpectatorEntity instead of separate
unit/convoy/settlement types. Render all entity types with unified sprites.

### Scope

1. Parse SpectatorEntity from V2Snapshot frames.

2. Render entities by type:
   - Person with Combatant: colored circle with strength indicator
   - Person without Combatant (civilian): smaller neutral marker
   - Structure: hex fill with structure type icon
   - Resource: small resource icon

3. Stack rendering at mid zoom: when multiple entities share a hex, show a
   count badge ("x15") and the dominant entity type.

4. Facing indicator: small arrow showing combatant facing direction (only at
   close zoom).

5. Hover: clicking/hovering an entity hex shows entity details in a tooltip.

### Verification

```bash
cd frontend && bun run build
```

Open browser:
- All entity types render distinctly
- Stacks show count badges
- Facing arrows visible at close zoom
- Hover shows entity info
- No visual regressions from C1

Commit when done.
```

### C3: Entity Interpolation

```
## Task: Add tick interpolation to PixiJS renderer

Project: ~/dev/personal/simulate_everything
Depends on: C2 (entity rendering works)

Key files:
- frontend/src/HexCanvas.tsx — PixiJS renderer
- frontend/src/V2App.tsx — frame buffering

### Goal

Buffer two server ticks and interpolate entity positions between them for
smooth visual movement. Eliminates the hex-to-hex teleporting effect.

### Scope

1. Buffer: keep the two most recent frames (previous + current).

2. On each render frame (requestAnimationFrame), compute interpolation factor:
   `t = elapsed_since_last_tick / tick_interval`

3. For each entity that exists in both frames: lerp pixel position from
   previous hex center to current hex center using t.

4. For entities that appear in current but not previous: fade in.

5. For entities that appear in previous but not current: fade out (death).

6. Entity sprites update position every render frame, not every tick.

7. Handle edge cases: game reset (clear buffer), pause (freeze interpolation),
   speed change (adjust tick interval).

### Verification

```bash
cd frontend && bun run build
```

Open browser:
- Units glide smoothly between hexes instead of teleporting
- Movement looks natural at 10 ticks/sec
- New units fade in, dead units fade out
- Pausing freezes interpolation cleanly
- Speed changes don't cause visual glitches

Commit when done.
```
