# C1: PixiJS Scaffold — Implementation Plan

## Goal
Replace SVG HexBoard with PixiJS WebGL HexCanvas for V2 hex board rendering.

## Files to create/modify

1. **`frontend/package.json`** — add `pixi.js` v8 dependency
2. **`frontend/src/HexCanvas.tsx`** — new PixiJS renderer component
3. **`frontend/src/V2App.tsx`** — swap HexBoard import for HexCanvas

## HexCanvas.tsx Design

### Architecture
- PixiJS `Application` mounted to a `<canvas>` element via `onMount`
- All hex graphics in a `Container` (the "world") that transforms for zoom/pan
- SolidJS HTML overlay for score bar / game info (not in PixiJS)
- `onCleanup` destroys the PixiJS app

### Camera
- **Zoom**: mouse wheel → scale the world container. Clamp 0.2x–5x.
  Zoom toward cursor position (adjust container pivot).
- **Pan**: mousedown+drag → translate the world container position.
- Store camera state in local variables (not signals — PixiJS manages its own render).

### Hex Geometry (match HexBoard.tsx exactly)
- **Orientation**: Pointy-top (vertices at 60*i - 30 degrees)
- **Coordinate system**: Offset even-r (row, col)
- **Center**: `x = SQRT3 * size * (col + 0.5 * (row & 1))`, `y = 1.5 * size * row`
- **Size**: fixed at 20px (PixiJS handles zoom via container scale, unlike SVG which computed size from viewport)

### Rendering Layers (bottom to top)
1. **Terrain layer** — one `Graphics` per hex with biome fill color
2. **Territory overlay** — semi-transparent player-colored hex polygons
3. **Road segments** — line graphics between hex centers
4. **Settlements** — simple shapes (circle for farm, pentagon for village, tower for city)
5. **Units** — colored circles with count text labels
6. **Engagement indicators** — red edge lines on engaged hexes

### What to skip (per spec)
- No entity interpolation
- No LOD tiers or viewport culling
- No texture atlas — Graphics objects only
- No convoys, destinations, ghost units (can add later)
- No hover interaction (add later)

### Props (same interface as HexBoard)
```typescript
interface HexCanvasProps {
  staticData: BoardStaticData;
  frameData: BoardFrameData;
  numPlayers: number;
  showNumbers?: boolean;
  layers?: Set<RenderLayer>;
}
```

### Reactive Updates
- `createEffect` watches `frameData` changes → clear and redraw dynamic layers (territory, units, roads, settlements)
- `createEffect` watches `staticData` changes → redraw terrain layer
- Terrain is static per game — only redrawn on game reset

## V2App.tsx Changes
- Replace `import HexBoard from "./HexBoard"` with `import HexCanvas from "./HexCanvas"`
- Replace `<HexBoard .../>` with `<HexCanvas .../>` (drop hover props for now)

## Verification
```bash
cd frontend && bun install && bun run build
```
Then visual check at the V2 RR page in browser
