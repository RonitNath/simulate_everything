# Spec: V3 Domain A — Agent Architecture

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2)
Sequencing: `docs/plans/v3-sequencing.md`

## Vision

Three-layer agent architecture: Strategy (posture, priorities), Operations
(task allocation, logistics), Tactical (per-stack combat). Personality
differentiates only at the Strategy layer — Operations and Tactical are shared
implementations used by all agents. Agents reason about weapon-armor matchups
using a damage estimate table updated from combat observations. A raw combat
log captures every field the damage pipeline computes, serving as future NN
training data.

## Use Cases

1. **Strategic planning.** Every ~50 game-seconds, the strategy layer reads a
   `StrategicView` (perception-built abstraction of game state with fog of war)
   and emits `StrategicDirective`s: posture, economic focus, region priorities,
   stack archetypes. Personality (Spread, Striker, Turtle) determines the policy
   applied to the same view.

2. **Task allocation.** Every ~5 game-seconds, the operations layer reads
   strategic directives + current world state and assigns entities to tasks:
   farming, building, soldiering, crafting, trading, patrolling, settling.
   Equipment production decisions use the damage estimate table to translate
   stack archetypes into loadouts that counter observed enemy equipment.

3. **Tactical combat.** Every tick for stacks within 300m of an enemy entity,
   the tactical layer issues per-entity combat commands: attack targeting
   (using damage table matchup reasoning), facing, formation, blocking, retreat.
   Retreat triggers when own casualty rate exceeds 2x enemy casualty rate over
   a 20-tick rolling window.

4. **Combat learning.** Every combat resolution writes a `CombatObservation` to
   the raw log (replay stream) and updates the damage estimate table's running
   statistics. When observed outcomes diverge from predictions past a threshold,
   the entry resets from recent observations.

## Architecture

### Layer Dispatch

A `LayeredAgent` owns three layers and orchestrates cadence:

```
LayeredAgent {
    strategy: Box<dyn StrategyLayer>,   // personality-specific
    operations: SharedOperationsLayer,   // shared implementation
    tactical: SharedTacticalLayer,       // shared implementation
    damage_table: DamageEstimateTable,   // shared by operations + tactical
    active_directives: Vec<StrategicDirective>,
    strategy_cadence: u64,  // ticks between strategy runs (~50 game-seconds)
    operations_cadence: u64, // ticks between operations runs (~5 game-seconds)
}
```

Each tick, the agent dispatcher:
1. If `tick % strategy_cadence == 0`: run strategy, update `active_directives`
2. If `tick % operations_cadence == 0`: run operations with current directives
3. For each stack within 300m of an enemy entity: run tactical

### Perception Layer — StrategicView

Strategy never reads raw game state. A perception layer builds a
`StrategicView` from game state with fog of war applied:

```rust
struct StrategicView {
    territory: Vec<Region>,          // clusters of controlled/contested/unknown hexes
    relative_strength: StrengthAssessment, // own vs visible enemy stack count + equipment quality
    economy: EconomySnapshot,        // food surplus/deficit, material stockpile, production capacity, growth trend
    threats: Vec<ThreatCluster>,     // enemy concentrations with position, direction, advance/static/retreat
    stack_readiness: Vec<StackHealth>, // per-stack aggregate: fresh/wounded/depleted
}
```

All personalities read the same `StrategicView`. Personality is the policy
applied to the view, not the perception.

### Strategy Layer — Personalities

Three strategy implementations, differing only in posture transitions and
priority weighting:

**Spread** — economy-first. Defaults to Growth economic focus, Expand posture.
Transitions to Consolidate when contested territory exceeds threshold.
Transitions to Attack only when relative strength is overwhelmingly favorable.
Requests balanced archetypes (line infantry, skirmishers).

**Striker** — military-first. Defaults to Military economic focus, Attack
posture. Prioritizes regions near enemy territory. Transitions to Defend only
when relative strength is critically unfavorable. Requests offensive archetypes
(heavy infantry, wedge cavalry).

