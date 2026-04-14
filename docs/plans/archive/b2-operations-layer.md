# B2: Shared Operations Layer

## Context

The operations layer translates strategic directives (posture, economic focus,
priority regions) into entity-level operational commands. It replaces `city_ai.rs`'s
decision-making with a trait-based approach that all agent personalities share.

Depends on: B1 (agent layer types) — uses OperationsLayer trait, OperationalCommand enum.

## Design

### SharedOperationsLayer struct

Internal state:
- `posture: Posture` — current strategic posture (from directives)
- `economic_focus: EconomicFocus` — resource allocation mode
- `priority_regions: Vec<(Axial, f32)>` — weighted target areas
- `expansion_target: Option<Axial>` — where to send settlers
- `stacks: Vec<Stack>` — tracked unit groupings
- `next_stack_id: u32` — monotonic stack ID counter

### Decision pipeline (each execute() call)

1. **Update from directives** — parse StrategicDirectives to update posture, focus, priorities
2. **Population role assignment** — based on economic focus, emit ProducePerson for settlements
   that need growth. Role ratios: Growth→mostly farmers, Military→mostly soldiers,
   Infrastructure→mostly workers.
3. **Stack formation** — group nearby unengaged military units into stacks by proximity.
   Assign StackRole based on posture (Attack→Assault, Defend→Garrison, etc).
4. **Stack routing** — route stacks toward priority regions, expansion targets, or
   enemy positions based on posture.
5. **Infrastructure** — emit BuildStructure for depots at settlements, roads toward
   priority destinations.
6. **Supply lines** — identify settlements with surplus, forward positions with deficit,
   emit EstablishSupplyRoute.
7. **Expansion** — when posture is Expand, find good settlement sites and emit
   ProducePerson + route settler-like commands.

### V2 bridge

The operations layer produces OperationalCommands using B1 types. These are V3-ready.
For current V2 integration, city_ai.rs continues running in the sim loop unchanged.
The operations layer is the replacement that will be wired in when the sim tick
is updated (future chunk).

For testing: construct Observation objects and verify command output.

## Files

- `crates/engine/src/v2/operations.rs` — NEW: SharedOperationsLayer impl
- `crates/engine/src/v2/mod.rs` — add `pub mod operations;`

## Verification

```bash
cargo test -p simulate-everything-engine
cargo build -p simulate-everything-engine
```
