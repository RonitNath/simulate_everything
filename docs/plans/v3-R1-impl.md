# R1 Implementation Plan — PixiJS Scaffold + Hex Grid + SolidJS Shell

Source spec: `docs/specs/v3-R-renderer.md`
Wave: 0 (no dependencies)

## Scope

R1 delivers the V3 renderer foundation: PixiJS canvas with hex grid, camera
controls, height shading, territory overlay, infrastructure rendering, and
the SolidJS overlay shell (playback controls, score bar, layer toggles).

Entities, projectiles, LOD tiers, and interpolation are R2+.

## Data Source

The V3 backend already exists: `/ws/v3/rr` WebSocket, V3 RR API routes, V3
protocol types. R1 connects directly to V3 data. Wire types are already
defined in `frontend/src/v3types.ts`.

## Backend Changes (trivial)

1. Add `crates/web/templates/v3rr.html` — copy of v2rr.html with `__MODE__ = "v3rr"`
2. Add `/v3/rr` page route in `crates/web/src/main.rs`

## Frontend Files

### New files

| File | Responsibility |
|------|---------------|
| `frontend/src/V3App.tsx` | App shell, V3 WebSocket connection, frame buffering, playback state, REST API calls |
| `frontend/src/v3/HexCanvas.tsx` | PixiJS Application lifecycle, camera, hex grid rendering, territory, roads, settlements |
| `frontend/src/v3/PlaybackControls.tsx` | Play/pause, speed presets, frame scrubber, back-to-live button |
| `frontend/src/v3/ScoreBar.tsx` | Compact player scores strip (top bar) |
| `frontend/src/v3/LayerToggles.tsx` | Checkbox toggles for territory, roads, settlements, depots |
| `frontend/src/v3/render/grid.ts` | Pure functions: drawTerrain (hex grid + biome colors + height shading), drawTerritory, drawRoads, drawSettlements |
| `frontend/src/v3/render/camera.ts` | Camera state type, world-to-screen/screen-to-world transforms, viewport bounds |
| `frontend/src/styles/v3.css.ts` | Vanilla-extract styles for V3 components (flex layout, panels, controls) |

### Modified files

| File | Change |
|------|--------|
| `frontend/src/index.tsx` | Add `v3rr` mode → render V3App |

## Architecture Decisions

1. **Entity map stub**: HexCanvas creates the entity map data structure (plain
   `Map<number, EntityState>`) but does not render entities — that's R2. The
   map is ready for upsert when R2 lands.

2. **Rendering modules are pure functions**: `render/grid.ts` exports functions
   that take PixiJS Graphics objects + data, draw, and return. No component
   state. HexCanvas calls them from its render pipeline.

3. **Camera as plain variables**: Not signals. PixiJS manages rendering
   directly via `world.scale/position`. Matches V2 HexCanvas pattern.

4. **Hex geometry**: Port from V2 HexCanvas — offset even-r coordinates,
   same `hexCenter`, `drawHexPath`, `pixelToHex` functions. V3 entities use
   axial (q, r) but terrain is indexed by row*width+col (offset coords).

5. **WebSocket connection**: Same reconnection pattern as V2App (exponential
   backoff 500ms → 2000ms). Handles `v3_init`, `v3_snapshot`, `v3_snapshot_delta`,
   `v3_game_end`, `v3_config`, `v3_rr_status`.

6. **Frame buffer**: Store up to 600 V3Snapshot frames. Compact to keep every
   5th when exceeding limit. Same pattern as V2App.

7. **CSS flex layout**: App shell is `flex column 100vh`. Main area is
   `flex row`. Canvas fills remaining space via `flex: 1`. Inspector placeholder
   (future) will be a sibling flex item. PixiJS `resizeTo` handles resize.

8. **Height shading**: Darker = lower elevation, lighter = higher. Formula:
   `brightness = 0.6 + 0.4 * height` applied to biome base color. Same as
   V2 `biomeColorNum` function.

## Build Order

1. Backend: template + route (2 files)
2. CSS styles (`v3.css.ts`)
3. Render modules (`camera.ts`, `grid.ts`)
4. Components (`HexCanvas`, `PlaybackControls`, `ScoreBar`, `LayerToggles`)
5. App shell (`V3App.tsx`)
6. Entry point (`index.tsx`)
7. Build + verify in browser

## Verification

- `cd frontend && bun run build` succeeds
- Navigate to `/v3/rr` in browser
- Hex grid renders with biome colors and height shading
- Camera zoom (0.2x-5x) and pan work
- Territory overlay shows player colors
- Roads render between connected hexes
- Settlements render (farm/village/city shapes)
- Play/pause, speed controls, scrubber work
- Score bar shows player stats
- Layer toggles show/hide territory, roads, settlements
- CSS flex layout adapts to window resize
- No console errors, no memory leaks on unmount
