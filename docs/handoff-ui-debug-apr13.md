# Handoff: UI Debug Session — April 13, 2026

Branch: `ui-debug-session-apr13`

## What was done

### 1. Agent combat improvements (complete)
**File:** `crates/engine/src/v2/agent.rs`

- **Tightened engagement threshold**: `find_engageable_enemy` now requires `strength >= enemy * 0.8` for solo engagements (was 0.5). 2+ friendlies nearby still allows engagement at any strength.
- **Added disengage logic**: New `should_disengage()` helper. All 3 agents (Spread, Striker, Turtle) now evaluate engaged units each poll and issue `DisengageAll` when:
  - The unit is a general (always disengage)
  - Strength < 30 (too weak)
  - Projected 5-tick damage exceeds the 30% disengage penalty cost
- Tests pass: 84 V2 engine tests green.

### 2. Restart button (complete)
**File:** `frontend/src/V2App.tsx`

- Added `Restart` button in speed controls row, calls `POST /api/v2/rr/reset`.
- Also fixed `sendSpeed` to use HTTP API instead of dead WS send.

### 3. V2App partial changes (IN PROGRESS — needs completion + testing)
**File:** `frontend/src/V2App.tsx`

Changes started but **not yet built/tested**:

- **Server pause** (#3): State (`serverPaused`, `toggleServerPause`) is wired up but the **button is not yet added to the JSX**. Needs a pause/resume button in the speed controls area that calls `toggleServerPause()`.

- **Frame compaction** (#4): Logic added — keeps max 600 frames, compacts older frames by keeping every 5th. Includes viewIdx adjustment during compaction. **Needs testing** — verify scrubbing still works after compaction, no off-by-one on viewIdx.

- **Play at speed** (#5): Reworked the playback interval. New `playing` signal separate from `following`. Play button now plays at `tickMs` speed instead of jumping to live. Skip-to-end button (`⏭`) sets `following(true)` for live mode. **Needs testing** — verify play/pause/step/live all behave correctly.

- **Death animation** (#6): Ghost units are tracked in frame data — when a unit disappears between frames, it's added back with `_dead: true, _deadTick: tick` and carried forward for 8 ticks. **HexBoard.tsx rendering not yet implemented** — needs to render ghost units with fading opacity.

## What still needs to be done

### Priority 1: Finish the 6 items from the user's request

#### A. HexBoard.tsx changes (NOT STARTED)
All rendering changes go in `frontend/src/HexBoard.tsx`:

1. **Icon white background** (#1): The status icons (⚔, →, ◷) at ~line 477-489 are rendered as raw `<text>` elements. Add a small white circle behind them for contrast. Something like:
   ```tsx
   <circle cx={x} cy={y} r={iconRadius} fill="rgba(255,255,255,0.85)" />
   ```

2. **Icon scaling** (#2): Icons should be ~25% of the unit hex size. Currently `font-size` is `Math.max(6, s * 0.28)`. The icon position and size should scale proportionally — make the background circle `r = s * 0.12` and keep font-size at `s * 0.25`.

3. **Death animation** (#6): Ghost units (units with `_dead` flag) need visual treatment in the render loop. In `renderData()` around line 193, detect `(u as any)._dead` and render those units with fading opacity. Calculate opacity from `_deadTick`: `opacity = 1 - (currentTick - _deadTick) / 8`. Render as a dimmed hex with an "X" or skull, or just fade the player color to transparent.

#### B. Server pause button JSX (#3)
In V2App.tsx, add a button in the speed controls `<div>` (around line 415):
```tsx
<button
  class={styles.btn}
  style={{ "font-size": "10px", padding: "2px 6px", "font-weight": serverPaused() ? "bold" : "normal" }}
  onClick={toggleServerPause}
>
  {serverPaused() ? "Resume Server" : "Pause Server"}
</button>
```

#### C. Build + test
After all frontend changes:
```bash
cd frontend && npm run build
```
Then test in browser at `http://localhost:3333/v2`:
- Play/pause/step controls work correctly
- Play advances at server speed, not jumping to live
- Skip-to-end goes to live mode
- Server pause actually stops new ticks from arriving
- Restart starts a new game
- Death animation: units fade out over ~8 ticks when killed
- Icons have white backgrounds and scale with zoom
- Long games (2000+ ticks) don't freeze the tab

### Priority 2: Investigate combat display issues
The user reported confusing combat sequences where:
- Red units weren't stacked when expected
- Blue killed red then advanced, but no combat state shown
- Red reinforcements ate blue with no combat indicators
- A lone red unit faded out after 10 ticks with no visible enemy

This may be related to:
- Engagement edge requirements (units must be adjacent on a shared edge)
- The `AGENT_POLL_INTERVAL=5` meaning combat decisions only happen every 5 ticks
- Units moving through enemy hexes without engaging (pathfinding ignores enemies)
- The `0.8` threshold change possibly being too conservative

## Key files

| File | What |
|------|------|
| `crates/engine/src/v2/agent.rs` | Agent combat logic (engagement, disengage) |
| `frontend/src/V2App.tsx` | V2 round-robin app (WS, frames, controls) |
| `frontend/src/HexBoard.tsx` | Hex grid SVG renderer (units, icons, animations) |
| `frontend/src/v2types.ts` | TypeScript types for frames/units |
| `crates/web/src/v2_roundrobin.rs` | Server-side RR loop |
| `crates/engine/src/v2/combat.rs` | Combat engage/disengage/damage |
| `crates/engine/src/v2/sim.rs` | Tick order, movement, cleanup |

## Running locally

```bash
# Build and start server (serves frontend from frontend/dist)
cargo run --bin simulate_everything

# Rebuild frontend after changes
cd frontend && npm run build

# V2 RR is at http://localhost:3333/v2
# ASCII debug: curl "http://localhost:3333/api/v2/ascii?seed=1000&width=30&height=30&ticks=300&at=240"
```