**Turtle** — infrastructure-first. Defaults to Infrastructure economic focus,
Defend posture. Requests defensive archetypes (garrison infantry, fortification
builders). Transitions to Attack very late, only when economy is dominant.

Posture transition conditions are tunable constants: territory ratio thresholds,
strength ratio thresholds, economy ratio thresholds.

```rust
enum StrategicDirective {
    SetPosture(Posture),
    SetEconomicFocus(EconomicFocus),
    PrioritizeRegion { center: Axial, priority: f32 },
    RequestStack { archetype: StackArchetype, region: Axial },
    SetExpansionTarget { hex: Axial },
}

enum Posture { Expand, Consolidate, Attack, Defend }
enum EconomicFocus { Growth, Military, Infrastructure }
enum StackArchetype { HeavyInfantry, LightInfantry, Skirmisher, Cavalry, Garrison, Settler }
```

### Operations Layer — Task Allocator

Operations is a priority-weighted coordination layer, not a task allocator.
It manages stack formation, routing, equipment, and behavior availability
for autonomous entities.

Inputs:
- Strategic directives (priorities, posture, economic focus, archetypes)
- Available entities and their capabilities
- Resource state (food, materials, equipment inventory)
- Damage estimate table (for archetype → loadout translation)

Output: stack/equipment commands plus method-availability injections.

```rust
enum OperationalCommand {
    FormStack { entities: Vec<EntityKey>, archetype: StackArchetype },
    RouteStack { stack: StackId, waypoints: Vec<Vec3> },
    DisbandStack { stack: StackId },
    ProduceEquipment { workshop: EntityKey, item_type: EquipmentType },
    EquipEntity { entity: EntityKey, equipment: EntityKey },
    EstablishSupplyRoute { from: Axial, to: Axial },
    FoundSettlement { entity: EntityKey, target: Axial },
}
```

**Archetype → loadout translation.** When operations forms a stack, it
consults the damage estimate table to select equipment. Example: strategic
layer requests HeavyInfantry for a front where enemies wear plate armor →
operations reads `(Crush, *, Plate, *)` entries in the damage table →
selects maces + plate for that stack. Same archetype against leather-armored
enemies → selects swords + chain mail.

Operations reads available inventory and workshop capacity to determine
what's feasible. Shortfalls trigger `ProduceEquipment`; supply routes and
settlement opportunities inject HTN methods into the faction domain registry.

### Tactical Layer — Matchup Reasoning

Runs every tick for stacks whose local `resolution_demand_at(...)` exceeds
the hotspot threshold. Tactical activation is driven by conflict intensity
and stakes, not just enemy proximity.

```rust
enum TacticalCommand {
    Attack { attacker: EntityKey, target: EntityKey },
    SetFacing { entity: EntityKey, angle: f32 },
    Block { entity: EntityKey },
    Retreat { entity: EntityKey, toward: Vec3 },
    Hold { entity: EntityKey },
    SetFormation { stack: StackId, formation: FormationType },
}

enum FormationType { Line, Column, Wedge, Square, Skirmish }
```

**Target selection.** For each friendly entity, the tactical layer reads the
damage table to evaluate `(my_weapon, my_material, their_armor, their_material)`
for each visible enemy. Assigns targets to maximize expected damage output.
Redirects entities away from bad matchups: "my slingers vs their plate → low
effectiveness → redirect to their unarmored archers."

**Formation assignment.** Based on stack composition and terrain:
- Spear + shield wall → Line facing enemy
- Archers → Skirmish behind the line
- Cavalry → Wedge for breakthrough
- Mixed defense → Square

Formation determines entity positions (handled by M domain's formation
movement) and facing direction. When in formation, facing follows formation
direction. When formation breaks, facing follows individual threat.

