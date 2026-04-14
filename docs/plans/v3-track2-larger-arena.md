# V3 Track 2: Larger Arena Scenarios

## Context

V3.0 implementation is complete. Arena duel mode works for 1v1 (null-vs-striker
and mutual combat). Combat loop is validated: movement → melee → wound → bleed → death.

We need to scale up to test group combat: tactical target selection, formation
behavior, mixed weapon types, and armor interaction.

Available weapon constructors: `iron_sword()`, `wooden_bow()`.
Available armor constructors: `leather_cuirass()`, `bronze_breastplate()`.

## Goal

5v5 and 10v10 arena fights with mixed weapons to test tactical target selection.

## Key files

- `crates/cli/src/v3bench.rs` — `run_arena()` function
- `crates/engine/src/v3/weapon.rs` — `iron_sword()`, `wooden_bow()` constructors
- `crates/engine/src/v3/armor.rs` — `leather_cuirass()`, `bronze_breastplate()`
- `crates/engine/src/v3/tactical.rs` — `SharedTacticalLayer::assign_targets()`,
  `score_target()` — this is where the damage table matchup reasoning lives

## What to build

1. Add `--arena-size N` flag (default 1). Spawns N soldiers per side.
   Place them in a cluster: side A around (50,50), side B around (200,50),
   each soldier offset randomly within a 30m radius.

2. Add `--arena-weapons mixed` flag. When set:
   - Side A: 60% swords, 40% bows
   - Side B: 60% swords, 40% bows
   Give some soldiers `leather_cuirass()` as torso armor.

3. Pre-form one stack per side containing all soldiers (up to 32 members,
   which is the SmallVec capacity on Stack.members).

4. Both sides get striker agents (mutual combat).

5. Output: same tick-by-tick format but summarize per-side (alive count,
   total wounds, avg blood) instead of per-entity.

## What to watch for

- Do archers hang back or walk into melee? (Archers should attack from range
  via projectile system — check if tactical layer handles ranged differently)
- Does `score_target` prefer armored or unarmored enemies?
- Does the formation system space out entities or do they clump?

## Dependencies

None. CLI only.

## Verify

```bash
cargo build --release --bin simulate_everything_cli
./target/release/simulate_everything_cli v3bench --arena --arena-mode mutual --arena-size 5
./target/release/simulate_everything_cli v3bench --arena --arena-mode mutual --arena-size 10 --arena-weapons mixed
```

## Commit

`feat(cli): larger arena scenarios — N-vs-N with mixed weapons and armor`
