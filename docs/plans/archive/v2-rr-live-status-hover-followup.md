# Plan: V2 RR Live Status Streaming and Tick-Gated Hover

Created 2026-04-13.

## Goal

Fix two UX/perf problems in the V2 RR client without changing the review feature set:

- live RR status should update on the same WebSocket connection as the board, instead of lagging behind on a 2-second HTTP poll
- hover inspection should not trigger full reactive work on every mouse movement while the game is running

The intended result is:

- capturable range, active capture state, pause state, and tick speed stay live even while the game is paused
- hover inspection feels stable and cheap while the game is advancing
- paused inspection still feels immediate

## Current State

As implemented now:

- `frontend/src/V2App.tsx` opens `ws/v2/rr` for game-state messages
- the same component separately polls `/api/v2/rr/status` and `/api/v2/rr/reviews` every 2 seconds
- `capturable_start_tick`, `capturable_end_tick`, `activeCapture`, `serverPaused`, and `tickMs` are driven by that poll
- `frontend/src/HexBoard.tsx` emits `onHoverHex` on every `mouseenter`
- `V2App` stores hover immediately in top-level state and derives inspector content from it immediately

Observed problems:

- live status can visibly lag behind the actual viewed tick because the board is WS-driven but capturable metadata is poll-driven
- when the server is paused there are no new snapshots, so embedding status only in `v2_snapshot` would freeze status updates
- hover currently causes large app-level recomputation during pointer motion

## Decision

Use a **separate WebSocket status message on the existing `ws/v2/rr` socket**.

Do not embed live RR status in `v2_snapshot`.

Reason:

- the board snapshot stream stops when the server is paused
- pause/resume/config/capture state must still update immediately
- using the same socket keeps transport unified without coupling status refresh to gameplay ticks

For hover behavior:

- keep only the **latest** hovered hex while live ticks are advancing
- resolve that pending hover on the next incoming game-data tick
- when the server is paused, resolve hover immediately

## Implementation Plan

### 1. Add a live RR status WS message

Extend `crates/web/src/v2_protocol.rs` with a new server-to-spectator message, e.g.:

- `type: "v2_rr_status"`

Payload should include only live RR metadata needed by the current UI:

- `game_number`
- `current_tick`
- `capturable_start_tick`
- `capturable_end_tick`
- `paused`
- `tick_ms`
- `active_capture`

Notes:

- `active_capture` should reuse the existing summary shape already returned by `/api/v2/rr/status`
- do not include pending/saved review lists in this live message
- keep `/api/v2/rr/status` as a fallback/debug endpoint, but not as the steady-state frontend source of truth

### 2. Broadcast status from the RR loop and controls

`crates/web/src/v2_roundrobin.rs` should gain a status-broadcast helper that reads current review status and emits `v2_rr_status`.

Broadcast it at these times:

- after `Init` / initial snapshot setup for a new game
- after every `record_tick` / review update in the main tick loop
- after pause
- after resume
- after reset request
- after tick speed changes
- after `flag_tick`
- after `start_capture`
- after `stop_capture`
- after finalization if the active capture state changes because of game end/reset

Catchup behavior:

- `spectator_catchup()` should include the latest `v2_rr_status` alongside `v2_init` and the latest `v2_snapshot`
- late joiners must not wait for the next tick to learn the current capturable window or paused state

### 3. Remove status polling from the frontend

In `frontend/src/V2App.tsx`:

- stop using the 2-second interval to refresh `/api/v2/rr/status`
- parse the new `v2_rr_status` message from the existing socket
- update these signals only from WS status in live mode:
  - `serverPaused`
  - `tickMs`
  - `liveTick`
  - `capturableStartTick`
  - `capturableEndTick`
  - `activeCapture`

Keep review-list fetching explicit and HTTP-based:

- initial load
- after flag
- after capture start/stop
- after delete
- after opening a saved review
- optionally once after `v2_game_end` if needed to pick up finalized partial bundles

Do not stream full review lists over WebSocket in this change.

### 4. Change hover into a tick-gated queue

Replace the current immediate hover application with two client-side states:

- `pendingHoverHex`: latest raw hover candidate from the board
- `resolvedHoverHex`: hover target currently used for board highlight + inspector

Rules:

- live RR mode, server not paused:
  - `HexBoard` hover events only update `pendingHoverHex`
  - on each incoming live `v2_snapshot`, promote `pendingHoverHex` to `resolvedHoverHex`
  - if several hover events happen before the next tick, only the latest one survives
- paused live RR mode:
  - hover updates `resolvedHoverHex` immediately
- saved review mode:
  - hover updates `resolvedHoverHex` immediately
- leaving the board clears both pending and resolved hover state

This should make hover effectively “another queued event” that resolves on the next tick, except while paused.

### 5. Keep heavy reactive work off raw pointer motion

The inspector and hover outline should read from `resolvedHoverHex`, not directly from raw pointer callbacks.

That means:

- `HexBoard` can still detect hover candidates per hex
- `V2App` should not recompute inspector content on every mouseenter while the live game is running
- the visible hover outline should also use `resolvedHoverHex`, so the SVG does not churn continuously during pointer sweeps

The inspector payload itself does not need to change in this task.

### 6. Keep both capture systems

Do not remove `Flag Tick`.

Product intent remains:

- `Flag Tick` is for short exact windows and quick triage
- `Start Capture` / `Stop Capture` is for long evolving situations

This task only changes how live status is delivered and when hover resolves.

## Concrete Acceptance Criteria

- The capturable range in `/v2/rr` no longer visibly trails the live viewed tick because of a 2-second status poll
- Pausing the RR still updates paused state and capture state immediately even if no new snapshots arrive
- Rapid live hover does not trigger inspector/highlight updates until the next tick
- While paused, hover remains immediate
- Start/stop capture and flag-tick actions update live status without waiting for polling
- Saved review playback still supports immediate hover inspection

## Test Plan

### Backend

- Add protocol-level coverage for serializing the new `v2_rr_status` message
- Add or extend `V2RoundRobin` tests to verify status broadcast on:
  - tick
  - pause/resume
  - config change
  - flag tick
  - start capture
  - stop capture

### Frontend

- Typecheck and build after removing the status polling path
- Verify that the scrubber capturable band uses WS-updated status
- Verify that the app still loads correctly on reconnect/catchup

### Manual

- In live RR mode, move the mouse quickly across many hexes and confirm inspector/highlight updates only on the next tick
- Pause the RR and confirm hover becomes immediate
- Start a segment capture, stop it, and flag a point tick; confirm active/capturable status updates immediately
- Reconnect a spectator tab and confirm it receives current board state plus current RR status without waiting for a new tick

## Files Likely To Change

- `crates/web/src/v2_protocol.rs`
- `crates/web/src/v2_roundrobin.rs`
- `frontend/src/V2App.tsx`

Possibly also:

- `frontend/src/HexBoard.tsx`
- `frontend/src/v2types.ts`
- `docs/architecture.md`

## Non-Goals

- changing the review bundle data model
- removing either point flagging or segment capture
- streaming full review lists over WS
- redesigning the inspector payload or layout
