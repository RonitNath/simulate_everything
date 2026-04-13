# HexBoard.tsx Rendering Analysis

> Analyzed 2026-04-13. Documents the current SVG rendering implementation for reference during the PixiJS migration.

## Files

- **Component**: `frontend/src/HexBoard.tsx`
- **Styles**: `frontend/src/styles/app.css.ts`, `board.css.ts`, `theme.css.ts` (vanilla-extract)

## Hex Geometry

- **Orientation**: Pointy-top (vertices at 0, 60, 120, 180, 240, 300 degrees)
- **Coordinate system**: Offset-even (even-r)
- **Hex size calc**: `Math.max(4, Math.min((vw - 320) * 0.88 / (SQRT3 * (w + 0.5)), (vh - 140) * 0.88 / (1.5 * h + 0.5)))`
- **Hex center**: `x = SQRT3 * size * (col + 0.5 * (row & 1))`, `y = 1.5 * size * row`
- **Polygon scale**: `size * 0.96` (96% of hex size, creates gap between hexes)
- **Stroke**: `#1a1a2e`, width `Math.max(0.5, size * 0.04)`

## Biome Colors

```
desert:    [180, 160, 90]
steppe:    [140, 150, 80]
grassland: [80, 140, 60]
forest:    [40, 100, 45]
jungle:    [20, 80, 35]
tundra:    [130, 155, 170]
mountain:  [100, 95, 95]
```

Height modifier: `t = 0.6 + 0.4 * height`. River: blend 80% base + 20% `[60, 120, 200]`.

## Player Colors

```
[0]: #4a9eff  [1]: #ff4a6a  [2]: #4aff8a  [3]: #ffa04a
[4]: #c04aff  [5]: #4affd0  [6]: #ff4aff  [7]: #d0ff4a
```

Strength brightness: `0.3 + 0.7 * log(1 + strength) / log(1 + maxStrength)`, range 0.3-1.0.

## Settlement Rendering

| Type | Element | Size | Fill | Stroke |
|------|---------|------|------|--------|
| Farm | `<circle>` | r=`s*0.12` | `playerRgba(owner, 0.8)` | none |
| Village | `<path>` pentagon | `s*0.25` wide | `playerRgba(owner, 0.9)` | white, 0.5px |
| City | `<path>` crenellated tower | `s*0.35` wide | `playerRgba(owner, 0.95)` | white, 1px |

## Road Rendering

Connected segments from hex center to midpoint of neighbor hex centers.

Neighbor arrays:
- Even rows: `[[-1,-1], [-1,0], [0,1], [1,0], [1,-1], [0,-1]]`
- Odd rows: `[[-1,0], [-1,1], [0,1], [1,1], [1,0], [0,-1]]`

| Level | Color | Width |
|-------|-------|-------|
| 3+ | `rgba(240,220,160,0.8)` | `max(1.5, level * 0.8 + size * 0.04)` |
| 2 | `rgba(220,200,140,0.7)` | same formula |
| 1 | `rgba(200,200,180,0.6)` | same formula |

Stroke linecap: `round`.

## Convoy Rendering

- **Shape**: Diamond, `ds = size * 0.3`
- **Fill**: `playerRgba(owner, 0.85)`, stroke white 0.5px
- **Label**: F/M/S character if `size > 12`, font `max(6, size * 0.22)`, bold white
- **Route lines**: origin→destination, stroke 0.8px, opacity 30%
  - Food: dash `3,3`, Material: dash `6,4`, Settlers: dash `2,6`

## Unit Rendering

- **Normal**: hex fill `playerRgbDim(owner, brightness)`
- **General**: white stroke `#ffffff`, width `max(2, size * 0.1)`. Crown icon: 3-point, scale `size * 0.3`, white fill, black stroke
- **Engaged**: red-orange stroke `#ff6644`, width `max(1, size * 0.06)`
- **Status icons**: `⚔` red-orange, `→` green `#88cc88`, `◷` gray `#aaa`, `·` dark gray `#666`
- **Count numbers**: font `max(7, size * 0.35)`, white, bold, shown only when `showNumbers` toggled on

## Territory Overlay

- Scale: `size * 0.88`
- Fill: `playerRgba(owner, 0.25)`
- Suppressed on hexes with settlements

## Ghost Units (Dead)

- Opacity: `max(0, 1 - age / 8)` — fades over 8 ticks
- Scale: `size * 0.82`
- Fill: `playerRgba(owner, 0.45)`, stroke white 70% opacity
- Dash: `3,3`
- Death marker: `x` at `size * 0.46`

## SVG Render Order (bottom to top)

1. Ghost units
2. Base hex polygons (biome fill)
3. Territory overlays
4. Road segments
5. Depots (`<rect>` gold `#c0a000`)
6. Settlements
7. Unit count numbers
8. General crowns
9. Status icons
10. Engagement edge lines (`#ff6644`, width `max(2, size * 0.1)`)
11. Convoy route lines
12. Convoy diamonds
13. Unit destination lines (dashed, 40% opacity)

## Layer Toggles

Types: `territory | roads | depots | settlements | convoys | destinations`

Default enabled: all except `destinations`.

## Theme Colors (vanilla-extract)

```
bg: #0a0a0f
surface: #141420
surfaceHover: #1a1a2e
border: #2a2a3e
text: #e0e0e8
textMuted: #8888a0
```
