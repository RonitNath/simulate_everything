# Stream E: Agent Behavior System

Status: **ready for implementation**
Depends on: Phase 0 (protocol crate), Stream C (spatial index), A1-A4 (agent layers)
Integrates with: Stream D (terrain ops as action targets), Stream F (compositional world model)

## Goal

Replace the current flat task-assignment system (`Person::task: TaskAssignment`)
with an autonomous agent behavior architecture: needs-driven utility scoring,
HTN-based goal decomposition into action queues, and dual-mode execution
(tick-by-tick and batch-resolvable). Every entity decides for itself. Strategy
and Operations layers influence behavior by adjusting need weights and injecting
HTN methods — they no longer micromanage individual entities.

## User stories

**As a farmer entity**, I autonomously pursue food-related goals when hungry,
shelter goals when exposed, social goals when isolated, and duty goals when
assigned by my faction. I don't wait for an operations layer to assign me a
task — I find work that satisfies my needs from the methods available to me.

**As a soldier entity**, my safety need spikes when enemies approach, which
causes my utility scorer to select combat goals. My faction's strategy layer
has raised my `duty.combat_weight`, making me prioritize fighting over fleeing.
But if I'm starving and no food is available, I may desert — not because
someone coded desertion, but because survival needs overwhelm duty.