**Retreat decision.** 20-tick rolling window tracking own and enemy casualty
rates. Retreat when own rate exceeds 2x enemy rate sustained over the window.
Both the window size and the ratio threshold are tunable constants.

### Stacks — Game State

Stacks live in `GameState`, not agent internals. Movement (M domain) reads
stack membership every tick for formation steering.

```rust
struct Stack {
    id: StackId,
    owner: u8,
    members: SmallVec<[EntityKey; 32]>,
    formation: FormationType,
    leader: EntityKey,
}
```

The agent's operations layer creates/modifies/dissolves stacks by mutating
game state. The tactical layer reads stacks from game state. The movement
system reads stacks for formation offsets.

### Damage Estimate Table

Plain `HashMap<(WeaponType, WeaponMaterial, ArmorType, ArmorMaterial), DamageEstimate>`.
No moka. ~480 possible entries (6 weapon types × 4 weapon materials × 5 armor
types × 4 armor materials). Fits in a few KB.

```rust
struct DamageEstimate {
    wound_rate: f32,       // fraction of hits that wound
    avg_severity: f32,     // average wound severity (0.0–1.0 scale)
    stagger_rate: f32,     // fraction of hits that stagger
    stamina_drain: f32,    // average stamina cost to defender
    sample_count: u32,     // observations backing this estimate
    last_updated: u64,     // tick of last update
}
```

**Initialization.** At game start, compute theoretical estimates from material
physics (sharpness vs hardness formulas from D domain). These are deterministic
and shared across all agents — they're physics, not learned behavior.

**Empirical update.** After each combat observation, update the running
statistics. Each agent maintains its own table (different agents observe
different combats due to fog of war).

**Staleness.** When reading an entry, if `current_tick - last_updated > STALE_THRESHOLD`,
the entry is treated as lower confidence (wider uncertainty in target selection).

**Surprise detection.** After updating an entry, compare the new running stats
to the prior estimate. If the observed wound rate diverges from the predicted
wound rate by more than `SURPRISE_THRESHOLD`, reset the entry's stats from
the last N observations only (discarding old data that may reflect changed
conditions).

### Combat Observation Log

Every combat resolution writes a `CombatObservation` to the replay stream.
Log everything the 7-step damage pipeline computes — no curation.

```rust
struct CombatObservation {
    tick: u64,
    attacker: EntityKey,
    defender: EntityKey,
    // Weapon properties
    weapon_type: WeaponType,
    weapon_material: MaterialType,
    weapon_sharpness: f32,
    weapon_hardness: f32,
    weapon_weight: f32,
    // Armor properties
    armor_type: ArmorType,
    armor_material: MaterialType,
    armor_hardness: f32,
    armor_thickness: f32,
    armor_coverage: f32,
    // Impact parameters
    hit_zone: BodyZone,
    angle_of_incidence: f32,
    impact_force: f32,
    damage_type: DamageType,
    // Resolution results
    penetrated: bool,
    penetration_depth: f32,
    residual_force: f32,
    wound_severity: Severity,
    bleed_rate: f32,
    stagger_force: f32,
    stagger: bool,
    // Context
    distance: f32,
    height_diff: f32,
    attacker_skill: f32,
    defender_stamina: f32,
    defender_facing_offset: f32, // 0 = facing attacker, PI = rear
}
```

Written to the replay stream as a parallel event channel alongside entity
state snapshots. No in-memory retention beyond the damage estimate table's
running statistics.

### Command Validation

All commands are validated before execution. If a referenced entity, stack,
or equipment no longer exists, the command is dropped with a `tracing::warn!`.
No panics, no error propagation to the agent. The agent self-corrects from
its next observation.

## Convention References

- V2 three-layer skeleton: `crates/engine/src/v2/agent_layers.rs`
- V2 shared operations: `crates/engine/src/v2/operations.rs`
- V3 damage pipeline: `docs/specs/v3-D-damage.md`
- V3 weapons/armor: `docs/specs/v3-W-weapons.md`
- V3 movement/formations: `docs/specs/v3-M-movement.md`
- V3 spatial queries: `docs/specs/v3-S-spatial.md`

