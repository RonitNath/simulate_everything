# Game UI & Visual Design Research

> Research conducted 2026-04-13. Sources include GDC talks, game dev articles, library benchmarks, and analysis of Civ 6, Humankind, Polytopia, Screeps, Factorio, Supreme Commander, Victoria 3, Battle for Wesnoth.

## 1. Visual Hierarchy on Hex Maps

**Layered rendering** is universal across strategy games. Establish exactly 3-4 visual layers with clear z-ordering:

1. **Terrain** (lowest, most muted) — biome colors, height shading
2. **Territory overlay** (semi-transparent color wash over terrain)
3. **Infrastructure** (roads, routes — line-based, medium contrast)
4. **Entities** (units, settlements — highest contrast, most saturated)

**Civ 6** treats the hex grid as toggleable infrastructure, not decoration. "Strategic view" strips 3D to flat icons — players toggle based on what decision they're making. Resource/yield indicators are toggleable overlays.

**Humankind** is a cautionary tale: dashed borders for outposts vs solid for cities was invisible with dark faction colors. Border style differentiation only works with sufficient contrast at ALL zoom levels.

**Battle for Wesnoth** uses two-layer tiles: base terrain + overlay (buildings, bridges). Terrain transitions blend at hex edges rather than filling entire hexes — prevents jarring boundaries.

**Polytopia** proves minimal design maintains full readability. Bold flat colors, distinctive silhouettes, simplified geometry. Units/buildings readable at any zoom because they rely on **shape recognition, not texture detail**. Closest reference for a browser-based hex game.

## 2. Information Density

From Gamedeveloper.com strategy UI analysis and HUD design articles:

- **Unify information display**: Consolidate into 1-2 screen regions. Rise of Nations / AoE3 used a single bottom pane for contextual info.
- **Progressive disclosure**: Show summary by default, details on hover/click. AoE3 let players control how much was revealed.
- **Visual indicators for off-screen events**: Idle workers, completed buildings, incoming attacks need notification markers.
- **Replace raw numbers with visual metaphors**: Bars, color-coded indicators communicate faster than digits. Reserve exact numbers for tooltips.
- **One "hot-action color"**: Single accent color for highlights and critical actions. Desaturate everything else.
- **Group related elements**: Use proximity as a semantic signal. Cluster health/mana/stamina; space dissimilar info apart.

## 3. Color Theory for Territory

