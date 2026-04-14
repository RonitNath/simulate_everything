# V3 Track 4: Personality Differentiation

## Context

V3.0 has three strategy personalities (Spread, Striker, Turtle) with different
posture thresholds and economic focus defaults. The bench infrastructure
supports `--matchups all` to run every pair. But current results show all draws
because economy is stubbed and armies barely engage.

This track measures whether personalities produce meaningfully different
gameplay, and diagnoses what's blocking engagement.

## Goal

Measure whether Spread, Striker, and Turtle produce meaningfully different
gameplay. Produce a data-backed report.

## Key files

- `crates/cli/src/v3bench.rs` — bench infrastructure, `--matchups all`
- `crates/engine/src/v3/strategy.rs` — three personalities with different
  posture thresholds and economic focus defaults
- `crates/engine/src/v3/operations.rs` — `route_stacks()` uses posture to
  decide destinations; `assign_tasks()` uses economic focus for role ratios

## What to measure

1. Run `--matchups all --seeds 0-99 --ticks 2000 --size 20x20` for all 6 pairs
   (spread-vs-striker, spread-vs-turtle, striker-vs-turtle, plus mirrors).
   Capture JSON output.

2. Parse results and report:
   - Win rate per personality across all matchups
   - Average game length per matchup
   - Average deaths per matchup (proxy for combat engagement)
   - Economic divergence: final entity counts (do spread agents grow faster?)
   - Territory control patterns

3. If all games are draws (likely given current state), diagnose why:
   - Are stacks forming? (check final_soldiers in snapshots)
   - Are stacks routing toward enemies? (deaths > 0?)
   - Is economy growing? (entity counts increasing over snapshots?)

4. Write findings to `docs/v3-personality-report.md` with data tables.

## Dependencies

Partially depends on Track 3 (economy wiring). Without economy, personalities
differ only in posture/routing, not in army composition. Run the measurement
first to get a baseline, then re-run after Track 3 lands.

## Verify

The report should show either meaningful differentiation OR a clear diagnosis
of why games are stalemates, with specific numbers.

## Commit

`docs: V3 personality differentiation report — baseline measurements`