**As the strategic tier**, I can fast-forward 10,000 entities through 3 game-hours
by popping their action queues: apply effects, accumulate durations, skip physics.
Each entity emerges with a coherent personal history ("walked to field, harvested
wheat, walked home, ate, rested") computed in microseconds.

**As the operations layer**, I don't assign tasks. I adjust faction-wide need
weights ("attack posture" → `duty.combat_weight += 0.5`) and inject HTN methods
("supply route A→B available" → new `SupplyHaul` method for my faction's
entities). Entities figure out the rest.

**As the viewer/spectator**, I can inspect any entity and see its current goal,
action queue, need state, and recent decision history. "Why did this farmer
walk to the river?" — because their water need was highest and the nearest
water source decomposed to a MoveTo(river) action.

## Current state

`Person::task: Option<TaskAssignment>` — flat enum (Farm, Workshop, Patrol,
Garrison, Train, Idle). Assigned by `OperationalCommand::AssignTask`. No
autonomy, no needs, no goal decomposition.

`EntityTask` in `agent.rs` — similar flat enum used by the operations layer
to specify what entities should do.

`tick_economy` in `economy.rs` — checks `person.task` directly to determine
production. Tightly coupled to the task assignment model.

Strategy personalities (Spread/Striker/Turtle) in `strategy.rs` — read
`StrategicView`, emit `StrategicDirective`. Sound architecture but
directives currently map 1:1 to operational commands which map 1:1 to task
assignments. No entity autonomy in the chain.

## Architecture

### Three-layer entity decision system

```
Needs (what do I want?) → Goal (what should I pursue?) → Action Queue (how do I do it?)
```

#### Layer 1: Needs + Utility Scorer

Per-entity needs that decay over time:

```rust
struct EntityNeeds {
    hunger: f32,      // 0 = full, 1 = starving
    safety: f32,      // 0 = secure, 1 = terrified
    duty: f32,        // 0 = fulfilled, 1 = derelict
    rest: f32,        // 0 = fresh, 1 = exhausted
    social: f32,      // 0 = content, 1 = isolated
    shelter: f32,     // 0 = housed, 1 = exposed
}
```

Needs decay at configurable rates. Events spike needs instantly (nearby
explosion → safety = 0.8, long march → rest += 0.3).

**Utility scoring**: hierarchical buckets with geometric mean.

```
Bucket 1 (survival): hunger, safety — evaluated first
Bucket 2 (duty): duty — military/economic obligations
Bucket 3 (maintenance): rest, shelter — personal upkeep
Bucket 4 (social): social — community participation
```

Within each bucket, candidate goals are scored by response curves on relevant
need axes. Geometric mean of all consideration scores per goal. The first
bucket with any goal scoring above threshold wins. This gives natural priority
ordering without hardcoded if-chains.

**Archetype clustering** for amortized evaluation:
- Hash entity state into behavioral archetypes: `(personality_bucket, needs_bucket, context_bucket) → cluster_id`
- Run utility scoring once per cluster, broadcast result to all entities in cluster
- 100k entities with ~50-200 archetypes = 50-200 scoring passes per decision cycle

**Decision frequency LOD**:
- `next_decision_tick: u64` per entity
- Combat-adjacent: every tick
- Active goal in progress: every 50 ticks (check for interrupts only)
- Peaceful interior: every 200 ticks
- On decision tick: apply accumulated need decay as `rate × ticks_elapsed`

#### Layer 2: HTN Decomposition

Goals decompose into concrete action sequences via **Hierarchical Task
Network** methods. Methods are data, not code.

```rust
struct HtnMethod {
    /// Human-readable name for debugging/inspection.
    name: &'static str,
    /// Conditions that must hold for this method to apply.
    preconditions: SmallVec<[Condition; 4]>,
    /// Subtasks to execute in sequence. May be primitive actions or
    /// compound tasks requiring further decomposition.
    subtasks: SmallVec<[TaskRef; 8]>,
    /// Expected duration for batch-resolution fast-forward.
    expected_duration: f32,
    /// State changes applied on completion (for batch resolution).
    effects: SmallVec<[Effect; 4]>,
}
```

**Domains** are registries of methods grouped by capability area:
- `subsistence` — food acquisition, water, foraging
- `material_work` — shaping, cutting, heating, mixing materials
- `construction` — placing structures, modifying terrain
- `transport` — walking, riding, hauling, sailing
- `combat` — engaging, retreating, flanking, holding
- `social` — trading, negotiating, communicating, gathering

Domains compose freely. "Build a wall" decomposes through construction →
transport → material_work as needed. New capabilities = new method
definitions in the domain registry.

**Method Traversal Record (MTR)** for efficient replanning:
- Store the decomposition path taken for the current plan
- On precondition failure, compare new MTR to old
- Replan only the divergent subtask, not the whole tree
- Keeps replanning cost proportional to the failure depth, not plan depth

**Decomposition depth varies by tier**:
- Close tier: fully decompose to primitive actions (MoveTo, PickUp, ApplyTool)
- Tactical tier: decompose to medium-grain actions (GoToLocation, PerformWork)
- Strategic tier: keep abstract (AcquireFood, DoLabor) — batch-resolve by
  applying effects and durations

#### Layer 3: Action Queue Execution

Primitive actions executed tick-by-tick or batch-resolved:

```rust
enum Action {
    // Locomotion
    MoveTo { target: Vec3, mode: MovementMode },

    // Object manipulation
    PickUp { object: EntityKey },
    PutDown { object: EntityKey, location: Vec3 },
    ApplyTool { tool: EntityKey, target: EntityKey, action_type: PhysicalAction },
    Transfer { item: EntityKey, to: EntityKey },

    // Construction / terrain
    Place { object: EntityKey, position: Vec3, orientation: f32 },
    ModifyTerrain { area: Aabb2d, op: TerrainOpType },

    // Communication
    Communicate { target: EntityKey, content: CommContent },
    Broadcast { radius: f32, content: CommContent },

    // Perception
    Observe { target: ObserveTarget, duration: f32 },

    // Bodily
    Consume { item: EntityKey },
    Rest { duration: f32 },

    // Meta
    Wait { duration: f32, until: Option<Condition> },
}

enum PhysicalAction {
    Strike,    // hammer on anvil, sword on person, axe on tree
    Shape,     // carve, mold, bend
    Cut,       // slice, saw
    Heat,      // apply fire source to target
    Mix,       // combine materials
    Dig,       // shovel into ground → terrain op
}
```

**Dual-mode execution**:

*Tick-by-tick (close/tactical tier)*:
- `MoveTo` → steering behaviors, physics integration, collision
- `ApplyTool` → animation, body model, damage/crafting resolution
- `Communicate` → social interaction, opinion dynamics
- Each tick: check current action preconditions, advance progress, check completion

*Batch resolution (strategic tier)*:
- `MoveTo` → teleport, subtract travel_duration from time budget
- `ApplyTool` → apply effect immediately, subtract action_duration
- `Communicate` → apply opinion shift, subtract duration
- Pop actions from queue, apply effects, accumulate durations until time budget exhausted

### Integration with existing agent layers

**Strategy layer** (Spread/Striker/Turtle):
- No longer emits `RequestStack` or `SetExpansionTarget` directly
- Instead emits **need weight adjustments** for faction entities:
  - `SetPosture(Attack)` → all faction entities: `duty.combat_weight += 0.5`
  - `SetEconomicFocus(Growth)` → all faction entities: `duty.production_weight += 0.3`
  - `PrioritizeRegion { center, priority }` → entities near region: `duty.regional_weight += priority`

**Operations layer**:
- No longer emits `AssignTask` commands
- Instead **injects HTN methods** into faction domain registries:
  - `FormStack` → adds `JoinStack` method available to nearby military-ready entities
  - `EstablishSupplyRoute { from, to }` → adds `SupplyHaul { from, to }` method
  - `ProduceEquipment` → adds `CraftItem { recipe }` method at relevant locations
- Also manages entity role transitions (civilian ↔ military readiness)

**Tactical layer**:
- Becomes a **priority interrupt** on the action queue
- When `resolution_demand` at entity's location exceeds threshold, safety need spikes
- Entity's own utility scorer selects combat goal, decomposes via combat domain
- Tactical layer's value: **coordination** (formation, focus-fire, flanking), not individual action selection
- Coordination signals override individual goal selection for stack members in combat

### Resolution demand (generalized engagement detection)

Replace `stack_near_enemy()` with:

```rust
fn resolution_demand_at(state: &GameState, hex: Axial) -> f32 {
    let conflict_intensity = count_conflicting_goals_in_region(state, hex);
    let outcome_uncertainty = estimate_uncertainty(state, hex);
    let stakes = estimate_stakes(state, hex);

    conflict_intensity * outcome_uncertainty * stakes
}
```

Triggers promotion to higher tick rate when threshold exceeded. Applies to:
- Combat (conflicting survival/duty goals between factions)
- Negotiation (conflicting resource/influence goals between entities)
- Contested construction (time-critical building under threat)
- Competition (conflicting achievement goals in shared space)
- Research/discovery (high-uncertainty high-stakes individual actions)

## Waves

### E1: Needs system + utility scorer

**Files created:**
- `crates/engine/src/v3/needs.rs` — `EntityNeeds` struct, need decay logic,
  need event triggers (spike on combat proximity, rest on sleep, etc.),
  `NeedWeights` for faction-level bias

**Files modified:**
- `crates/engine/src/v3/state.rs` — Add `needs: Option<EntityNeeds>` to Entity,
  add `faction_need_weights: Vec<NeedWeights>` to GameState
- `crates/engine/src/v3/sim.rs` — Add need decay phase to tick loop
  (after economy, before steering)
- `crates/engine/src/v3/mod.rs` — Declare module
- `crates/engine/src/v3/lifecycle.rs` — Initialize needs on entity spawn

**Deliverables:**
- Need decay with configurable rates per need type
- Need event triggers (combat proximity → safety spike, food consumption → hunger reset)
- Decision frequency LOD (`next_decision_tick` per entity)
- Accumulated decay on decision tick: `need += rate × ticks_elapsed`

**Tests:**
- Need decay over time matches expected rate
- Safety spike when enemy within engagement radius
- Hunger resets on food consumption
- Decision LOD: entity with `next_decision_tick = 50` not evaluated at tick 25
- Accumulated decay: entity evaluated at tick 100 after last eval at tick 0
  has correct need values

### E2: Utility scorer + goal selection

**Files created:**
- `crates/engine/src/v3/utility.rs` — `UtilityScorer`, response curves,
  geometric mean scoring, hierarchical bucket evaluation, archetype
  clustering, `Goal` enum

**Files modified:**
- `crates/engine/src/v3/state.rs` — Add `current_goal: Option<Goal>` to Entity
- `crates/engine/src/v3/sim.rs` — Add goal selection phase (after need decay,
  before action execution)
- `crates/engine/src/v3/strategy.rs` — Refactor directives to emit need weight
  adjustments instead of direct commands

**Deliverables:**
- Response curves (linear, quadratic, logistic, step) on need axes
- Geometric mean of consideration scores per goal
- Hierarchical bucket evaluation with early-exit
- Archetype clustering: hash `(personality_bucket, needs_bucket, context_bucket)`
- Goal enum: Eat, Drink, Shelter, Rest, Socialize, Work, Fight, Flee, Trade, Build, Explore
- Scorer runs only on decision ticks, skips entities not due

**Tests:**
- Starving entity selects Eat goal
- Entity with high duty and combat proximity selects Fight
- Archetype clustering: two entities with identical state hash to same cluster
- Hierarchical buckets: survival bucket preempts duty bucket
- Geometric mean: goal with one zero-scored consideration gets eliminated
- Strategy directive `SetPosture(Attack)` adjusts faction need weights

### E3: HTN domain engine + method registry

**Files created:**
- `crates/engine/src/v3/htn.rs` — `HtnMethod`, `HtnDomain`, `DomainRegistry`,
  decomposition engine, `TaskRef` (primitive or compound), `Condition`, `Effect`,
  Method Traversal Record, partial replan logic

**Files modified:**
- `crates/engine/src/v3/state.rs` — Add `domain_registry: DomainRegistry` to
  GameState, add `action_queue: Option<ActionQueue>` and `mtr: Option<Mtr>` to Entity

**Deliverables:**
- Method definition as data struct (preconditions, subtasks, duration, effects)
- Domain registry: named collections of methods, per-faction override layers
- Decomposition engine: given a goal + entity state, produce an action queue
- MTR-based partial replan on precondition failure
- Decomposition depth control (full/medium/abstract for tier-appropriate resolution)

**Tests:**
- Goal `Eat` with food at home decomposes to [MoveTo(home), PickUp(food), Consume(food)]
- Goal `Eat` without food decomposes to [MoveTo(field), Harvest, MoveTo(home), Consume]
- Precondition failure mid-queue triggers partial replan
- MTR replan: only divergent subtask redecomposes
- Faction method injection: new method available only to injecting faction's entities
- Compound task decomposition: nested methods flatten to primitive action sequence

### E4: Action queue execution engine

**Files created:**
- `crates/engine/src/v3/action_queue.rs` — `ActionQueue`, `Action` enum,
  tick-by-tick execution, batch resolution, precondition checking, action
  progress tracking

**Files modified:**
- `crates/engine/src/v3/sim.rs` — Add action execution phase (after goal
  selection, integrated with steering/movement/economy). Replace direct
  task-based economy with action-based production.
- `crates/engine/src/v3/economy.rs` — Refactor: production no longer checks
  `person.task` directly. Instead, `ApplyTool` actions targeting farm/workshop
  entities produce resources as a side effect of action execution.
- `crates/engine/src/v3/movement.rs` — `MoveTo` action sets waypoints on
  Mobile component; existing steering system handles the rest.

**Deliverables:**
- Tick-by-tick execution: advance current action, check completion, pop next
- Batch resolution: apply effects, accumulate durations, skip physics
- `MoveTo` integration with existing steering/waypoint system
- `ApplyTool` integration with existing weapon/damage system (combat) and
  new resource production (economy)
- `Consume` integration with food consumption
- Precondition checking on each action start
- Goal completion detection → trigger new goal selection

**Tests:**
- MoveTo action: entity moves toward target via steering, completes on arrival
- ApplyTool(Strike, sword, enemy): triggers existing attack pipeline
- ApplyTool(Harvest, sickle, wheat): produces food resource
- Batch resolution: 5-action queue resolves to correct final state
- Precondition failure: entity re-evaluates goal
- Queue empty: entity returns to utility scorer for new goal

### E5: First domain definitions

**Files created:**
- `crates/engine/src/v3/domains/mod.rs` — Domain module declaration
- `crates/engine/src/v3/domains/subsistence.rs` — Food acquisition methods
  (forage, harvest, trade for food, hunt)
- `crates/engine/src/v3/domains/material_work.rs` — Material transformation
  methods (shape metal, cut wood, mix materials)
- `crates/engine/src/v3/domains/construction.rs` — Building methods
  (place structure, modify terrain, repair)
- `crates/engine/src/v3/domains/transport.rs` — Movement methods
  (walk, haul cargo, escort)
- `crates/engine/src/v3/domains/combat.rs` — Combat methods
  (engage, retreat, hold position, flank)
- `crates/engine/src/v3/domains/social.rs` — Social methods
  (trade goods, gather, communicate)

**Files modified:**
- `crates/engine/src/v3/mapgen.rs` — Initialize domain registry with default
  method set during map generation

**Deliverables:**
- ~30-40 method definitions across 6 domains
- Each method: preconditions, subtask sequence, expected duration, effects
- Methods parameterized by physical constraints (tool properties, material
  properties), not named entity types
- Combat domain methods integrate with existing tactical layer coordination

**Tests:**
- Subsistence domain: entity with Eat goal + food at home produces correct queue
- Material work: entity with Build goal + iron + hammer produces correct queue
- Construction: entity with Build goal + materials produces place+terrain actions
- Combat: entity with Fight goal produces engage sequence
- Cross-domain: Build goal decomposes through construction → transport → material_work
- Domain injection: faction-specific method overrides default

### E6: Resolution demand + social state

**Files created:**
- `crates/engine/src/v3/resolution.rs` — `resolution_demand_at()`, conflict
  intensity estimation, uncertainty estimation, stakes estimation
- `crates/engine/src/v3/social.rs` — `SocialState` (personality vector,
  relationship cache, reputation, faction loyalty, social memory ring buffer),
  opinion dynamics on interaction

**Files modified:**
- `crates/engine/src/v3/state.rs` — Add `social: Option<SocialState>` to Entity
- `crates/engine/src/v3/agent.rs` — Replace `stack_near_enemy()` with
  `resolution_demand_at()` for tactical layer activation
- `crates/engine/src/v3/sim.rs` — Add social state update phase
  (after action execution)

**Deliverables:**
- Resolution demand evaluation per hex region
- Conflict detection beyond combat: negotiation, contested construction, competition
- Per-entity personality vector (8 × i8), relationship cache (top-8 by salience)
- Opinion dynamics: personality similarity + shared events → opinion shift
- Social memory ring buffer (last 8 interactions)
- Social need satisfaction from proximity and interaction

**Tests:**
- Two opposing armies in same hex → high resolution demand
- Two negotiating entities with conflicting goals → moderate resolution demand
- Peaceful hex with no conflicts → zero resolution demand
- Opinion dynamics: similar personalities shift toward each other
- Relationship cache: most-interacted entities maintained in top-8
- Social memory: recent events accessible, oldest evicted

### E7: Behavior validation infrastructure

**Goal:** Tick-level forensic debugging, headless screenshot pipeline, arena
behavior scenarios, and long-horizon invariant-checking bench harness. Two
modes: statistical (1000 runs, check invariants) and forensic (1 run, capture
everything, step through frame-by-frame).

**Files created:**
- `crates/cli/src/v3_behavior_bench.rs` — Behavior scenario runner with
  invariant checking. Scenarios defined as TOML configs specifying setup
  (entity count, settlement layout, threat injection timing), duration, and
  invariant thresholds. Two modes: `--stat` (batch runs, pass/fail rates)
  and `--forensic` (single run, full state capture).
- `crates/engine/src/v3/behavior_snapshot.rs` — Rich per-tick entity state
  capture: position, needs, current goal, action queue contents, HTN
  decomposition path, utility scores for top-3 goals, social state summary.
  Serializable to JSON for analysis tools.
- `crates/cli/src/headless_renderer.rs` — Headless 2D top-down renderer.
  Outputs PNG per tick or filmstrip. Renders: terrain heightmap as grayscale,
  terrain ops as overlays (ditches=blue, walls=brown, roads=gray, furrows=green),
  entities as colored dots with goal annotations, movement vectors as arrows,
  action targets as dotted lines. No GPU required — CPU rasterization via
  `tiny-skia` or similar.

**Files modified:**
- `crates/cli/src/main.rs` — Add `v3behavior` subcommand
- `crates/cli/src/v3bench.rs` — Extract shared arena setup logic into reusable
  helpers that `v3_behavior_bench.rs` also uses
- `crates/engine/src/v3/sim.rs` — Add optional `BehaviorSnapshot` capture hook
  in tick loop (zero-cost when disabled via compile flag or runtime toggle)

**Scenario types:**

*Individual behavior (arena-style):*
```toml
[scenario]
id = "solo_farmer_harvest"
description = "One farmer, one field, one home. Verify harvest → eat cycle."
duration_ticks = 500
mode = "forensic"

[setup]
entities = [
  { role = "person", pos = [100, 100], needs = { hunger = 0.6 } },
]
structures = [
  { type = "field", pos = [150, 100], properties = { tags = "HARVESTABLE" } },
  { type = "shelter", pos = [100, 100] },
]

[invariants]
entity_0_hunger_below = { threshold = 0.3, by_tick = 200 }
entity_0_visited = { pos = [150, 100], by_tick = 100 }
food_produced = { min = 1, by_tick = 300 }
```

*1v1 combat behavior:*
```toml
[scenario]
id = "1v1_sword_engagement"
description = "Two soldiers, verify approach → engage → resolve."
duration_ticks = 300
mode = "forensic"

[setup]
entities = [
  { role = "soldier", pos = [50, 100], owner = 0, weapon = "sword", needs = { duty = 0.9 } },
  { role = "soldier", pos = [200, 100], owner = 1, weapon = "sword", needs = { duty = 0.9 } },
]

[invariants]
engagement_started = { by_tick = 100 }
one_entity_dead_or_fled = { by_tick = 250 }
```

*Small group coordination:*
```toml
[scenario]
id = "patrol_responds_to_threat"
description = "5 patrol entities, 2 hostiles appear at tick 100."
duration_ticks = 500

[setup]
entities = [
  { role = "soldier", pos = [100, 100], owner = 0, count = 5 },
]
threat_injection = { tick = 100, entities = [
  { role = "soldier", pos = [300, 100], owner = 1, count = 2 },
]}

[invariants]
response_time = { max_ticks = 50 }
committed_defenders = { min = 3, by_tick = 150 }
hostiles_engaged = { by_tick = 200 }
```

*Long-horizon settlement stability:*
```toml
[scenario]
id = "settlement_stability_200"
description = "200 entities, established settlement. Survive 5000 ticks."
duration_ticks = 5000
mode = "stat"
runs = 100

[setup]
preset = "established_settlement"
entity_count = 200

[invariants]
food_stockpile_positive = { all_ticks = true }
tool_coverage = { min = 0.8, sample_interval = 100 }
idle_fraction = { max = 0.15, sample_interval = 100 }
population_alive = { min = 180, at_end = true }
```

*Terrain exploitation:*
```toml
[scenario]
id = "terrain_road_emergence"
description = "50 entities, two clusters 500m apart. Roads should emerge."
duration_ticks = 10000

[setup]
clusters = [
  { pos = [200, 200], entities = 30, type = "settlement" },
  { pos = [700, 200], entities = 20, type = "farm_cluster" },
]

[invariants]
road_ops_between_clusters = { min = 5, by_tick = 5000 }
travel_time_decreased = { comparison = "tick_1000_vs_tick_8000", min_ratio = 0.7 }
```

**Forensic output format:**

For `--forensic` mode, produces a directory per scenario:
```
v3behavior_output/solo_farmer_harvest/
  summary.json          — scenario metadata, invariant results, pass/fail
  ticks/
    0000.json           — full BehaviorSnapshot at tick 0
    0001.json           — ...
  frames/
    0000.png            — headless-rendered frame at tick 0
    0001.png            — ...
  filmstrip.png         — composite image, 1 row per 50 ticks
  entity_timelines.json — per-entity: [(tick, goal, action, pos, needs)]
  terrain_ops.json      — terrain op log at end of scenario
  invariants.json       — per-invariant: [(tick, value, threshold, pass)]
```

The `entity_timelines.json` is the primary analysis artifact — you can grep
it, plot it, or feed it to a visualization tool. Each entry is a full
decision record: what the entity needed, what it chose, what it did, where
it was.

**Headless renderer requirements:**
- 2D top-down orthographic projection
- Terrain: heightmap as grayscale intensity
- Terrain ops: colored overlays (ditch=blue, wall=brown, road=gray, furrow=green)
- Entities: colored circles (by owner), size proportional to entity importance
- Annotations: goal name above entity, action target as dotted line
- Movement: velocity vector as arrow from entity center
- Scale bar and tick number in corner
- Resolution: 1024×1024 default, configurable
- Output: PNG per frame, or single filmstrip (N frames tiled)

**Tests:**
- Solo farmer scenario: entity harvests, eats, hunger decreases
- 1v1 combat scenario: entities approach, engage, one wins
- Patrol response: defenders reposition within time bound
- Settlement stability: 100 runs, >95% pass all invariants
- Terrain exploitation: roads appear between frequently-traveled points
- Headless renderer: output PNG matches entity positions from snapshot
- Forensic output: all files present, JSON parseable, invariants evaluated
- Batch resolution parity: forensic snapshot at tick N matches batch-resolved state

## Agent-terrain interplay

Stream E domain definitions (E5) and Stream D terrain ops must integrate
bidirectionally. This is not a separate wave — it's woven into E5 domain
definitions and E6 perception.

### Terrain → Agent decisions

- **Road ops reduce travel duration** in HTN `MoveTo` decomposition. Planner
  checks path for road ops: `expected_duration = distance / (speed × road_bonus)`.
  Entities naturally prefer paths with roads.
- **Furrow ops improve farming yield**. `ApplyTool { action: Harvest }` on
  plowed land produces more food. Entities preferentially farm plowed areas.
- **Ditch/wall ops change movement costs**. Patrol routes adapt. Enemy
  approach paths change. Defensive value of positions changes.
- **Crater ops create hazards**. Safety-sensitive entities avoid damaged areas.
  Over time, abandoned war zones become no-man's-land by emergent avoidance.

### Agent decisions → Terrain

- **Farming domain**: Eat goal → HTN checks if target area has furrow ops →
  if not, first subtask is `ModifyTerrain { op: Furrow }`.
- **Construction domain**: Strategy injects `BuildWall` → entities decompose
  to `ModifyTerrain { op: Ditch }` (foundation) → `ApplyTool { Shape }`
  (materials) → `Place` (structure).
- **Road domain**: Operations observes frequent travel on path A→B → injects
  `BuildRoad { waypoints }` → entities decompose to `ModifyTerrain { op: Road }`.
- **Defensive domain**: Sustained resolution demand → entities with high
  safety+duty select Fortify → `ModifyTerrain { op: Ditch }` across approaches.

### Perception updates needed

`StrategicView` needs terrain awareness (added in E6 or follow-up):
- **Terrain infrastructure assessment** — road coverage, fortification density,
  farming improvement density per region
- **Terrain opportunity assessment** — chokepoints for fortification, fertile
  areas for farming, trade corridor potential
- **Terrain damage assessment** — war-damaged areas, depleted farmland, broken
  infrastructure needing recovery investment

HTN affordance queries need terrain checks (added in E5):
- "Is this area plowed?" → check terrain ops for Furrow at location
- "Is there a road from A to B?" → check terrain ops for Road along path
- "Is this border fortified?" → check terrain ops for Ditch/Wall at location

## Starting conditions

Mapgen must produce settlements that look like they've been running for weeks.
This is a mapgen enhancement after E5 lands, not a separate wave:

- Entities with established need states and spatial habits
- Tools with partial wear (durability < max)
- Fields with furrow terrain ops already applied
- Paths between settlement and fields with road terrain ops
- Social relationships pre-seeded from personality compatibility
- Stockpiles partially filled (not empty, not overflowing)

The benchmark `established_settlement` preset (E7) encodes this: a snapshot
of what the autonomous system produces after stabilization.

## Verification criteria (full stream)

- [ ] Entities autonomously select goals from needs without operations layer commands
- [ ] Utility scoring respects hierarchical bucket priority (survival > duty > maintenance > social)
- [ ] Archetype clustering reduces scoring passes from 100k to ~200
- [ ] Decision frequency LOD: <15k actual decisions per tick at 100k population
- [ ] HTN decomposition produces correct action queues for all 6 domain areas
- [ ] Partial replan via MTR: only divergent subtask redecomposes
- [ ] Tick-by-tick execution: actions drive steering, combat, economy correctly
- [ ] Batch resolution: same action queue produces same final state as tick-by-tick (within epsilon)
- [ ] Strategy directives adjust need weights, not task assignments
- [ ] Operations layer injects methods, not commands
- [ ] Tactical layer triggers on resolution demand, not just combat proximity
- [ ] Economy production driven by action execution, not task assignment checks
- [ ] Entity inspection: current goal, action queue, needs, decision history queryable
- [ ] Performance: 100k entities, strategic tier, batch-resolve 1 game-hour < 50ms
- [ ] Performance: 1k entities, close tier, tick-by-tick execution < 2ms
- [ ] No regression in existing combat, movement, economy behavior
- [ ] Headless renderer produces readable PNGs with entity positions + annotations
- [ ] Forensic mode captures full decision context per entity per tick
- [ ] Solo behavior scenarios pass (harvest, eat, craft, patrol, combat)
- [ ] Group coordination scenarios pass (threat response, formation, patrol coverage)
- [ ] Settlement stability: >95% of 100 runs pass all invariants at 5000 ticks
- [ ] Terrain exploitation: roads emerge between frequently-traveled points
- [ ] Batch-resolve parity: forensic snapshot matches batch-resolved state (within epsilon)
- [ ] Terrain ops integration: farming creates furrows, construction creates ditches/walls
- [ ] Terrain perception: StrategicView reflects infrastructure/damage state
