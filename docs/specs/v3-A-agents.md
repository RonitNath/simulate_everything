# Spec: V3 Domain A — Agent Architecture

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2)
Sequencing: `docs/plans/v3-sequencing.md`
Implementation: Stream E (`docs/plans/v3-streamE-agent-behavior.md`)

**Updated 2026-04-14** to reflect Stream E (autonomous behavior) and Stream F
(compositional world model) landing. The original three-layer dispatch
(Strategy/Operations/Tactical) is retained but reframed: entities are
autonomous, faction layers influence rather than command.

## Vision

Entities are autonomous. Each entity has needs (hunger, safety, duty, rest,
social, shelter) that drive utility scoring → HTN goal decomposition → action
queue execution. The faction-level agent layers (Strategy, Operations,
Tactical) influence entity behavior by adjusting need weights and injecting
HTN methods — they do not micromanage individual entities.

Strategy sets the conditions under which autonomous entities make different
choices. Operations manages the availability of methods and resources.
Tactical coordinates during high-resolution-demand situations.

## Architecture

### Entity decision system (Stream E)

Three layers per entity:

1. **Needs + Utility Scorer** — per-entity needs decay over time. A utility
   scorer evaluates candidate goals against current needs using hierarchical
   buckets (survival > duty > maintenance > social) with geometric mean
   scoring. Archetype clustering amortizes evaluation across similar entities.
   Decision frequency LOD: combat-adjacent every tick, peaceful interior
   every 200 ticks.

2. **HTN Decomposition** — goals decompose into concrete action sequences via
   Hierarchical Task Network methods. Methods are data (preconditions,
   subtasks, duration, effects), not code. Domains compose freely:
   subsistence, material work, construction, transport, combat, social.
   MTR-based partial replan on precondition failure.

3. **Action Queue** — primitive actions (MoveTo, PickUp, ApplyTool, Transfer,
   Place, ModifyTerrain, Communicate, Consume, Rest, Wait) executed
   tick-by-tick or batch-resolved at strategic tier.

### Faction-level layers (original A1-A5, reframed)

**Strategy Layer** — personality-specific (Spread/Striker/Turtle).
Reads `StrategicView` (fog-of-war filtered perception). Now emits need weight
adjustments instead of direct commands:
- `SetPosture(Attack)` → all faction entities: `duty.combat_weight += 0.5`
- `SetEconomicFocus(Growth)` → all faction entities: `duty.production_weight += 0.3`
- `PrioritizeRegion` → entities near region: `duty.regional_weight += priority`

**Operations Layer** — injects HTN methods into faction domain registries:
- `FormStack` → adds `JoinStack` method for nearby military-ready entities
- `EstablishSupplyRoute` → adds `SupplyHaul` method
- `ProduceEquipment` → adds `CraftItem` method at relevant locations
- No longer issues `AssignTask` commands

**Tactical Layer** — priority interrupt on the action queue. When resolution
demand at entity's location exceeds threshold (not just combat — negotiation,
contested construction, competition), safety need spikes and the entity's own
system selects a combat/response goal. Tactical layer value: coordination
(formation, focus-fire, flanking), not individual action selection.

### Resolution demand (generalized engagement)

Replaces `stack_near_enemy()` with:
```
resolution_demand = conflict_intensity × outcome_uncertainty × stakes
```
Triggers: combat, negotiation, contested construction, competition, research.
Promotes region to higher tick rate when threshold exceeded.

### Compositional world model (Stream F)

No special entity types. Physical properties + affordance queries replace
typed Structure/Resource components. HTN methods specify physical constraints
(tool force, material temperature), not named entity types. A "forge" is
co-located fire source + anvil + hammer.

### Damage estimate table and combat learning

Retained from original spec. `DamageEstimateTable` with ~480 entries,
`CombatObservation` log for replay/NN training. Learning loop (observation →
table update → tactical adaptation) remains partially wired.

## Key Files

- `crates/engine/src/v3/agent.rs` — LayeredAgent, layer dispatch, cadence
- `crates/engine/src/v3/perception.rs` — StrategicView, perception layer
- `crates/engine/src/v3/strategy.rs` — Spread/Striker/Turtle personalities
- `crates/engine/src/v3/operations.rs` — Operations (method injection)
- `crates/engine/src/v3/tactical.rs` — Tactical coordination
- `crates/engine/src/v3/needs.rs` — EntityNeeds, NeedWeights, decay
- `crates/engine/src/v3/utility.rs` — UtilityScorer, Goal, response curves
- `crates/engine/src/v3/htn.rs` — HtnMethod, DomainRegistry, decomposition
- `crates/engine/src/v3/action_queue.rs` — ActionQueue, Action enum, execution
- `crates/engine/src/v3/resolution.rs` — resolution_demand_at()
- `crates/engine/src/v3/social.rs` — SocialState, opinion dynamics
- `crates/engine/src/v3/physical.rs` — PhysicalProperties, ToolProperties, tags
- `crates/engine/src/v3/affordance.rs` — AffordanceConstraint, find_affordance()
- `crates/engine/src/v3/damage_table.rs` — DamageEstimateTable
- `crates/engine/src/v3/combat_log.rs` — CombatObservation

## Deferred (post-V3)

- **Neural net insertion at 5 points** — utility scoring, HTN method selection,
  body control, tactical coordination, social reasoning. Classical system is
  bootstrap + fallback. See `docs/plans/future-neural-evolution.md`.
- **Multi-agent coordination** — allied agents sharing observations.
- **Agent-to-agent diplomacy** — truces, alliances, trade agreements.
