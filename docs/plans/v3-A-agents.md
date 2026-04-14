# V3 Domain: A — Agent Architecture

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Agent Architecture section)
Sequencing: `docs/plans/v3-sequencing.md`

## Purpose

Three-layer agent architecture: Strategy (posture, priorities), Operations
(production, logistics, stack management), Tactical (per-stack combat). Shared
infrastructure with personality differentiation only at the Strategy layer.
Agents must reason about weapon-armor matchups using cached damage lookup tables
updated from combat observations.

## Design Questions

### A.1 Layer Interfaces

- The spec defines three command types: StrategicDirective, OperationalCommand,
  TacticalCommand. Are these enums or trait objects? Enums are simpler and faster.
  Trait objects allow extensibility. For a fixed set of command types, enums.
- Cadence: strategy every ~50 game-seconds, operations every ~5, tactical every
  tick for engaged stacks. How is "engaged stack" detected? Any stack with an
  entity whose continuous position is within N world-units of an enemy entity?
  What's N? Probably max weapon reach + some buffer (200m? 300m?).
- Layer state: each layer maintains state between invocations. Strategy keeps
  current posture, region priorities. Operations keeps stack assignments, supply
  routes. Tactical keeps per-stack engagement plans. Where does this state live?
  Inside the agent struct (per-player), or in shared game state?
- Command validation: what happens when an agent issues an invalid command?
  (e.g., RouteStack to an entity that no longer exists, EquipEntity with
  equipment that was destroyed.) Silently ignore? Log warning? The engine should
  be resilient to stale commands.

### A.2 Operations Layer

- This is the biggest layer. It manages the entire economy and military logistics.
  How should it be decomposed internally? Subsystems:
  - Population manager: role assignment, training pipeline
  - Equipment manager: production queues, distribution priorities
  - Stack manager: formation, composition, routing
  - Supply manager: convoy scheduling, depot placement, route planning
  - Infrastructure manager: road building, structure placement
  - Settler manager: expansion site selection, settler dispatch
- Should these be separate structs that operations orchestrates, or methods on a
  single OperationsLayer struct? Separate structs allow independent testing.
- Equipment profiles: the spec has `equipment_profile: EquipmentProfile` on
  RequestStackFormation. What does an equipment profile look like? "Sword + shield +
  leather armor" vs "Bow + cloth tunic" vs "Spear + chain mail"? This determines
  what the operations layer requests from workshops.
- How does operations decide what equipment to produce? Based on strategic
  directives (Attack posture → more weapons, Defend → more armor?) and on the
  damage lookup table (enemy has plate → produce more maces/crossbows)?

### A.3 Tactical Layer

- Weapon-armor matchup reasoning is the key intelligence. The tactical layer sees
  local entities and must decide: which of my units attack which enemies? The
  decision uses the damage lookup table: "my slingers vs their plate → low
  effectiveness, redirect to their archers."
- Formation types for V3.0:
  - Line: entities in a row, all facing forward. Shield wall.
  - Column: entities in a file, for marching. Vulnerable to flanking.
  - Wedge: arrow shape, for breaking through lines.
  - Square: defensive formation, all-around facing.
  - Skirmish: loose spacing, for ranged units.
- Formation assignment: tactical decides formation based on force composition and
  terrain. Spear + shield wall → Line facing enemy. Archers → Skirmish behind
  the line. Cavalry → Wedge for charge.
- Retreat decision: when does tactical order retreat? Force ratio threshold? Or
  based on casualty rate observed over last N ticks? "We're losing 3 entities per
  tick and they're losing 1 → retreat." The observation-based approach is more
  robust.
- Facing management: each entity faces its assigned target. When no target, face
  the nearest threat. When in formation, face the formation direction (overrides
  individual target facing). Trade-off: formation discipline vs individual threat
  response.

### A.4 Strategy Personalities

- Spread: economy-first, expand territory, defend. Requests Growth economic focus,
  Expand posture. Transitions to Consolidate when borders meet enemy. Transitions
  to Attack only when overwhelmingly advantaged.
- Striker: military-first, aggressive. Requests Military economic focus, Attack
  posture. Prioritizes regions near enemy settlements. Transitions to Defend only
  when losing badly.
- Turtle: infrastructure-first, defensive. Requests Infrastructure economic focus,
  Defend posture. Builds dense road networks, many depots. Transitions to Attack
  very late, only when economy is dominant.
- Posture transition conditions: what signals trigger transitions? Territory ratio,
  military strength ratio, economic output ratio, casualty rate? Should be tunable
  constants.

### A.5 Damage Lookup Table

- Key: (WeaponType, ArmorType) → ExpectedOutcome. What's the granularity of
  WeaponType and ArmorType? Per specific item (IronSword, BronzeSpear) or per
  category (Sword, Spear)?
- Moka cache configuration: max entries, TTL. With ~6 weapon types × ~5 armor
  types = 30 entries, the table is tiny. TTL should be long (1000+ game-seconds?)
  with explicit invalidation when the agent observes a surprising outcome.
- "Surprising outcome" detection: if the empirical wound_rate diverges from the
  cached expected wound_rate by > threshold, mark the entry stale. What threshold?
- Initialization: the theoretical lookup table computed from material physics at
  game start. This is a function of the weapon/armor property values. Can be
  computed once and shared across all agents (it's physics, not learned behavior).
  The empirical overrides are per-agent.

### A.6 Observation Journal

- Log format: `(tick, attacker_weapon_type, defender_armor_type, zone, penetrated: bool, severity, stagger: bool)`.
- Storage: ring buffer per agent? Max entries? At 500 entities in combat, ~200
  attacks per tick, that's 200 entries/tick × 1000 ticks = 200k entries per
  extended battle. Trim by aggregating: after N raw entries for a (weapon, armor)
  pair, compute running average and discard raw entries.
- This is future NN training data. What additional fields would a NN need that we
  should log now? Distance, height_diff, attacker_skill, defender_stamina,
  angle_of_attack?

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| A1 | 1 | W1 | Layer traits, directive/command enums, cadence dispatch |
| A2 | 2 | A1, M2 | Operations layer (all subsystems) |
| A3 | 3 | A2, W3, D2 | Tactical layer (matchup reasoning, formation, retreat) |
| A4 | 3 | A3 | Strategy personalities (Spread, Striker, Turtle) |
| A5 | 3 | D2, W2, W3 | Damage lookup table (moka) + observation journal |

## Key Files (Expected)

- `crates/engine/src/v3/agent.rs` — Agent trait, layer dispatch, cadence
- `crates/engine/src/v3/strategy.rs` — StrategicDirective, personality impls
- `crates/engine/src/v3/operations.rs` — OperationalCommand, subsystem managers
- `crates/engine/src/v3/tactical.rs` — TacticalCommand, matchup reasoning, formations
- `crates/engine/src/v3/damage_table.rs` — moka cache, theoretical + empirical tables
- `crates/engine/src/v3/journal.rs` — observation journal, aggregation

## Constraints

- Agent layers must complete within their compute budget: strategy < 5ms,
  operations < 2ms, tactical < 1ms per stack. Measure with the bench harness.
- Damage lookup table queries must be O(1) (hash map lookup, moka-cached).
- All agent state must be serializable for replay reconstruction.
- Agent layer interfaces must be generic enough that a future NN-based agent
  can implement the same traits.
