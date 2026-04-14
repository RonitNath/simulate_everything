# Spec: V3-R Renderer — PixiJS WebGL with SolidJS Overlay

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Frontend section)
Sequencing: `docs/plans/v3-sequencing.md`

## Vision

Replace the V2 SVG/PixiJS hybrid renderer with a modular PixiJS WebGL renderer that
displays entities at continuous world positions with interpolated motion, LOD-based
rendering tiers, projectile flight visualization, and corpse persistence. SolidJS
manages UI overlay panels (controls, inspector, tooltip) while PixiJS owns the
canvas render loop. The architecture is built for delta-mode protocol upgrades
without renderer changes.

## Use Cases

### UC1: Spectator watches a live battle at close zoom
The viewer zooms in on a hex cluster where combat is active. Individual soldiers
render at their continuous (x, y) positions with facing arrows, weapon/armor icons,
blood bars, and wound count indicators. Projectiles (arrows, bolts) fly between
combatants with smooth interpolated parabolic or flat trajectories. A soldier dies
and plays a fall animation, transitioning to a desaturated corpse with equipment
visible on the ground. The viewer hovers over a living soldier and sees a tooltip:
ID, owner, role, blood %, weapon type, wound count. Clicking opens a persistent
side-panel inspector with full EntityInfo detail.

*Implementation: entity map stores prev/curr positions, PixiJS render loop lerps at
60fps. Death triggers `state: 'dying'` with fall animation, then `state: 'corpse'`.
Inspector reads entity map entry for selected ID via signal bridge.*

### UC2: Spectator watches a strategic overview at far zoom
The viewer zooms out to see the full 30x30 map. Individual entities are invisible.
Hexes show player-colored density heatmap (intensity proportional to entity count).
Settlement dots mark structure positions. Road network lines connect hexes. Corpse
fields render as subtle ground markers (darkened hex tint). The viewer can identify
army concentrations and territory control at a glance.

*Implementation: LOD tier `far` (hex < 20px). Hex-bucket entity counts aggregated
from entity map. Density rendered as alpha-scaled player-color fill per hex.*

### UC3: Spectator scrubs through a replay
The viewer pauses live playback, drags the frame scrubber backward. The entity map
receives historical tick data and renders it with the same interpolation, LOD, and
corpse logic. The viewer can step frame-by-frame to analyze a combat exchange.
Switching back to live mode resumes real-time updates.

*Implementation: playback controls (SolidJS component) manage frame index. WebSocket
or replay file provides tick data. Entity map applies upserts from any source — live
or replay.*

### UC4: Spectator resizes window with inspector open
The viewer opens the inspector side panel, reducing canvas width. The PixiJS renderer
resizes automatically via ResizeObserver (CSS flex layout). The hex grid and entity
positions adjust to the new viewport without manual resize calls. Closing the inspector
restores full canvas width.

*Implementation: CSS flex container with canvas filling remaining space. PixiJS
`resizeTo` option uses ResizeObserver internally.*

## Architecture

### Components

| Component | Responsibility | Owns |
|-----------|---------------|------|
| `V3App.tsx` | App shell, WebSocket connection, frame routing | Connection state, playback mode |
| `PlaybackControls.tsx` | Play/pause, speed presets, frame scrubber, back-to-live | Playback signals (playing, tickMs, viewIdx, following) |
| `HexCanvas.tsx` | PixiJS Application lifecycle, render loop, camera, all canvas rendering | PixiJS app, world container, render layers, entity map, camera state |
| `Inspector.tsx` | Click-selected entity detail panel | Selected entity ID (signal) |
| `Tooltip.tsx` | Hover tooltip near cursor | Tooltip data (signal) |
| `ScoreBar.tsx` | Player scores, population, territory counts | Reads from latest tick data |
| `LayerToggles.tsx` | Toggle visibility of territory, roads, settlements, etc. | Layer visibility set (signal) |

### Data Flow

