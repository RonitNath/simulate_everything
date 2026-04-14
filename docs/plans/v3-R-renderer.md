# V3 Domain: R — Renderer

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Frontend section)
Sequencing: `docs/plans/v3-sequencing.md`

## Purpose

Replace the SVG renderer with PixiJS WebGL. Render entities at their continuous
positions — no hex-center snapping. Support zoom/pan, LOD tiers, height
visualization, projectile rendering, and wound/equipment indicators at close zoom.

## Design Questions

### R.1 PixiJS Setup

- PixiJS v8. Application lifecycle: create on mount, destroy on unmount. How does
  the PixiJS Application integrate with SolidJS reactivity? The canvas is a raw
  DOM element managed by PixiJS; SolidJS manages HTML overlay panels.
- Resize handling: PixiJS renderer must resize when the browser window resizes.
  ResizeObserver on the container div → renderer.resize(). Debounce?
- Render loop: PixiJS's built-in ticker or manual requestAnimationFrame? The
  ticker is simpler. 60fps target.
- Asset loading: for V3.0, use PixiJS Graphics objects (procedural shapes), not
  sprite textures. A hex is a drawn polygon. An entity is a drawn circle. This
  avoids texture atlas complexity initially. When does it become worth switching
  to sprites? Probably when entity count exceeds ~5k.

### R.2 Camera

- Zoom: mouse wheel. Smooth zoom (lerp toward target zoom level) or instant?
  Smooth feels better. Zoom range: 0.1x (strategic overview) to 5x (individual
  entities). Zoom center: under the mouse cursor (standard map behavior).
- Pan: click-drag (middle mouse or left mouse on empty space). Touch support
  (pinch zoom, two-finger pan) needed? Not for V3.0 (desktop first).
- Camera state: { x, y, zoom }. The camera transforms world coordinates to screen
  coordinates. All rendering uses this transform.
- Minimap: a small inset showing the full map with the viewport rectangle. V3.0
  or deferred? Probably deferred — the map is small enough at 30×30.

### R.3 Hex Grid Rendering

- Each hex is a flat-top hexagon. At close/mid zoom, draw individual hex outlines
  with biome-based fill colors. At far zoom, render chunk textures (pre-rendered
  groups of hexes as a single texture).
- Height visualization: darken/lighten hex fill based on terrain height. Contour
  lines at regular height intervals (every 10m?). Contour lines only at mid zoom
  (too noisy at far, unnecessary at close)?
- Territory: player-colored semi-transparent overlay per hex. Only for owned hexes.
  Recomputed when territory changes.
- Infrastructure: roads as line segments between hex centers. Walls as thicker
  lines on hex edges. Both drawn on a separate PixiJS Container layer.

### R.4 Entity Rendering

- Entities are at continuous world positions. Convert to screen position via camera
  transform. No hex-center snapping — a soldier between two hexes renders between
  them.
- Close zoom (hex > 80px): individual entities visible. Person = small circle in
  player color. Equipment shown as icons (sword, bow, shield tiny sprites or
  unicode). Facing arrow. Wound indicator (red tint, progressively darker with
  blood loss).
- Mid zoom (hex 20-80px): stack badges. A hex with 15 friendly soldiers shows a
  player-colored circle with "15" text. Structure icons (farm, village, city,
  depot). Dominant equipment type shown as a small icon on the stack badge.
- Far zoom (hex 5-20px): density heatmap. Player-colored intensity proportional
  to entity count. Settlement dots. Road network lines.
- Projectile rendering: arrows in flight as small line segments oriented along
  their velocity vector. At close zoom, individual arrows. At mid/far zoom,
  volley clusters (aggregate).

### R.5 SolidJS Overlay

- PixiJS renders the map canvas. SolidJS renders UI panels as HTML elements
  positioned via CSS over the canvas. This separates game rendering from UI
  rendering — PixiJS doesn't handle text well, SolidJS does.
- Panels: score bar (top), speed controls (bottom), hex/entity inspector (side
  panel on hover), minimap (corner, future).
- Inspector: on hover/click, show entity details (wounds, equipment, blood,
  stamina) or hex details (terrain, stockpiles, structures). Reads from the
  current frame data.
- The existing V2App.tsx controls (play/pause/step, speed slider, server
  pause, restart, flag/capture) should port to V3 with minimal changes.

### R.6 Performance

- Viewport culling: only render entities and hexes visible in the current viewport.
  For hexes: compute the hex range visible given camera position and zoom, iterate
  only those. For entities: Flatbush (static R-tree rebuilt per frame from hex
  grid bounds) or simpler — just iterate entities in visible hexes.
- Batching: PixiJS batches draw calls for same-texture sprites. With Graphics
  objects (V3.0), batching is less efficient. Monitor draw call count. Switch to
  sprite sheets if draw calls exceed ~500.
- Entity count target: 10k entities at 60fps. At close zoom, only ~100-200 are
  visible. At far zoom, they're aggregated to heatmap (no individual rendering).
  The bottleneck is mid zoom with ~1000 visible entities.

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| R1 | 0 | — | PixiJS scaffold, camera, hex grid rendering, SolidJS overlay |
| R2 | 1 | S1, R1 | Entity rendering at continuous positions, height shading |
| R3 | 3 | R2, P1 | Projectile rendering, wound/equipment indicators at close zoom |
| R4 | 3 | R3 | LOD tiers, viewport culling, chunk textures at far zoom |
| R5 | 4 | R4 | Height contour lines, isometric toggle prep |

## Key Files (Expected)

- `frontend/src/HexCanvas.tsx` — PixiJS Application, render loop, camera
- `frontend/src/renderer/grid.ts` — hex grid layer, territory overlay, height shading
- `frontend/src/renderer/entities.ts` — entity sprites/graphics, LOD switching
- `frontend/src/renderer/projectiles.ts` — arrow/stone rendering
- `frontend/src/renderer/camera.ts` — zoom, pan, world↔screen transforms
- `frontend/src/V3App.tsx` — SolidJS app shell, controls, inspector

## Constraints

- 60fps at 1000 visible entities (mid zoom on 30×30 map with full battle).
- No PixiJS in the Rust engine. The renderer is purely frontend. It reads
  whatever the protocol sends.
- SolidJS reactivity must not fight PixiJS's imperative render loop. The bridge:
  SolidJS signals drive PixiJS state updates in the render tick, not vice versa.
