# Handoff: Frontend Design Research — April 13, 2026

## What was done

### Visual audit
Took Playwright screenshots of V2 simulator at ticks 10, 50, 100, 500, and end-state for both 2-player and 4-player games (seed 42, 30x30). Screenshots saved in `frontend/` as `tick-*.png` and `4p-tick-*.png`.

### Key findings from screenshots
1. Settlement icons (Farm/Village/City) nearly invisible — Farm is 1.7px at typical hex size
2. Territory fill too subtle (25% opacity) to read ownership boundaries
3. Convoy routes (0.8px, 30% opacity) functionally invisible
4. Weak units (brightness floor 0.3) blend into terrain
5. Score bar below the fold — primary game feedback hidden
6. Map feels static between tick 100 and tick 1000

### Research conducted
Comprehensive research on strategy game UI design, large-scale browser rendering, and entity architecture patterns. Full findings in:

- `docs/research/game-ui-design.md` — visual hierarchy, color theory, icon design, typography, animation, rendering benchmarks, spatial indexing, ECS patterns, entity sync, zoom/LOD
- `docs/research/hexboard-rendering-analysis.md` — complete analysis of current HexBoard.tsx: every size constant, color value, render order, layer system

### Plans written
- `docs/plans/svg-quick-fixes.md` — 5 immediate changes to improve playability (settlement sizes 2-3x, territory opacity, brightness floor, score bar position, default unit counts)
- `docs/plans/frontend-rendering-overhaul.md` — full plan to replace SVG renderer with PixiJS v8 for 100k tiles / 10k units. Covers: architecture, zoom/LOD tiers, spatial indexing, entity interpolation, chunk system, delta sync protocol, 6-phase implementation plan.

### Key architectural decision
**Don't polish the SVG renderer.** It caps at ~5k elements. The target (100k tiles, 10k units, zoom/pan) requires WebGL via PixiJS. Make the SVG functional enough to develop gameplay (Phase 1 quick fixes), then replace it entirely (Phases 3-6).

### Settlement hierarchy engine work (from earlier in session)
The settlement hierarchy (Farm/Village/City), city AI, and agent simplification were implemented in a prior segment of this conversation. Engine chunks 1-3 from the plan at `~/.claude/plans/wobbly-noodling-matsumoto.md` are complete and merged to main. The frontend rendering for settlements exists but is too small to see (hence the quick fixes plan).

## What's next
1. **Phase 1**: SVG quick fixes (1-2 hours) — `docs/plans/svg-quick-fixes.md`
2. **Phase 2**: Convoy entities in Rust engine (independent of rendering)
3. **Phase 3+**: PixiJS renderer — `docs/plans/frontend-rendering-overhaul.md`

## Open question
Whether to also implement the Phase 2 convoy-as-entity engine changes before the PixiJS rewrite, or do both in parallel. Convoy entities change game mechanics and the sync protocol, so they're worth doing first.