```
WebSocket
  │
  ├─ TickMessage { entities: EntityInfo[], projectiles: ProjectileInfo[], ... }
  │
  V3App.tsx
  │  Passes tick data to HexCanvas via props
  │  Updates playback signals
  │
  HexCanvas.tsx
  │  Owns entityMap: Map<id, EntityState>  (plain object, NOT reactive)
  │  Owns projectileMap: Map<id, ProjectileState>  (plain object)
  │  On tick: upsert entities/projectiles into maps, mark absent entities stale
  │  On render frame (60fps): lerp positions, render by LOD tier
  │
  ├─ Tooltip signal ──► Tooltip.tsx (SolidJS HTML overlay)
  ├─ Selected ID signal ──► Inspector.tsx (SolidJS HTML side panel)
  └─ Player data ──► ScoreBar.tsx (SolidJS HTML overlay)
```

### Entity Map

The entity map is the central data structure. It is a plain `Map<number, EntityState>`
**outside SolidJS reactivity**. The WebSocket handler writes to it, the PixiJS render
loop reads from it at 60fps. Only UI-facing state (selected entity ID, tooltip data,
playback controls) uses SolidJS signals.

```typescript
interface EntityState {
  // Wire data (latest tick)
  info: EntityInfo;

  // Interpolation
  prevPos: { x: number; y: number; z: number };
  currPos: { x: number; y: number; z: number };
  prevFacing: number;
  currFacing: number;
  lastTickTime: number;  // timestamp when currPos was set

  // Lifecycle
  state: 'alive' | 'dying' | 'corpse';
  deathTime?: number;  // timestamp when death started (for fall animation)
}

interface ProjectileState {
  info: ProjectileInfo;
  prevPos: { x: number; y: number; z: number };
  currPos: { x: number; y: number; z: number };
  velocity: { x: number; y: number; z: number };
  lastTickTime: number;
}
```

**Upsert logic per tick:**
1. For each EntityInfo in the tick: if ID exists in map, shift `currPos` to `prevPos`,
   update `currPos` and `info`. If ID is new, set `prevPos = currPos` (snap, no lerp
   on first frame).
2. For entities in the map but absent from the tick: if `state === 'alive'` and absent
   for N consecutive ticks, transition to stale/removal candidate. (In full-snapshot
   mode, absence means death or despawn. In future delta mode, absence means no change.)
3. Dead entities (blood <= 0): transition `state` to `'dying'`, record `deathTime`.
   After fall animation completes (~300ms), transition to `'corpse'`. Corpses remain
   in the map indefinitely — position frozen, equipment visible at close zoom.

**Delta-readiness:** The entity map + upsert pattern means delta mode is just "fewer
entries in the update array." The renderer code does not change. The `full_state` flag
(staged in P spec) controls whether absence means "dead/removed" (full snapshot) or
"unchanged" (delta mode).

### Wire Format

Two separate arrays per tick, as decided in the P spec:

```typescript
interface EntityInfo {
  id: number;
  owner: number | null;
  x: number; y: number; z: number;     // continuous world position
  hex_q: number; hex_r: number;         // derived hex (convenience, used for stacking)
  facing: number | null;                // radians
  role: string | null;                  // "Idle" | "Farmer" | "Worker" | "Soldier" | "Builder"
  blood: number | null;                 // 0.0-1.0
  stamina: number | null;              // 0.0-1.0
  wound_count: number;                 // active wound count
  weapon_type: string | null;          // "sword" | "bow" | "spear" | etc.
  armor_type: string | null;           // "leather" | "chain" | "plate"
  resource_type: string | null;
  resource_amount: number | null;
  structure_type: string | null;       // "Farm" | "Village" | "City" | "Depot"
  build_progress: number | null;
  contains_count: number;
}

interface ProjectileInfo {
  id: number;
  x: number; y: number; z: number;     // continuous world position
  vx: number; vy: number; vz: number;  // velocity vector
}
```

### Camera

Camera state is plain variables (not signals), managed by PixiJS:

```typescript
let cameraX = 0;
let cameraY = 0;
let zoom = 1.0;
```