- **Player colors**: 6-8 hues spaced evenly on color wheel. Vary lightness AND saturation for colorblind accessibility.
- **Territory overlay**: faction color at 15-25% opacity over terrain. Increase saturation at borders, decrease toward centers.
- **Border rendering**: 2-3px solid colored borders at territory edges. Avoid dashed lines alone (Humankind's mistake). Border thickness should scale with zoom.
- **CSS blend modes**: `mix-blend-mode: multiply` for territory overlays on dark backgrounds darkens terrain naturally. `overlay` mode combines multiply/screen for more nuance.
- **Dark theme specifics**: Territory colors need higher saturation (60-80%) but lower lightness (40-55%) to remain visible without washing out terrain.

## 4. Icon Design at Small Scales

**Silhouette-first**: Icons must be recognizable by outline alone at 16-24px. Test by reducing to single-color silhouette.

**Settlement hierarchy** — use progressive visual weight:
- Farm: Small, single simple shape
- Village: Medium, 2-3 shapes clustered
- City: Large, complex silhouette with height (towers, walls)

**Practical techniques**:
- 2px bright outline (faction-colored or white) around dark icons for readability against any terrain
- Consistent "footprint" size within each category
- At very small scales (<16px), switch from detailed icons to simple geometric markers
- Units: distinguish by shape, not detail. Use faction color as fill, white/black outline for contrast.

## 5. Making the Map Feel Alive

Strategy games need subtle juice, not screen shake:

- **Animated supply lines**: SVG/Canvas `stroke-dashoffset` cycling via CSS animation ("marching ants")
- **Idle unit animation**: 1-2px vertical oscillation reads as "alive"
- **Territory pulse**: Flash new color at higher opacity on ownership change, settle to normal
- **Fog of war transitions**: Smooth 300-500ms opacity transitions, not instant binary
- **Water shimmer**: Slow color cycling or subtle opacity pulsing
- **Combat feedback**: Brief red flash on affected hexes, not whole-screen shake

## 6. Typography and UI Panels

**Typography rules**:
- Two fonts: one display, one body (sans-serif for game UIs)
- Minimum body: 14px desktop. Resource counters: 12px minimum.
- `letter-spacing: 0.02-0.05em` on small text
- Off-white text (#e0e0e0 to #f0f0f0) on dark, not pure #fff
- Tabular/monospace figures for resource numbers (prevents column shifting)

**Dark theme panels**:
- Background: dark grays (#1a1a2e, #16213e), not pure black
- Glass panels: `backdrop-filter: blur(8-12px)` with `background: rgba(17, 25, 40, 0.75)`
- Border: 1px `rgba(255, 255, 255, 0.08-0.15)`
- Box shadow: `0 4px 30px rgba(0, 0, 0, 0.3)`
- Panel opacity: 75-85% primary, 60-70% secondary/tooltip
- Corner-anchored panels (not edge-spanning) waste less screen real estate

## 7. Rendering Technology Benchmarks

| Technology | Sweet Spot | 60fps Ceiling |
|---|---|---|
| SVG | <5,000 elements | ~1,000 elements |
| Canvas 2D | 5,000-50,000 | ~10,000 |
| WebGL (PixiJS) | 50,000-500,000 | ~100,000 |
| WebGPU | 500,000+ | ~1,000,000+ |

For 100k tiles + 10k entities: **WebGL via PixiJS v8** is the right choice. WebGPU delivers 5-100x over WebGL in benchmarks but browser support still maturing.

**Key techniques**:
- Sprite batching: PixiJS batches sprites sharing a BaseTexture into single draw calls
- `ParticleContainer` renders 100k+ lightweight particles
- Texture atlases pack all hex states/entity sprites into shared sheets

**What real games use**:
- Screeps: PixiJS (WebGL)
- Territorial.io: Canvas2D (pixel-flood-fill, simpler workload)
- OpenBW/Titan Reactor (StarCraft in browser): Three.js + WebGL + WASM
- Generals.io: Canvas2D (small grids)

## 8. Spatial Indexing for Large Maps

All three libraries by Mourner (Mapbox/Leaflet author):

- **Flatbush**: Static R-tree. Significantly faster than RBush for bulk-loaded read-mostly data. Use for hex grid (positions don't change).
- **RBush**: Dynamic R-tree. Supports insert/delete/update. Use for moving entities. 2.2M weekly npm downloads.
- **KDBush**: Point-only index, 5-8x faster than RBush for static points.

Viewport query = `tree.search(viewportBBox)` returns entities in O(log n + k).

Hex grids are a natural spatial hash (axial coordinates → O(1) lookup), so R-trees are mainly for "everything in viewport rectangle" range queries on entities.

## 9. Zoom and Pan Systems

**Viewport culling** is mandatory — only render tiles intersecting camera rect. Eliminates 60-80% of GPU work.

**Chunk-based rendering**: Split map into NxN chunks (16x16 hexes). Pre-render each chunk to offscreen texture. At zoom-out, render chunk textures as single quads. At zoom-in, render individual tiles for visible chunks only.

**LOD tiers**:
- Zoom 1 (far): colored rectangles per chunk with aggregate data
- Zoom 2 (mid): hex outlines with simplified icons
- Zoom 3 (close): full detail with entities, routes, numbers

**Camera**: Lerp position over 200-600ms. Zoom uses exponential easing (each scroll step * 1.15x).

## 10. ECS in Browser Games

- **bitECS**: Data-oriented, typed arrays, 335k ops/sec. Cache-friendly. Low-level API.
- **Miniplex**: Object-based, plain JS objects, 109k ops/sec. More ergonomic, works naturally with PixiJS.
- **Becsy**: TypeScript-native, automatic system scheduling, multithreading support.

**Integration pattern**: ECS manages state (position, health, owner, movement). Rendering system queries ECS for visible entities and updates PixiJS sprites. Keep ECS and rendering loosely coupled.

## 11. Entity Patterns from Real Games

### Convoys

**Victoria 3**: Convoys are pooled resources with route-specific allocation. Routes are entities tracking endpoints, nodes traversed, cost, and effectiveness. Effectiveness degrades from attacks.

**Factorio**: Trains are full entities with position, cargo manifest, schedule, state machine (WAIT_AT_STATION / PATH_TO_STATION / MANUAL). Logistic Train Network mod adds dispatcher matching supply/demand across stops.

**Pattern**: `{id, origin, destination, route: Vec<Hex>, progress: f32, speed, cargo, state: Loading|Moving|Delivering|Intercepted}`. Route is pre-computed via pathfinding. Each tick advances progress. Hex occupancy check enables interception.

### Entity Lifecycle

**Bevy ECS**: Generational indices (Entity = index + generation). Components in archetype-based columnar storage.

**Pattern**: Flat `Vec<Option<EntityData>>` with generational IDs. State machine per entity type: `Created → Active(substates) → Dead`. Dead entities tombstoned, slots recycled. For replay, record `(tick, EntityEvent)` tuples — event-source the entity layer.

### Server-Client Sync

**Screeps protocol**: WebSocket per room. First message = full snapshot. Subsequent ticks = only changed properties, `null` for deleted. Property-level delta sync.

**Pattern**: Full snapshot on connect. Each tick: `{added: [...], updated: {id: {changed_fields}}, removed: [ids]}`. Filter by viewport/fog of war.

### Zoom-Dependent Entity Representation

**Supreme Commander**: Close = unit models, mid = NATO-style icons, far = colored dots. Continuous transition.

**Pattern** (3 LOD levels):
- Close (<50 tiles visible): Individual sprites, health bars, cargo indicators
- Mid (50-500 tiles): Group icons with count badge ("x47"), route lines
- Far (500+ tiles): Heatmap overlay, settlement dots sized by population, no individual units

LOD is purely client rendering — server sends same data regardless.

### Roads

**Red Blob Games**: Movement cost on edges between hexes, not on hexes themselves.

**Civ**: Roads are tile improvements (flag on cell).

**Factorio**: Belts are entities (state, throughput, item positions).

**Pattern for generals**: Hybrid — `HashMap<(Axial, Axial), RoadLevel>` with canonicalized edge keys. Road level modifies A* edge weights. Not full entities — they're infrastructure. Promote to entities if they gain durability/degradation.

## Sources

- [Gamedeveloper.com - UI Strategy Game Design Dos and Don'ts](https://www.gamedeveloper.com/design/ui-strategy-game-design-dos-and-don-ts)
- [7 Beginner HUD Mistakes](https://thewingless.com/index.php/2021/05/12/7-obvious-beginner-mistakes-in-your-video-games-hud-from-a-ui-ux-art-director/)
- [SVG vs Canvas vs WebGL 2025](https://www.svggenie.com/blog/svg-vs-canvas-vs-webgl-performance-2025)
- [Red Blob Games - Hexagonal Grids](https://www.redblobgames.com/grids/hexagons/)
- [Screeps Renderer (PixiJS)](https://github.com/screeps/renderer)
- [PixiJS v8 Performance Tips](https://pixijs.com/8.x/guides/concepts/performance-tips)
- [RBush R-tree](https://github.com/mourner/rbush)
- [bitECS](https://bitecs.dev/docs/introduction)
- [Gaffer on Games - State Synchronization](https://gafferongames.com/post/state_synchronization/)
- [Gabriel Gambetta - Entity Interpolation](https://www.gabrielgambetta.com/entity-interpolation.html)
- [Victoria 3 Dev Diary #39 - Shipping Lanes](https://www.paradoxinteractive.com/games/victoria-3/news/dev-diary-39-shipping-lanes)
- [Humankind Border Readability Feedback](https://community.amplitude-studios.com/amplitude-studios/humankind/forums/169-game-design/threads/41948)
- [Game Juice - GameAnalytics](https://www.gameanalytics.com/blog/squeezing-more-juice-out-of-your-game-design)
- [Dark Glassmorphism UI](https://medium.com/@developer_89726/dark-glassmorphism-the-aesthetic-that-will-define-ui-in-2026-93aa4153088f)
