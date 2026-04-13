# Frontend Rendering Overhaul — SVG to PixiJS

> Plan written 2026-04-13. Supersedes ad-hoc HexBoard.tsx improvements for anything beyond quick fixes.

## Context

The current frontend renders the hex map as inline SVG via SolidJS (`HexBoard.tsx`). This works at current scale (~900 hexes, ~50 units) but has hard limits:

- SVG caps at ~5k elements at 60fps, ~1k for smooth interaction
- No zoom/pan — the entire map renders at a fixed size
- No viewport culling — all hexes rendered every frame
- No entity interpolation — movement snaps between ticks
- Settlements, roads, convoys are visual decorations, not interactive entities

The long-term target is **100k tiles, 10k units, zoom levels, and real entity systems**. This requires replacing the SVG renderer with a WebGL-based system.

## Target Architecture

```
┌─────────────────────────────────────────────┐
│  SolidJS App (V2SimApp / V2App)             │
│  ┌────────────────────────────────────────┐ │
│  │  PixiJS Canvas (WebGL)                 │ │
│  │  ┌──────────┐ ┌──────────┐ ┌────────┐ │ │
│  │  │ Hex Grid  │ │ Territory│ │Infra   │ │ │
│  │  │ Layer     │ │ Layer    │ │Layer   │ │ │
│  │  │ (chunks)  │ │ (texture)│ │(roads) │ │ │
│  │  └──────────┘ └──────────┘ └────────┘ │ │
│  │  ┌──────────┐ ┌──────────┐ ┌────────┐ │ │
│  │  │ Entity   │ │ Route    │ │ UI     │ │ │
│  │  │ Layer    │ │ Layer    │ │Overlay │ │ │
│  │  │ (sprites)│ │ (lines)  │ │(labels)│ │ │
│  │  └──────────┘ └──────────┘ └────────┘ │ │
│  └────────────────────────────────────────┘ │
│  ┌──────────┐ ┌──────────┐ ┌─────────────┐ │
│  │ Spatial  │ │ Camera   │ │ Entity      │ │
│  │ Index    │ │ System   │ │ Interpolator│ │
│  │(Flatbush)│ │(zoom/pan)│ │ (lerp)      │ │
│  └──────────┘ └──────────┘ └─────────────┘ │
│  ┌────────────────────────────────────────┐ │
│  │  SolidJS UI Panels (HTML overlay)      │ │
│  │  Score bar, legend, controls, tooltips │ │
│  └────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

Key principle: **PixiJS renders the map. SolidJS renders the UI.** The canvas sits inside a SolidJS component. UI panels are HTML overlaid on the canvas via CSS positioning.

## Rendering Layers (bottom to top)

1. **Hex Grid Layer** — Biome-colored hex sprites from texture atlas. Chunked: 16x16 hex chunks pre-rendered to RenderTexture at far zoom. Individual sprites at close zoom.
2. **Territory Layer** — Separate RenderTexture. Player-colored regions drawn with blend mode `multiply`. Recomputed only when territory changes (not every frame).
3. **Infrastructure Layer** — Roads as thick line segments with double-stroke technique. Depots as sprites.
4. **Route Layer** — Convoy routes as animated dashed lines (marching ants via shader or sprite strip). Color-coded by cargo type.
5. **Entity Layer** — Units, convoys, settlements as sprites from atlas. Managed by entity system. Position interpolated between server ticks.
6. **UI Overlay Layer** — Labels, count badges, health bars. PixiJS Text or BitmapText for performance.

## Zoom / LOD System

Three tiers with crossfade transitions (~200ms):

| Zoom | Tiles visible | Hex rendering | Entity rendering | Routes |
|------|--------------|---------------|------------------|--------|
| Close | <200 | Individual sprites, full biome detail | Individual sprites, status icons, strength numbers | Full lines with animation |
| Mid | 200-2000 | Individual sprites, simplified | Group badges ("x47"), settlement icons only | Route lines, no animation |
| Far | 2000+ | Chunk textures (16x16 pre-composited) | Heatmap overlay (density), settlement dots by type | Hidden |

LOD is purely a client rendering decision. The server sends the same entity data regardless of zoom.

**Camera**: Exponential zoom (1.15x per scroll step). Smooth pan via lerp (150-300ms). Clamp to map bounds with elastic overscroll. Pinch-to-zoom on touch.

## Spatial Indexing

- **Flatbush** (static R-tree) for the hex grid. Bulk-loaded once at map init. Used for "which hexes are in the viewport" queries.
- **RBush** (dynamic R-tree) for moving entities. Updated when entities move. Used for "which entities are visible" and "what did I click on" queries.
- Hex axial coordinates provide O(1) lookup for "what's at hex (q,r)" — the R-trees handle rectangular viewport range queries.

## Entity Interpolation

Server sends authoritative state every tick (100-250ms for strategy games). Client buffers two states:

1. `state[t-1]` — previous tick
2. `state[t]` — current tick

Each render frame, lerp entity positions: `pos = lerp(state[t-1].pos, state[t].pos, elapsed / tickInterval)`.

No client-side prediction needed — unit speeds and paths are known. If a correction arrives, ease toward corrected position over 100-200ms.

## Texture Atlas

Pre-render all visual states into a shared spritesheet:

- **Hex biomes**: 7 biomes x 4 height levels = 28 hex sprites
- **Settlements**: 3 types x 8 player colors = 24 sprites
- **Units**: 8 player colors x 3 states (normal, engaged, general) = 24 sprites
- **Infrastructure**: road segments (3 levels), depots, convoy diamonds
- **Status icons**: engaged, moving, cooldown, idle

Total: ~100 sprites. Single texture atlas, single draw call per layer via batching.

## Integration with SolidJS

```typescript
// HexCanvas.tsx — replaces HexBoard.tsx
const HexCanvas: Component<Props> = (props) => {
  let containerRef: HTMLDivElement;
  const app = new PIXI.Application();

  onMount(async () => {
    await app.init({ resizeTo: containerRef, antialias: true });
    containerRef.appendChild(app.canvas);
    // Initialize layers, spatial indices, camera
  });

  // React to frame data changes
  createEffect(() => {
    const frame = props.frame;
    if (frame) updateEntities(frame);
  });

  onCleanup(() => app.destroy());

  return <div ref={containerRef!} class={styles.boardCanvas} />;
};
```

UI panels (score bar, legend, controls) remain as SolidJS HTML components, positioned over the canvas with `position: absolute`.

## Phase Plan

### Phase 1: Quick SVG Fixes (now, 1-2 hours)
Keep the current SVG renderer functional while developing gameplay:
- Settlement icon sizes 2-3x (change 3 constants)
- Territory fill opacity 25% → 35%
- Unit brightness floor 0.3 → 0.5
- Score bar above the fold (layout change)

### Phase 2: Convoy Entities in Engine (Rust, independent of rendering)
Make convoys real entities with position, route, progress, state:
- `Convoy { id, owner, origin, destination, route: Vec<Axial>, progress: f32, speed, cargo, state }`
- Each tick advances progress along route
- Hex occupancy enables interception
- Update protocol to send convoy positions (not just "convoy exists at origin")
- This changes game mechanics — do it before the renderer rewrite

### Phase 3: PixiJS Renderer Prototype (the big one)
Replace HexBoard.tsx with HexCanvas.tsx:
- PixiJS v8 setup with WebGL
- Hex grid rendering from texture atlas
- Viewport culling via Flatbush
- Basic zoom/pan with camera system
- Territory overlay as RenderTexture
- Wire into existing SolidJS app (drop-in replacement)

### Phase 4: Entity Rendering + Interpolation
- Unit/convoy/settlement sprites from atlas
- RBush for entity spatial queries
- Position interpolation between server ticks
- LOD tier switching (close/mid/far)
- Animated convoy routes (marching ants shader or dash animation)

### Phase 5: Delta Sync Protocol
- Full snapshot on connect
- Per-tick deltas: `{added, updated, removed}`
- Viewport-filtered sync (only send visible entities)
- Dejitter buffer (render one tick behind)

### Phase 6: Chunk System for Far Zoom
- 16x16 hex chunks pre-rendered to RenderTexture
- Invalidate/re-render on content change
- Far zoom renders ~400 chunk sprites instead of 100k hex sprites
- Aggregate entity display (heatmaps, population density)

## Key Dependencies

- **PixiJS v8**: `bun add pixi.js` — main rendering library
- **Flatbush**: `bun add flatbush` — static spatial index for hex grid
- **RBush**: `bun add rbush` — dynamic spatial index for entities
- **@pixi/math-extras** or built-in: hex coordinate math (we already have this in Rust, mirror in TS)

## What This Replaces

| Current (HexBoard.tsx) | New (HexCanvas.tsx) |
|---|---|
| SVG `<polygon>` per hex | PixiJS Sprite per hex (or chunk texture) |
| SVG `<circle>/<path>` per settlement | PixiJS Sprite from atlas |
| SVG `<line>` for roads/routes | PixiJS Graphics or sprite-based lines |
| No zoom/pan | Camera with 3 LOD tiers |
| No culling (all hexes rendered) | Flatbush viewport query |
| Snap between ticks | Lerp interpolation at 60fps |
| ~1k element ceiling | ~100k element ceiling |

## Open Questions

1. **Hybrid period**: Do we run SVG and PixiJS renderers in parallel during transition, or hard-cut?
2. **Touch support**: Pinch-to-zoom and touch-pan for mobile/tablet viewing?
3. **Minimap**: Render a low-res version of the full map in a corner panel? (Standard for strategy games)
4. **Entity selection**: Click-to-select entities for inspector panel? (Future, but architecture should support it)
5. **WebGPU**: Worth adding as a fallback/upgrade path now, or wait until PixiJS adds native support?

## References

- See `docs/research/game-ui-design.md` for full research findings
- [PixiJS v8 docs](https://pixijs.com/8.x/guides)
- [Screeps renderer source](https://github.com/screeps/renderer) — reference PixiJS strategy game
- [Red Blob Games hex grids](https://www.redblobgames.com/grids/hexagons/)
- [Gabriel Gambetta entity interpolation](https://www.gabrielgambetta.com/entity-interpolation.html)