- **Zoom**: mouse wheel, smooth lerp toward target zoom. Range: 0.1x to 5x.
  Zoom center: under mouse cursor.
- **Pan**: pointer drag. Pointer capture on drag start, release on drag end.
- **World-to-screen transform**: `screenPos = (worldPos - camera) * zoom`
- **Viewport bounds**: derived from camera position + zoom + canvas size.
  Used for culling.

### LOD Tiers

Tier determined by hex pixel size (`HEX_SIZE * zoom`):

| Tier | Hex Size | Entity Rendering | Terrain | Projectiles |
|------|----------|-----------------|---------|-------------|
| **Close** | > 80px | Individual entities: facing arrow, weapon/armor icons, blood bar, wound indicators | Individual hex outlines + biome fill + height shading (darker = higher, lighter = lower) | Individual arrows/bolts oriented along velocity |
| **Mid** | 20-80px | Stack badges: player-colored circle with count text, grouped by hex_q/hex_r | Individual hex outlines + biome fill + height shading | Volley clusters (aggregate) |
| **Far** | < 20px | Density heatmap: player-colored alpha per hex proportional to entity count | Chunk textures (pre-rendered hex groups) | Not rendered |

**Corpse rendering by LOD:**
- Close: fallen sprite, desaturated, equipment visible on ground
- Mid: included in hex entity count (distinguished by owner or dimmed badge)
- Far: subtle ground darkening on hexes with many corpses

**Stacking (mid zoom):** group entities by `hex_q, hex_r` from EntityInfo. Hysteresis
in the S domain prevents boundary flicker. Multi-hex stacks show one badge per hex,
same player color, with that hex's member count. No spatial clustering.

### Interpolation

Every render frame (60fps), for each entity in the entity map:

```
t = clamp((now - lastTickTime) / tickInterval, 0, 1)
renderPos = lerp(prevPos, currPos, t)
renderFacing = shortestArcSlerp(prevFacing, currFacing, t)
```

- **Spawn** (no previous position): `prevPos = currPos`, snap to position, `t = 1`.
- **Death** (`state: 'dying'`): play fall animation over ~300ms (rotate sprite to
  horizontal), then freeze as `state: 'corpse'`. Stop updating position.
- **Corpse**: render at frozen position, skip interpolation.
- **Projectiles**: same lerp pattern. Projectiles move fast, so interpolation is
  critical for visual smoothness.

### Rendering Layers (bottom to top)

1. **Hex grid**: biome-colored hexagons with height shading (darker = lower, lighter = higher)
2. **Territory overlay**: player-colored semi-transparent fill per owned hex
3. **Infrastructure**: roads (line segments between hex centers), structures (snapped to hex centers)
4. **Corpse layer**: dead entities rendered at frozen positions
5. **Entity layer**: living entities at interpolated positions
6. **Projectile layer**: projectiles at interpolated positions, oriented along velocity
7. **Badge/indicator layer**: stack badges (mid zoom), wound indicators (close zoom), facing arrows

### SolidJS ↔ PixiJS Bridge

SolidJS reactivity drives PixiJS state updates, not vice versa:

- `createEffect` watches tick data props → calls imperative entity map upsert
- `createEffect` watches layer toggle signals → shows/hides PixiJS containers
- Camera state is plain variables — PixiJS manages rendering directly
- **Tooltip**: PixiJS hover handler reads entity map, writes to a `createSignal<TooltipData>`.
  SolidJS `<Tooltip>` component reads the signal and renders HTML.
- **Inspector**: PixiJS click handler writes selected entity ID to a `createSignal<number | null>`.
  SolidJS `<Inspector>` component reads the signal, reads entity map entry by ID, renders HTML.
- **Score/playback**: SolidJS signals only, no PixiJS involvement.

**Anti-patterns to avoid:**
- Do not destructure props — access `props.tickData` inside tracking scopes
- Do not make entity map reactive (no createStore for 500+ entities updated every tick)
- Do not create effects that accidentally subscribe to unrelated signals
  (the V2 bug: terrain redraw effect tracking frameData)
