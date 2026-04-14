# R2 Implementation Plan — Entity Map, Interpolation, Entity/Corpse Rendering

Source spec: `docs/specs/v3-R-renderer.md`
Wave: 1 (depends on S1, P1, R1 — all landed)

## Scope

R2 delivers entity rendering at continuous world positions with smooth
interpolation, the entity map data structure with upsert logic, corpse
persistence with death animation, and a debug resync mechanism.

## Key Design Decisions

### Entity Map

Plain `Map<number, EntityState>` outside SolidJS reactivity. WebSocket writes,
PixiJS render loop reads at 60fps.

```typescript
interface EntityState {
  info: SpectatorEntityInfo;
  prevPos: { x: number; y: number; z: number };
  currPos: { x: number; y: number; z: number };
  prevFacing: number;
  currFacing: number;
  lastTickTime: number;
  state: 'alive' | 'dying' | 'corpse';
  deathTime?: number;
}
```

### Interpolation

Every render frame: `t = clamp((now - lastTickTime) / tickInterval, 0, 1)`.
- Position: `lerp(prevPos, currPos, t)`
- Facing: shortest-arc slerp
- Spawn: snap (prevPos = currPos)
- Death: animate fall over ~300ms, then freeze as corpse
- Corpse: skip interpolation, render at frozen position

### Death / Corpse Lifecycle

1. Entity has `blood <= 0` → transition to `'dying'`, record `deathTime`
2. After 300ms → transition to `'corpse'`
3. Entity appears in `entities_removed` AND was alive → treat as death (dying → corpse)
4. Corpses remain in entity map indefinitely, rendered desaturated with equipment

### Delta Protocol

V3App already handles `v3_snapshot` and `v3_snapshot_delta`. R2 improves this:
- Entity map upsert replaces naive frame array buffering for entity state
- Full snapshot (`full_state: true`) rebuilds entire entity map
- Delta applies appeared/updated/removed incrementally
- Frame buffer still exists for playback scrubbing (hex ownership, roads, players)

### Debug Resync

A "Resync" button that disconnects and reconnects the WebSocket. Server sends
full catchup (init + latest snapshot) on new connection. Zero backend changes.

## Files

### New files

| File | Responsibility |
|------|---------------|
| `frontend/src/v3/entityMap.ts` | EntityState type, EntityMap class with upsert, interpolation helpers, corpse lifecycle |
| `frontend/src/v3/render/entities.ts` | Entity rendering: individual entities at close zoom, stack badges at mid zoom, density at far zoom |
| `frontend/src/v3/render/corpses.ts` | Corpse rendering: desaturated, fallen, equipment visible at close zoom |

### Modified files

| File | Change |
|------|--------|
| `frontend/src/v3/HexCanvas.tsx` | Integrate entity map, add PixiJS ticker for 60fps render loop, render entities+corpses |
| `frontend/src/V3App.tsx` | Entity map upsert on tick, resync callback, pass tickInterval to HexCanvas |
| `frontend/src/v3/PlaybackControls.tsx` | Add resync button |

## Verification

- Entities render at continuous positions (not hex-center snapped)
- Smooth interpolation between ticks (no visual jitter)
- Facing arrows rotate smoothly
- Death triggers fall animation (~300ms), then corpse persists
- Corpses render desaturated with equipment visible at close zoom
- Resync button reconnects WS and gets fresh full snapshot
- 60fps render loop via PixiJS ticker
- Mid zoom: stack badges with count per hex
