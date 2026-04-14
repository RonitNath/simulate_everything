# V3 Track 5: Inspector & Tooltip Components

## Context

V3 renderer (R1-R4) is complete. Entities render with interpolation, projectiles
fly, close-zoom shows wound/equipment indicators. But there's no way to inspect
an individual entity's state or see entity details on hover.

## Goal

Click an entity in the renderer to see its details. Hover a hex to see entity info.

## Key files

- `frontend/src/V3App.tsx` — main V3 app, renders PixiJS + SolidJS overlay
- `frontend/src/v3types.ts` — TypeScript types matching V3 protocol
- `crates/web/src/v3_protocol.rs` — `SpectatorEntityInfo` (line 168) defines
  what data is available per entity: id, owner, x/y/z, hex, facing, role,
  blood, stamina, wounds, weapon_type, armor_zones, stack_id, attack_target, etc.

## What to build

1. **Entity inspector panel** — SolidJS component, shown when an entity is
   clicked in the PixiJS canvas. Displays:
   - Entity ID, owner, role
   - Position (x, y, z), hex (q, r)
   - Blood, stamina (progress bars)
   - Wound list (zone, severity, bleed rate)
   - Equipment (weapon type, armor zones)
   - Stack membership
   - Attack state (target, phase)
   Panel docked to right side, closes on click-away or Escape.

2. **Hex tooltip enhancement** — currently shows hex coords on hover. Add:
   - Entity count at hex
   - Entity names/roles on hover
   - Owner color coding

3. **Click detection** — PixiJS click event on entity sprites. The entity map
   (from R2) already tracks entity sprites by ID. Add hit testing: on canvas
   click, find nearest entity within 20px, dispatch to inspector.

## Data flow

V3App already stores entity state from snapshots/deltas. The inspector reads
from this store. No new API calls or protocol changes needed.

## Dependencies

None. Pure frontend, reads existing protocol data.

## Verify

Open V3 in browser, start a round-robin game (`curl -X POST localhost:3333/api/v3/rr/resume`),
click an entity, see its stats. Hover a hex, see entity summary.

Use `bun` for frontend tooling:
```bash
cd frontend && bun install && bun run build
```

## Commit

`feat(frontend): entity inspector panel + hex tooltip enhancement`