- Use `batch()` when a WebSocket message updates multiple signals

## Security

No new attack surfaces. The renderer is a read-only consumer of server-pushed data.
No user input is sent to the server beyond existing WebSocket control messages
(pause/resume/reset). The inspector displays server-provided data — no user-editable
fields.

## Privacy

No PII handled. All data is game simulation state.

## Audit

No mutations to audit. The renderer is a pure display layer.

## Scope

### V3.0 (ship this)

- PixiJS v8 Application lifecycle (create on mount, destroy on cleanup)
- Camera: zoom (mouse wheel, 0.1x-5x), pan (pointer drag), cursor-centered zoom
- Hex grid rendering with biome colors and height shading
- Territory overlay (player-colored per owned hex)
- Infrastructure: roads, structures (farm/village/city/depot)
- Entity map with upsert logic (full snapshot mode, delta-ready)
- Interpolation: position lerp, facing slerp, 60fps render loop
- Entity rendering at continuous positions (no hex-center snapping)
- LOD tiers: close (individual entities), mid (stack badges), far (density heatmap)
- Close zoom: facing arrow, weapon/armor type labels, blood bar, wound count indicator
- Corpse persistence: fall animation, desaturated corpse with visible equipment
- Projectile rendering: separate ProjectileInfo array, lerped flight, velocity-oriented
- Hover tooltip: ID, owner, role, blood %, weapon type, wound count
- Click inspector: full EntityInfo detail, side panel, persistent until dismissed
- Playback controls: play/pause, speed presets, frame scrubber, back-to-live
- Layer toggles: territory, roads, settlements, convoys
- CSS flex layout, PixiJS resizeTo handles resize on inspector open/close
- Component decomposition: V3App, HexCanvas, PlaybackControls, Inspector, Tooltip, ScoreBar, LayerToggles
- Port V2 features: live WebSocket streaming, review/capture, flag system

### Deferred

- **Agent decision inspector** — show tactical/operations/strategy layer decisions per
  entity in the inspector panel. Requires protocol expansion (per-entity decision data).
  Deferred until agent decision wire format is designed.
- **Minimap** — inset showing full map with viewport rectangle. Map is small enough at
  30x30 that far zoom serves this purpose.
- **Chunk textures** — pre-rendered hex group textures for far zoom. Implementation
  detail for R4 wave; eng-lead can implement when draw call count exceeds budget.
- **Contour lines** — height contour lines at regular elevation intervals. Adds visual
  complexity for a feature that only matters at mid zoom on hilly terrain — not worth
  the rendering budget for V3.0.
- **Isometric toggle** — 3D elevation visualization. R5 wave.
- **Touch support** — pinch zoom, two-finger pan. Desktop-first for V3.0.
- **Sprite sheets** — switch from Graphics objects to texture atlases when entity count
  exceeds ~5k or draw calls exceed ~500.
- **Delta protocol mode** — entity map architecture supports it; renderer code unchanged.
  Activated when P spec implements the `full_state` flag.

## Verification

- [ ] PixiJS canvas renders on mount, destroys on unmount without leaks
- [ ] Zoom 0.1x to 5x works, centered on cursor
- [ ] Pan via pointer drag works, pointer capture prevents selection
- [ ] Hex grid renders with correct biome colors and height shading
- [ ] Entities render at continuous positions (not hex-center snapped)
- [ ] Interpolation produces smooth motion between ticks (no visual jitter)
- [ ] LOD close: individual entities visible with facing, equipment, blood, wounds
- [ ] LOD mid: stack badges with counts, grouped by hex
- [ ] LOD far: density heatmap, settlement dots
- [ ] Projectiles render with smooth flight, oriented along velocity
- [ ] Arc projectiles show parabolic trajectory
- [ ] Death: fall animation plays, corpse persists with equipment at close zoom
- [ ] Corpses desaturated at mid zoom, ground markers at far zoom
- [ ] Hover tooltip shows entity summary near cursor
- [ ] Click inspector opens side panel with full entity detail
- [ ] Inspector open/close resizes canvas correctly (CSS flex + resizeTo)
- [ ] Play/pause, speed presets, frame scrubber, back-to-live all functional
- [ ] Layer toggles show/hide territory, roads, settlements
- [ ] 500 entities at 60fps at mid zoom (V3.0 target)
- [ ] WebSocket reconnection recovers state via full snapshot
- [ ] Review/capture/flag system ported from V2

