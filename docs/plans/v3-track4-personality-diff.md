# V3 Track 4: Personality Differentiation

## Context

V3.0 has three strategy personalities (Spread, Striker, Turtle) with different
posture thresholds and economic focus defaults. The bench infrastructure can
already run fixed-seed matchups and emit per-game JSON, but this track needs a
dedicated reporting flow for the three real personalities rather than the
generic `--matchups all` behavior.

This track measures whether personalities produce meaningfully different
gameplay in the current V3 state, and diagnoses stall patterns per matchup.

## Goal

Measure whether Spread, Striker, and Turtle produce meaningfully different
gameplay. Produce a data-backed report.

## Key files

- `crates/cli/src/v3bench.rs` — bench infrastructure and report generation
- `crates/engine/src/v3/strategy.rs` — three personalities with different
  posture thresholds and economic focus defaults
- `crates/engine/src/v3/operations.rs` — `route_stacks()` uses posture to
  decide destinations; `assign_tasks()` uses economic focus for role ratios
- `docs/architecture.md` — CLI/report mode documentation

## What to measure

1. Add a dedicated report mode:
   `simulate_everything_cli v3bench --personality-report`

2. Run the full 3x3 ordered personality matrix over:
   - seeds `0-99`
   - ticks `2000`
   - size `20x20`
   - matchups:
     `spread/spread`, `spread/striker`, `spread/turtle`,
     `striker/spread`, `striker/striker`, `striker/turtle`,
     `turtle/spread`, `turtle/striker`, `turtle/turtle`

3. Persist reproducible artifacts:
   - raw per-game JSONL
   - aggregate JSON summary
   - checked-in markdown report at `docs/v3-personality-report.md`

4. Parse results and report:
   - Win rate per personality across all matchups
   - Average game length per matchup
   - Average deaths per matchup (proxy for combat engagement)
   - Economic divergence: final entity/soldier counts
   - Territory control patterns

5. Diagnose each matchup with explicit flags:
   - `zero_deaths`
   - `flat_entities`
   - `flat_soldiers`
   - `flat_territory`
   - `attrition_without_resolution`

6. Write findings to `docs/v3-personality-report.md` with data tables and
   revision context (git SHA + dirty flag).

## Verify

```bash
cargo test -p simulate-everything-cli v3bench -- --nocapture
cargo run -p simulate-everything-cli --bin simulate_everything_cli -- \
  v3bench --personality-report
```

The report should show either meaningful differentiation or a matchup-specific
diagnosis of stall behavior, with specific numbers.

## Commit

`feat(cli): add V3 personality report pipeline`
