# Plan: V2 RR Review UX and Long-Engagement Diagnostics

Created 2026-04-13.

## Goal

Make V2 RR debugging practical for long-running, hard-to-pinpoint combat pathologies without bloating the live RR loop.

The immediate driver is a real review workflow:
- pause the local RR replay in the browser
- scrub to the exact viewed tick that looks suspicious
- flag or capture that segment while the live RR server continues in the background
- open the saved review later, inspect the board, and delete it when done

The current point-flag system is good enough for short windows and exact overlap reproduction. It is not yet good enough for:
- engagements that develop over hundreds of ticks
- diagnosing why a duel lasted too long
- visually locating suspect hexes quickly in the board view

## Current State

Already implemented:
- server-side RR review recorder with a 600-tick ring buffer
- point flags saved as exact replay windows
- review bundle persistence under a single review directory
- RR UI support for flagging the currently viewed tick
- review list/open/delete flow
- basic overlap anomaly capture for cross-owner same-hex occupancy

Current limitations:
- only fixed `tick-5 .. tick+10` point capture
- no start/stop capture for long segments
- no replay scrubber overlays for saved or active capture ranges
- no per-hex hover inspector
- no coordinate labels to help communicate board locations
- no long-engagement heuristic

## Decision

Do not add long-engagement analysis directly to the live RR review path first.

Reason:
- long engagements are a behavioral diagnostic, not just a replay-capture concern
- the repo already has a V2 testing and benchmarking harness in `crates/cli/src/v2bench.rs`
- the harness already runs games with `GameLog`, computes behavioral checks, and is the right place to evaluate heuristic quality across many seeds
- the live RR path should stay focused on exact capture, review, and lightweight anomaly surfacing

That means this work splits into two tracks:

1. RR review UX and capture improvements
2. Offline long-engagement diagnostics in the V2 bench harness

## Track 1: RR Review UX and Capture

### 1. Start/stop segment capture

Add a segment capture flow alongside the existing one-shot point flag:
- `Start Capture` at the currently viewed tick
- `Stop Capture` at the currently viewed tick

Rules:
- start/stop are keyed to the browser's selected viewed tick, not the live server tick
- the live RR loop keeps running
- the selected viewed tick must still be inside the server capturable range
- one active capture per game is sufficient for v1
- if the game ends or resets before stop, save the partial segment and mark it incomplete

Bundle model additions:
- `kind: "point" | "segment"`
- `start_tick`
- `stop_tick`
- `flagged_ticks`
- `complete`

### 2. Replay progress bar overlays

The RR scrubber should show review state directly:
- saved point/segment bundles as muted bands
- active capture as a stronger highlighted band
- flagged point ticks as markers
- the server capturable range as a subtle background span

This solves the operator problem of not knowing what is already captured and what section is still collectible.

### 3. Hex coordinate labels

Add lightweight always-visible labels to improve verbal debugging.

Scope:
- board-edge row/column labels, not text on every cell
- keep labels subtle enough to avoid overwhelming the board
- use the existing even-r offset display coordinates as the primary human-facing reference

### 4. Hover inspector

Add a per-hex hover inspector to the board view.

Minimum payload:
- offset coordinates
- axial coordinates
- terrain and settlement info
- owner / stockpile owner
- units on the hex
- unit id, owner, strength, engaged state
- whether the hovered hex participates in a saved anomaly for the selected tick

This is the main usability upgrade for manual review.

## Track 2: Long-Engagement Diagnostics in `v2bench`

### Why it belongs there

The harness already supports:
- deterministic fixed-seed runs
- `GameLog` recording
- `--diagnose` checks for behavioral health
- postmortem-style inspection

That makes it the right place to iterate on heuristics like:
- "how often do 1v1 engagements last too long?"
- "which agents produce sticky, indecisive contact?"
- "do changes reduce pathological duel duration across many seeds?"

### Proposed heuristic direction

Initial heuristic target:
- detect engagements between the same pair of units that remain continuously engaged for too many sampled ticks

Important caveat:
- current `GameLog` is too sparse to support a robust version of this yet
- `runner.rs` only records `unit_positions` every 10 ticks
- `GameEvent` logs `EngagementCreated`, but not explicit `EngagementEnded`
- current samples only expose `engaged: bool`, not which enemy or edge a unit is engaged with

Because of that, the first implementation should be harness-first and may require a small `GameLog` extension, likely one of:

Option A:
- add engagement-pair samples to `GameLog`
- sample active `(unit_id, enemy_id)` pairs on the same cadence as unit positions

Option B:
- add explicit `EngagementEnded` events
- reconstruct durations from create/end events

Option C:
- add a dedicated post-tick engagement snapshot ring only when `GameLog` is enabled

My current bias is Option B plus a small sampled context layer:
- `EngagementCreated`
- `EngagementEnded`
- keep existing position samples

That gives cleaner duration accounting without needing a full dense combat trace.

### Bench integration plan

Extend `crates/cli/src/v2bench.rs` with a new behavioral check:
- `Check 5: Long Engagement Rate`

Candidate output:
- average number of long engagements per game
- average longest engagement duration
- count of 1v1 engagements over threshold
- optional top worst seeds for follow-up replay/postmortem

Candidate threshold, subject to tuning:
- warning at 100 ticks continuous engagement
- report exact durations in diagnose output

This check should be used to tune the heuristic before any RR-side surfacing.

### RR integration after harness validation

Only after the heuristic is validated in `v2bench` should RR review add a lightweight summary such as:
- `long_engagement_count`
- `max_engagement_duration`
- flagged pair summaries in saved review metadata

The live RR should not become the place where the heuristic is invented.

## Recommended order

1. Keep the current RR point-flag flow as-is.
2. Add this plan file so the deferred work stays concrete.
3. Inspect `v2bench`, `gamelog`, and `runner` to define the smallest viable long-engagement logging extension.
4. Implement the harness-side heuristic first.
5. Once the heuristic is trusted, add RR segment capture and board UX improvements.

## Concrete next tasks

1. Add a harness-oriented design note for long-engagement metrics after inspecting `v2bench` and `gamelog` in more detail.
2. Decide whether long-engagement duration should be event-based, sampled, or hybrid.
3. Implement `Check 5` in `v2bench` and validate it on a small seed set.
4. Return to RR UX:
   - start/stop capture
   - scrubber overlays
   - coordinate labels
   - hover inspector