## Deploy Strategy

Frontend-only change. Build with `cd frontend && bun run build`. The built assets
are served by the existing Axum static file server. No backend deploy required for
renderer changes. Rollback: revert to previous frontend build.

## Convention Observations

- V2App.tsx uses vanilla-extract CSS. V3 components should follow the same pattern
  unless Tailwind is adopted for this project (currently not in deps).
- The existing `v2types.ts` defines wire types. V3 will need a `v3types.ts` with
  EntityInfo, ProjectileInfo, and related types. Keep V2 types for backward
  compatibility during transition.
- HexCanvas.tsx currently mixes rendering logic (hex drawing, entity drawing) with
  component lifecycle. V3 should extract rendering into pure functions that take
  the entity map and camera state, keeping HexCanvas.tsx focused on PixiJS lifecycle
  and the SolidJS bridge.

## Files Modified

### New files
- `frontend/src/V3App.tsx` — app shell, WebSocket, frame routing
- `frontend/src/v3/HexCanvas.tsx` — PixiJS lifecycle, render loop, entity map, camera
- `frontend/src/v3/PlaybackControls.tsx` — play/pause, speed, scrubber
- `frontend/src/v3/Inspector.tsx` — entity detail side panel
- `frontend/src/v3/Tooltip.tsx` — hover tooltip
- `frontend/src/v3/ScoreBar.tsx` — player scores
- `frontend/src/v3/LayerToggles.tsx` — layer visibility controls
- `frontend/src/v3/types.ts` — EntityInfo, ProjectileInfo, EntityState, ProjectileState
- `frontend/src/v3/entityMap.ts` — entity map class, upsert logic, interpolation helpers
- `frontend/src/v3/render/grid.ts` — hex grid drawing, height shading, territory overlay
- `frontend/src/v3/render/entities.ts` — entity rendering by LOD tier
- `frontend/src/v3/render/projectiles.ts` — projectile rendering
- `frontend/src/v3/render/camera.ts` — camera transforms, viewport culling
- `frontend/src/v3/render/corpses.ts` — corpse rendering, death animation

### Modified files
- `frontend/src/index.tsx` — add V3App route/entry point
- `frontend/src/styles/` — new component styles (vanilla-extract)

### Unchanged
- `frontend/src/V2App.tsx` — kept for V2 backward compatibility
- `frontend/src/HexCanvas.tsx` — kept for V2
- `frontend/src/v2types.ts` — kept for V2

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| R1 | 0 | — | PixiJS scaffold, camera, hex grid, height shading, SolidJS overlay shell, CSS flex layout |
| R2 | 1 | S1, P1, R1 | Entity map, upsert logic, interpolation, entity rendering at continuous positions, corpse lifecycle |
| R3 | 3 | R2, W1, D1 | Projectile rendering, close-zoom wound/equipment indicators, blood/stamina bars |
| R4 | 3 | R3 | LOD tiers (mid stack badges, far density heatmap), viewport culling |
| R5 | 4 | R4 | Chunk textures, isometric toggle prep, contour lines (deferred from V3.0) |

## Key Constraints

- 60fps at 500 visible entities (V3.0 mid zoom on 30x30 map with full battle)
- No PixiJS in the Rust engine. Renderer is purely frontend.
- SolidJS reactivity must not fight PixiJS's imperative render loop. Entity map is
  outside reactivity. Signals only for UI state.
- Entity map + upsert pattern must work for both full-snapshot and future delta mode
  without code changes.
- Corpses persist indefinitely — death is not removal from entity map.
- Projectiles are a separate wire array, not in EntityInfo.
