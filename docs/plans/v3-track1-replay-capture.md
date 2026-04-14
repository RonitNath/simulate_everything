# V3 Track 1: Arena Replay Capture

## Context

V3.0 implementation is complete. The arena duel mode proves combat works
end-to-end. The V3 renderer (R1-R4) can display entities, projectiles, wounds,
and interpolated movement. But there's no way to play back a CLI-generated
game in the renderer.

The bench shows stalemate games (0-1 deaths in 500 ticks) because economy
commands are stubbed and operations routing is indirect. Arena mode bypasses
this and produces interesting fights. We want to see these fights rendered.

## Goal

Write replay files from CLI arena/bench that the V3 renderer can play back.

## Key files

- `crates/cli/src/v3bench.rs` — `run_arena()` and `run_bench_game()` need replay output
- `crates/web/src/v3_protocol.rs` — `build_snapshot()` (line 348), `DeltaTracker::build_delta()` (line 672), `V3Init` (line 112), `V3ServerToSpectator` enum (line 68)
- `crates/web/src/v3_roundrobin.rs` — look at how it builds `V3Init` from engine state
- `frontend/src/V3App.tsx` — WS message handler (line 151+), switches on `v3_init`, `v3_snapshot`, `v3_snapshot_delta`

## What to build

1. Add `--replay <path>` flag to v3bench. When present, write a JSONL file:
   - Line 1: `V3ServerToSpectator::Init { ... }` (serialized as JSON)
   - Subsequent lines: `V3ServerToSpectator::Snapshot` for tick 0 (full state),
     then `V3ServerToSpectator::SnapshotDelta` for each subsequent tick

2. In `run_arena()`: create a `DeltaTracker`, call `build_snapshot` for tick 0,
   `build_delta` for subsequent ticks. Write each as a JSON line to the file.
   Same for `run_bench_game()` but only when `--replay` is set.

3. Need to construct a `V3Init` in the CLI. The init needs terrain/height/material
   data from the heightfield. Look at how `v3_roundrobin.rs` builds it.

4. Add a `/v3/replay` route or page in the frontend that:
   - Accepts a JSONL file upload or URL
   - Feeds messages to the same V3App handler as if they came from WS
   - Plays back at configurable speed (tick_ms slider)

## Dependencies

None. Pure CLI + frontend, no engine changes.

## Verify

```bash
cargo build --release --bin simulate_everything_cli
./target/release/simulate_everything_cli v3bench --arena --replay /tmp/test.jsonl
# Then load /tmp/test.jsonl in browser at /v3/replay
```

## Commit

`feat(cli,frontend): arena replay capture — write JSONL, play back in renderer`
