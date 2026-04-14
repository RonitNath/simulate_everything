# V3 Track 3: Economy Loop Wiring

## Context

V3.0 implementation is complete but the bench shows stalemate games: 500-tick
games end in draws with 0-1 deaths. The root cause: `apply_commands()` in
v3bench.rs stubs most operational commands. Agents issue Farm, Train,
ProduceEquipment, EquipEntity commands but they're no-ops.

Without economy, armies can't grow. Starting state is 5 soldiers + 30 civilians
per side, but civilians never train, workshops never produce, soldiers never
get equipped.

## Goal

Wire the remaining operational commands so agents can farm, build, produce
equipment, and grow their economy.

## Key files

- `crates/cli/src/v3bench.rs` — `apply_commands()` (line ~700). Currently stubs:
  Farm, Build, Craft, Garrison, Train, Idle, DisbandStack, ProduceEquipment,
  EquipEntity, EstablishSupplyRoute, FoundSettlement.
- `crates/engine/src/v3/operations.rs` — `SharedOperationsLayer::execute()` —
  produces the commands that need wiring
- `crates/engine/src/v3/state.rs` — `Entity`, `Structure`, `Resource` components,
  `Person { role, combat_skill }`, `Combatant`, `Equipment`
- `crates/engine/src/v3/mapgen.rs` — spawns starting entities (30 civilians,
  5 soldiers, structures, stockpiles). Check constants: STARTING_FOOD=500.0,
  STARTING_MATERIAL=200.0
- `crates/engine/src/v3/lifecycle.rs` — `spawn_entity()`, `contain()`

## What to wire (priority order)

1. **AssignTask::Farm** — entity with Farmer role at a Farm structure generates
   food. V2 had cell-level stockpiles. For V3 CLI: increment a per-player food
   float (simple bookkeeping, not entity-level). Track food per player in a
   `Vec<f32>` alongside the game loop.

2. **AssignTask::Train** — idle person becomes a Soldier. Change their `person.role`
   to `Role::Soldier`, add `Combatant::new()` and `Equipment::empty()`.

3. **ProduceEquipment** — workshop produces a weapon/armor entity. Spawn a new
   entity with `weapon_props` or `armor_props`, contained in the workshop.
   Costs material from the per-player stockpile.

4. **EquipEntity** — attach a weapon/armor entity to a soldier's equipment slots.
   Set `entity.equipment.weapon = Some(weapon_key)` and `contain()`.

5. **FormStack** — already wired.

6. **DisbandStack** — remove the stack from `state.stacks`.

## What NOT to wire (defer)

- FoundSettlement, EstablishSupplyRoute — complex placement logic
- Build — structure construction progress

## Also needed

Per-player food/material tracking in the bench game loop. Currently mapgen
spawns stockpiles as hex-level data (from V2). The simplest approach: add
`food: Vec<f32>` and `material: Vec<f32>` per player to the bench's game loop
state, seeded from mapgen starting values.

This means the game loop functions (`run_bench_game`, `run_ascii_game`,
`run_arena`) need a side-channel `EconomyState` struct that `apply_commands`
can read and mutate.

## Dependencies

None. Changes are in apply_commands + bench loop.

## Verify

```bash
cargo build --release --bin simulate_everything_cli
./target/release/simulate_everything_cli v3bench --ascii --seeds 0 --ticks 1000 --size 20x20
```

Observe: do soldiers get trained? Do they get weapons? Do stacks form and march?
The ASCII output should show entity counts growing over time. Soldier counts
should increase from the starting 5 as civilians train.

## Commit

`feat(cli): wire economy commands — farm, train, produce, equip, disband`