## Scope

### V1 (ship this)

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| A1 | 1 | W1 | Layer traits, directive/command enums, cadence dispatch, StrategicView struct |
| A2 | 2 | A1, M2 | Operations layer (task allocator, archetype→loadout translation) |
| A3 | 3 | A2, W3, D2 | Tactical layer (matchup reasoning, formation, facing, retreat) |
| A4 | 3 | A3 | Strategy personalities (Spread, Striker, Turtle) |
| A5 | 3 | D2, W2, W3 | Damage estimate table + combat observation log |

### Deferred

- **NN-based tactical layer.** The combat observation log is the training data
  foundation. A future NN agent implements the same `TacticalLayer` trait,
  replacing heuristic matchup reasoning with learned policies.
- **Dynamic archetype creation.** Operations could invent new archetypes from
  damage table analysis rather than using a fixed enum. Deferred — fixed
  archetypes are sufficient for V3.0.
- **Multi-agent coordination.** Allied agents sharing observations or
  coordinating strategy. Deferred — V3.0 is 1v1 or free-for-all with
  independent agents.
- **Agent-to-agent diplomacy.** Truces, alliances, trade agreements. Not in
  V3.0.

## Verification

Each A-item ships with unit tests:

1. **A1 — Cadence dispatch.** Construct a `LayeredAgent`, advance N ticks,
   assert strategy runs at the right cadence, operations at its cadence,
   tactical runs for stacks within 300m of enemies and not for distant stacks.

2. **A2 — Operations task allocation.** Construct a scenario (entities with
   capabilities, resource state, directives), call operations, assert task
   assignments match directive priorities. Test archetype→loadout translation
   with a pre-populated damage table.

3. **A3 — Tactical target selection.** Construct entities with known weapons
   and armor, populate damage table, call tactical, assert target assignments
   maximize expected damage. Test retreat: set casualty rates above threshold,
   assert retreat commands emitted.

4. **A4 — Strategy personalities.** Construct StrategicViews with different
   territory/strength/economy states, call each personality, assert correct
   posture and focus. Test transitions: Spread consolidates when contested
   territory rises, Striker defends when strength drops, Turtle attacks when
   economy dominates.

5. **A5 — Damage table.** Initialize from material properties, assert
   theoretical estimates are correct. Feed observations, assert running stats
   update. Feed divergent observations, assert surprise detection resets the
   entry.

Performance budgets (verified by E-domain bench harness, not A's
responsibility to build):
- Strategy: < 5ms per invocation
- Operations: < 2ms per invocation
- Tactical: < 1ms per stack

## Key Files (Expected)

- `crates/engine/src/v3/agent.rs` — LayeredAgent, layer dispatch, cadence
- `crates/engine/src/v3/perception.rs` — StrategicView, perception layer
- `crates/engine/src/v3/strategy.rs` — StrategyLayer trait, Spread/Striker/Turtle impls
- `crates/engine/src/v3/operations.rs` — SharedOperationsLayer, task allocator
- `crates/engine/src/v3/tactical.rs` — SharedTacticalLayer, matchup reasoning, formations
- `crates/engine/src/v3/damage_table.rs` — DamageEstimateTable, surprise detection
- `crates/engine/src/v3/combat_log.rs` — CombatObservation struct, replay stream writer

## Constraints

- Agent layers must complete within their compute budget: strategy < 5ms,
  operations < 2ms, tactical < 1ms per stack.
- Damage table queries are O(1) HashMap lookups.
- All agent state must be serializable for replay reconstruction.
- Agent layer interfaces must be generic enough that a future NN-based agent
  can implement the same traits.
- Stacks live in GameState, not agent internals.
- Strategy reads only StrategicView, never raw game state.
- Command validation: validate-and-drop with `tracing::warn!`, no panics.
