# Pressure Agent Handoff

## Status
**Competitive.** 92% win rate (86W/7L) across 93 games against expander and swarm on 23x23 maps. v3 uses role-based order emission with single-objective focus.

- vs expander: 90% (37W/4L)
- vs swarm: 94% (49W/3L)

## What exists
- `crates/engine/src/pressure_agent.rs` (~420 lines) — `PressureAgent` implementing `Agent` trait
- Registered in both `all_builtin_agents()` and `rr_agents()` in `agent.rs`
- Module declared in `lib.rs`

## Architecture (v3)

### World model (unchanged from v1)
- **Per-player threat model** (`PlayerModel`): tracks each opponent's total army/land (from `opponent_stats`), visible army/land, border cells, threat ratio. Rebuilt every turn.
- **Fog memory**: remembers last-seen owner/army/turn per cell. Persists across turns.
- **Enemy general tracking**: records known general positions, estimates unknown ones from centroid of visible enemy cells.

### Order emission (v3 — single-objective focus)
Three-phase system:

1. **Frontier cells** score each non-owned neighbor and expand/attack outward.
   - Priorities: generals > cities (5000) > enemy (2000) > neutral (800) > fog (700)
   - Center bias before enemy contact (pushes toward map center where enemy likely is)
   - **If no good outward move**: frontier cell consolidates its army inward toward the objective (doesn't sit idle)

2. **Interior cells** (army >= 2) consolidate toward a single objective:
   - **Defense mode**: toward our general when heavy enemy within 5 BFS tiles
   - **Intercept mode**: toward marauders (heavy enemy stacks deep in our territory, within 6 BFS tiles)
   - **Attack mode**: toward the top 25% of frontier cells nearest the attack target
   - **Fallback**: toward nearest frontier

3. **Marauder detection**: enemy cells with army >= 8 and >= 2 owned neighbors are classified as marauders. Nearby owned cells (within 6 BFS owned-territory steps) route to intercept instead of the main objective.

### Key design decisions
- **Army threshold = 2**: every cell with army > 1 contributes. Army=2 cells consolidate — the 1 left behind still holds territory.
- **No splitting**: all moves send full army for maximum concentration.
- **25% frontier focus**: only the quarter of frontier cells closest to the attack target receive consolidation army. This creates a focused attack column instead of spreading across the whole border.
- **Idle frontiers consolidate**: frontier cells that can't capture anything move army toward the objective rather than hoarding it.

## What failed and why (historical)

### v1: Pressure field (0% win rate)
Single gradient field = single direction per cell. All armies funneled toward the enemy on contact, starving expansion elsewhere. See git history for details.

### v2: Basic role-based (56% win rate)
Fixed the pressure-field problem with frontier/interior roles. Problems:
- army < 3 threshold left tons of idle army=2 cells
- Consolidation went to ALL enemy frontier cells equally — no focus
- No marauder response
- Frontier cells sat idle when they couldn't capture

## Remaining failure modes (7/93 losses)

### Rush vulnerability (3 swarm losses)
Pressure leads early but general falls around T90-135. Defense mode triggers too late — 5 BFS tiles + army >= 5 isn't enough warning for a fast swarm rush. Could improve by:
- Lowering defense threshold (3 BFS, any enemy army)
- Proactively keeping a garrison on/near general

### Long-game economy collapse (4 expander losses)
Pressure leads at midpoint then loses ground in 250+ turn games. The 25% frontier focus means most frontiers get no army, so expansion stalls while expander grows. Could improve by:
- Adaptive focus: widen the attack cone when ahead on army (e.g., 50% of frontier)
- Periodic "expansion turns" where consolidation targets all frontiers
- Economy-aware mode: if army ratio is favorable, keep expanding

## Files to read
- `crates/engine/src/pressure_agent.rs` — the agent
- `crates/engine/src/agent.rs` — `Agent` trait, `Observation` struct, expander/swarm for comparison
- `crates/engine/src/game.rs` — game rules, `grow_armies()` for current growth model
- `crates/engine/src/action.rs` — `Action`, `Direction` types
- `docs/architecture.md` — routes, runtime configuration, simulator API, and WebSocket protocol
