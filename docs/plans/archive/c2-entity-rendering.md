# C2: Entity Rendering — Implementation Plan

## Goal
Update PixiJS renderer (HexCanvas.tsx) to consume `SpectatorEntity[]` from unified entity
protocol instead of separate unit/convoy/settlement types.

## Entity Classification

From `SpectatorEntity` fields:
- **Structure**: `structure_type` is set (Farm/Village/City)
- **Resource**: `resource_type` is set (convoy carrying Food/Material/Settlers)
- **Combatant**: `health` is set, `role == "Soldier"`
- **Civilian**: none of the above (future-proofing, not in current data)

## Changes

### HexCanvas.tsx
1. Import `SpectatorEntity` type
2. Replace `CellStack` interface with entity-aware `HexStack`:
   - Groups entities by hex position
   - Tracks dominant type, total count, total health, owner
3. Rewrite `drawDynamic()`:
   - Build `hexStacks: Map<string, HexStack>` from `frameData.entities`
   - Fallback: if `entities[]` is empty, use old `units[]` path (replay compat)
   - Render per hex by dominant type:
     - Combatant hex: colored hex fill, brightness from health, red stroke if engaged
     - Structure hex: current settlement shapes (farm circle, village house, city tower)
     - Resource hex: small colored diamond
     - Civilian: small neutral circle
4. Stack count badge: when hex has >1 entity, draw "x{N}" label
   - Try PixiJS BitmapText; HTML overlay fallback if crashes
5. Facing indicator: at close zoom (scale > 2.0), draw directional arrow on combatant hexes
   - Arrow = line + small triangle in `facing` direction
6. Add hover/click detection:
   - Track pointer position in world coords
   - Convert to hex grid position (reverse of hexCenter)
   - Store hovered hex + entities in SolidJS signal
7. Add tooltip overlay:
   - Absolute-positioned HTML div over canvas
   - Shows entity details for hovered hex
   - Styled inline (no vanilla-extract needed)

### Props changes
- Add `onEntityHover?: (entities: SpectatorEntity[] | null) => void` callback prop (optional)
- Keep existing `RenderLayer` export unchanged

## Verification
```bash
cd frontend && bun run build
```
Then visual check at the V2 RR page:
- Entities render by type (combatant hexes, settlement shapes, convoy markers)
- Stacked hexes show count badge
- Hovering a hex shows entity tooltip
- Zoom levels affect what's shown (facing arrows at close zoom)
- Performance: still 60fps on 30x30 grid
