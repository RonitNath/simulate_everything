# R3 Implementation Plan — Projectile Rendering + Close-Zoom Indicators

Source spec: `docs/specs/v3-R-renderer.md`
Wave: 3 (depends on R2, W1, D1 — all landed)

## Scope

R3 adds projectile rendering (velocity-oriented flight, interpolated positions)
and enhances close-zoom entity indicators (wound zone markers, stamina bar,
improved equipment display).

## Files

### New
- `frontend/src/v3/render/projectiles.ts` — projectile rendering: line segments
  oriented along velocity vector, LOD-aware (close: individual, mid: volley clusters)

### Modified
- `frontend/src/v3/HexCanvas.tsx` — add projectileGfx layer, render projectiles in tick
- `frontend/src/v3/render/entities.ts` — enhanced close-zoom: stamina bar, wound zone
  indicators with body zone labels, improved facing arrow

## Verification
- Projectiles render as velocity-oriented line segments
- Projectile positions interpolate smoothly between ticks
- Close zoom: wound zone markers visible on injured entities
- Close zoom: stamina bar alongside blood bar
- Mid zoom: projectile volley clusters (aggregate nearby projectiles)
