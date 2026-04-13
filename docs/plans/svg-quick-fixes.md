# SVG Quick Fixes — Immediate Visual Improvements

> Plan written 2026-04-13. These are low-effort changes to the current SVG renderer (HexBoard.tsx) to improve playability while the PixiJS rewrite is planned. See `frontend-rendering-overhaul.md` for the long-term plan.

## Context

Screenshots taken at ticks 10, 50, 100, 500, and end-state for both 2-player and 4-player games (seed 42, 30x30) revealed these issues:

1. Settlement icons (Farm/Village/City) are nearly invisible at default hex size
2. Territory coloring is too subtle to read ownership at a glance
3. Convoy routes and road connections are invisible or nearly so
4. Units blend into the hex grid, especially weak ones
5. Score bar is below the fold
6. Map feels static — no visual sense of growth between tick 100 and tick 1000

## Fixes (in priority order)

### 1. Settlement Icon Sizes — 2-3x increase

**File**: `frontend/src/HexBoard.tsx`

Current sizes vs proposed:

| Type | Current | Proposed |
|------|---------|----------|
| Farm circle radius | `size * 0.12` | `size * 0.25` |
| Village pentagon | `size * 0.25` wide | `size * 0.45` wide |
| City tower | `size * 0.35` wide | `size * 0.55` wide |

Also add a semi-transparent background plate (circle) behind each settlement icon:
- Farm: `playerRgba(owner, 0.3)` circle at `r = size * 0.3`
- Village: `playerRgba(owner, 0.3)` circle at `r = size * 0.45`
- City: `playerRgba(owner, 0.35)` circle at `r = size * 0.55`

The background plate makes settlements readable against any terrain biome.

### 2. Territory Fill Opacity — 25% to 35%

**File**: `frontend/src/HexBoard.tsx`, line where territory overlay renders

Change: `playerRgba(owner, 0.25)` → `playerRgba(owner, 0.35)`

Also: **don't suppress territory fill on settlement hexes**. Settlements should sit on top of territory color, not replace it.

### 3. Unit Brightness Floor — 0.3 to 0.5

**File**: `frontend/src/HexBoard.tsx`, `strengthBrightness()` function (lines 57-60)

Change minimum brightness from 0.3 to 0.5:
```
if totalStrength <= 0: return 0.5  // was 0.3
else: return 0.5 + 0.5 * log(1 + totalStrength) / log(1 + maxStrength)  // was 0.3 + 0.7 * ...
```

Even 1-strength units should be visible.

### 4. Score Bar Above the Fold

**Files**: `frontend/src/V2SimApp.tsx`, `frontend/src/V2App.tsx`

Move the score breakdown section (player stats + stacked score bar) from below the board to between the speed/layer controls and the board canvas. Keep it thin — horizontal layout, 24-32px tall per player.

### 5. Show Unit Count by Default

**File**: `frontend/src/HexBoard.tsx`

Change the condition for showing unit count numbers:
- Current: only when `showNumbers` is toggled on
- Proposed: show by default when `size > 10` (most zoom levels)

The `#/` toggle button should still work to force-hide them.

## What NOT to fix

These are not worth investing in given the PixiJS rewrite:
- Animated convoy routes (marching ants) — needs Canvas/WebGL for perf
- Road double-stroke technique — visual polish on a dying renderer
- Glass-morphism panels — CSS polish, low impact
- Settlement pulse animation — nice but not worth SVG overhead
- Fog of war smooth transitions — needs render texture approach

## Verification

After changes:
1. `cd frontend && bun run vite build` — builds clean
2. `npx tsc --noEmit` — no type errors
3. Run seed 42, 4 players, 2000 ticks — scrub to tick 300 and tick 1000
4. Confirm: settlements visually distinct at each tier, territory boundaries readable, weak units visible, score bar visible without scrolling
