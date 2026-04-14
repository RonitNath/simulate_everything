# V2 Persistent Enemy Adjacency Investigation

Date: 2026-04-13

## Summary

The reproduced RR issue is real, but the core problem is not same-hex overlap. The more common failure mode is that opposing units can remain adjacent for very long periods without any lasting engagement.

This is now detectable in the harness via a new `v2bench` check:

- `Check 6: Persistent Enemy Adjacency`

The reproduced RR game from the saved review bundles (`seed=1000`, `30x30`, `spread` vs `striker`) fails this check.

## Reproduction

Source of truth:

- `var/v2_rr_reviews/manifest.json`
- flagged game: `seed=1000`
- map: `30x30`
- agents: `spread #1` vs `striker #2`

Representative harness commands:

```bash
cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --diagnose --seeds 1000 --ticks 800 --size 30x30 --players 2 --agents spread,striker

cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --postmortem --seeds 1000 --ticks 800 --size 30x30 --players 2 --agents spread,striker
```

Observed output:

- `Check 6` fails
- `avg persistent standoffs/game: 5.00`
- `avg longest duration: 310.0 ticks`

Representative flagged pairs from postmortem:

- `P1:6` next to `P0:24` for `310 ticks` at `(0, 19)`
- `P1:13` next to `P0:20` for `170 ticks` at `(2, 13)`
- `P0:22` next to `P1:25` for `150 ticks` at `(4, 22)`
- `P1:7` next to `P0:22` for `120 ticks` at `(-2, 21)`

These match the RR review windows around ticks `340..404`.

## Root Cause

The root cause is a same-poll `engage` / `disengage` loop caused by sequential agent polling.

Relevant code:

- agent engagement choice: `crates/engine/src/v2/agent.rs`
- directive application: `crates/engine/src/v2/directive.rs`
- engagement creation: `crates/engine/src/v2/combat.rs`
- poll ordering: `crates/engine/src/v2/runner.rs`

Mechanism:

1. Player A is polled first.
2. Its agent sees an adjacent enemy and issues `Directive::Engage`.
3. `directive::apply_directives` applies that immediately.
4. `combat::engage` succeeds and both units become engaged.
5. Player B is polled later in the same poll cycle.
6. Player B now sees itself as engaged and may immediately issue `Directive::DisengageAll`.
7. `directive::apply_directives` applies that immediately too.
8. By end-of-poll, the pair is no longer engaged.

So in saved per-tick frames the units appear to remain adjacent and inert, even though a transient engage/disengage happened inside the poll cycle.

## Concrete Verification

I verified this directly on the reproduced `seed=1000` case with a temporary debug probe in the engine test harness.

At tick `340`:

- `spread` unit `P0:20` at `(2, 13)` sees adjacent `P1:13` at `(1, 14)`
- `find_engageable_enemy(...)` returns `P1:13`
- `spread` issues `Directive::Engage`
- applying that directive creates the engagement successfully

Then in the same poll cycle:

- `striker` is polled later
- it sees `P1:13` as engaged and adjacent to a much stronger enemy
- it issues `Directive::DisengageAll`
- by end-of-tick both `P0:20` and `P1:13` have `engagements.len() == 0`

This means:

- visibility is not the problem for this reproduced case
- `combat::engage` is not failing for the reproduced case
- the persistent standoff is produced by immediate sequential resolution

## Why This Is Happening

The current rules mix:

- immediate directive application per player
- immediate engagement creation
- immediate disengage directives on later player polls

The key contributing logic:

- Engage only if favorable: `find_engageable_enemy(...)`
- Disengage if losing: `should_disengage(...)`

In the reproduced case this creates asymmetric behavior:

- stronger side repeatedly chooses `Engage`
- weaker side repeatedly chooses `DisengageAll`

Because polling is sequential rather than simultaneous, the weaker side gets to erase the engagement before the tick completes.

## New Harness Coverage

The CLI harness now includes a new sampled analysis in `crates/cli/src/v2bench.rs`:

- `analyze_unengaged_adjacencies(...)`
- `Check 6: Persistent Enemy Adjacency`
- postmortem section:
  - `LONGEST UNENGAGED ENEMY ADJACENCIES`

Current threshold:

- suspicious if enemy adjacency persists for `>= 50` ticks without engagement

Important implementation detail:

- this is sample-based using `GameLog.unit_positions`
- it excludes ticks where either unit is engaged or has nonzero `engagement_count`
- it detects long-lived enemy-adjacent pairs that never remain engaged in saved frames

## Recommended Fix Directions

Any real fix should happen in engine behavior, not just in diagnostics.

Likely options:

1. Simultaneous directive resolution for combat state changes

- collect all directives for all players first
- resolve `Engage` / `Disengage*` in a deterministic phase
- avoid later polls cancelling earlier decisions in the same cycle

2. Sticky engagement rule

- once an engagement is created during a poll cycle, prevent same-cycle voluntary disengage
- or enforce a minimum dwell time of at least one full tick before `DisengageAll`

3. Engagement precedence rule

- if two adjacent enemy units are mutually visible and one side chooses `Engage`, do not allow the target to immediately nullify it before combat resolves at least once

4. Agent-side heuristic change only

- weaker side could stop using immediate `DisengageAll` in these edge cases
- this is weaker than an engine fix because the sequencing bug remains

My recommendation is to prefer an engine-level fix over an agent-only heuristic patch.

## Suggested Next Steps For Follow-up Agent

1. Reproduce with the harness command above and confirm `Check 6` still fails.
2. Decide whether to solve this in:
   - the poll/directive resolution model, or
   - combat/disengage rules.
3. Add a focused regression test in engine sim/combat for:
   - stronger unit engages
   - weaker adjacent unit tries to disengage in same poll cycle
   - engagement should persist at least through tick resolution
4. Re-run:

```bash
cargo test -p simulate-everything-engine
cargo test -p simulate-everything-cli --bin simulate_everything_cli v2bench -- --nocapture
cargo run --release -p simulate-everything-cli --bin simulate_everything_cli -- \
  v2bench --diagnose --seeds 1000 --ticks 800 --size 30x30 --players 2 --agents spread,striker
```

Success condition:

- `Check 6` for `seed=1000` drops materially
- the flagged adjacent pairs no longer persist for 100+ ticks without lasting engagement

