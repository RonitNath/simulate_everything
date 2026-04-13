# V2 Integration Tests — Implementation Plan

Independent work item. Can run in parallel with any other V2 track.

## Goal

Add integration tests that run full or partial games and verify system-level invariants that unit tests don't cover. The engine has 80 unit tests but zero integration tests — no test runs the sim loop end-to-end checking that economic/combat/population/settlement systems compose correctly.

## Key Files

- `crates/engine/src/v2/runner.rs` — `run_game()` entry point
- `crates/engine/src/v2/sim.rs` — `tick()` sim loop
- `crates/engine/src/v2/state.rs` — `GameState`, all entity structs
- `crates/engine/src/v2/agent.rs` — `Agent` trait, `SpreadAgent`
- `crates/engine/src/v2/combat.rs` — engagement/damage
- `crates/engine/src/v2/observation.rs` — `Observation` struct
- `crates/engine/src/v2/mod.rs` — all constants

## Where to Put Tests

Create `crates/engine/src/v2/integration_tests.rs`. Add `#[cfg(test)] mod integration_tests;` to `crates/engine/src/v2/mod.rs`.

## Test Categories

### 1. Economic Conservation

Run a game for N ticks with a scripted agent that only issues `Pass` directives. Verify per tick:
- No stockpile goes negative
- Player aggregate totals match sum of owned hex stockpiles
- Material is not consumed unless a directive explicitly spends it
- Frontier stockpiles decay but total food/material accounting holds (production - consumption - decay = delta)

### 2. Settlement Lifecycle

Run a game long enough for population to grow, migrate, and form new settlements. Verify:
- Settlements form when population reaches `SETTLEMENT_THRESHOLD` (10)
- Migration only flows along owned connected territory
- Migration targets more fertile hexes over less fertile
- Population doesn't appear on hexes without migration or settler convoy

### 3. Settler Convoy Founding

Script: load settlers from a settlement, send to a remote owned hex. Verify:
- Source settlement retains at least `SETTLEMENT_THRESHOLD` population after departure
- Convoy delivers population to target hex
- Target hex becomes a settlement
- Production/role assignment works at the new settlement

### 4. Convoy Raiding

Set up a 2-player game. Player A sends a convoy through territory adjacent to Player B's unit. Verify:
- Convoy cargo transfers to raiding player's hex stockpile
- Convoy is removed from state
- Raider's hex gains the cargo amount

### 5. Road Movement Speed

Place a unit on a road (level 2 or 3) and an identical unit on bare terrain. Issue Move directives toward same-distance targets. Verify the road unit arrives in fewer ticks.

### 6. Height Effects on Combat and Movement

Set up two identical units on adjacent hexes with different heights:
- Engage them: verify uphill unit takes less damage
- Place a unit at base of steep hex, move uphill: verify movement cooldown is longer

### 7. Starvation Cascade

Place units on hexes with zero food production and no convoy resupply. Verify:
- Unit strength decreases from starvation
- Units die at strength 0
- Dead units cleaned from state

### 8. Frontier Stockpile Decay

Place stockpiles on hexes outside settlement support radius. Verify:
- Stockpiles decay per tick at `FRONTIER_DECAY_RATE`
- Stockpiles within settlement support radius do NOT decay
- Auto-accrual routes income to nearest settlement within support radius

### 9. Fog of War Correctness

Run a 2-player game, build observations for both players each tick. Verify:
- No enemy units visible outside vision radius
- No enemy stockpile data visible outside vision radius
- Own units always visible
- Stockpile owner masked to None outside vision

### 10. Game Convergence

Run 10 games with different seeds, SpreadAgent vs SpreadAgent, `TIMEOUT_TICKS` budget. Verify:
- At least 8/10 produce a winner (not all draws)
- No panics
- Final state internally consistent (no orphaned engagements, no units on invalid hexes)
- Timeout scoring produces a winner when tick limit reached

### 11. Replay Fidelity

Record a game, reconstruct state from replay. Verify reconstructed final state matches actual (unit positions, strengths, stockpiles, population counts).

## Implementation Notes

- Construct `GameState` via `mapgen::generate()` or manual setup — don't go through web layer.
- Small maps (15x15 or 20x20) and short tick budgets (100-500) for speed.
- For scripted agents, implement `Agent` trait with hardcoded directive sequences.
- Tests verifying "X does not go negative" should check every tick via `advance_game_tick()`, not just final state.
- Use `runner::run_game_loop()` with the `on_tick` callback for per-tick assertions.

## Commit

`test(v2): add integration tests for economy, settlement, combat, fog, convergence`
