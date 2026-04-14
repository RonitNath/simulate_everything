# Plan: Agent Intelligence Pipeline

Created 2026-04-13. Research basis: `docs/research/agent-intelligence-systems.md`.

## Goal

Build a generic **perception -> memory -> decision -> trace** pipeline for V2 agents.
Make combat engagement a strategic choice (only when you'll win decisively).
Shift gameplay toward expansion, scouting, information exploitation, and decisive
concentrated strikes. Settlements grant vision.

## User stories

1. As a **spectator**, I can toggle per-player vision overlays to see what each
   player can and can't see, making fog-of-war strategy legible.
2. As a **developer**, I can see why each unit made its decision (move/engage/scout/hold)
   via structured traces in the CLI and spectator UI.
3. As an **agent author**, I receive influence maps, frontier data, and enemy memory
   as part of the observation — I don't have to build my own perception layer.
4. As a **player/spectator**, I see units hold position and concentrate before attacking,
   not trickle into 1:1 fights. Battles are rare but decisive.
5. As a **settlement builder**, my settlements provide vision even if I have no units
   nearby, making expansion valuable for information control.

## Architecture

### AgentMemory (per-player, engine-managed)

```rust
pub struct AgentMemory {
    // Influence maps (flat Vec<f32>, same size as grid)
    pub influence_friendly: Vec<f32>,
    pub influence_threat: Vec<f32>,

    // Enemy tracking
    pub enemy_sightings: Vec<EnemySighting>,  // ring buffer

    // Exploration state
    pub frontier: HashSet<usize>,      // cell indices on scouted/unscouted boundary
    pub scouted_pct: f32,
    pub last_seen: Vec<u64>,           // per-cell: tick when last visible

    // Aggregate stats
    pub visible_enemy_count: usize,
    pub estimated_total_enemy_strength: f32,
    pub nearest_threat_distance: i32,
}

pub struct EnemySighting {
    pub pos: Axial,
    pub tick: u64,
    pub strength: f32,
    pub unit_count: u16,
}
```

Lifecycle: engine computes this each poll interval, passes to agent alongside
ObservationDelta. Built-in agents read it; external agents can ignore it.

### DecisionTrace (per-unit, per-poll)

```rust
pub struct DecisionTrace {
    pub tick: u64,
    pub unit_id: u32,
    pub action: TracedAction,
    pub reason: &'static str,
    pub score: f32,
}

pub enum TracedAction {
    Move { q: i32, r: i32 },
    Engage { target_id: u32 },
    Disengage,
    Scout { frontier_cell: usize },
    Hold,
}
```

Ring buffer of last 1000 traces per player. Zero-alloc (static str reasons).
Agents return `Vec<(Directive, Option<DecisionTrace>)>`.

### Settlement Vision

In `vision::visible_cells()`, add settlement iteration:

| Settlement Tier | Vision Radius |
|----------------|--------------|
| Farm (pop 2-9) | 2 |
| Village (10-29) | 3 |
| City (30+) | 4 |

### Force Evaluation

Replace current `find_engageable_enemy` with Lanchester-informed threshold:

```rust
fn local_force_ratio(pos: Axial, obs: &Observation, memory: &AgentMemory) -> f32 {
    let friendly: f32 = obs.own_units.iter()
        .filter(|u| hex::distance(Axial::new(u.q, u.r), pos) <= 2)
        .map(|u| u.strength)
        .sum();
    let enemy_visible: f32 = obs.visible_enemies.iter()
        .filter(|e| hex::distance(Axial::new(e.q, e.r), pos) <= 2)
        .map(|e| e.strength)
        .sum();
    let enemy_fog: f32 = memory.influence_threat[cell_index(pos)];
    let total_enemy = enemy_visible.max(enemy_fog);
    friendly / total_enemy.max(1.0)
}
```

Engage only when ratio > 1.5. This means 1.5:1 strength ~ 2.25:1 Lanchester
combat power. Mutual attrition becomes rare.

## Implementation phases

### Phase 1: Settlement vision + force ratio (quick wins)

**Files**: `vision.rs`, `agent.rs`, `mod.rs`

1. Add settlement vision to `visible_cells()`. Iterate population groups, find
   settlement hexes (same as `is_settlement()`), add vision by tier.
   Add constants: `FARM_VISION=2`, `VILLAGE_VISION=3`, `CITY_VISION=4`.

2. Tighten `find_engageable_enemy()` threshold from current
   `friends >= 2 || strength >= 0.8x` to local force ratio >= 1.5 within radius 2.

3. Run `--diagnose` to verify spatial diversity improves (units hold instead of
   engaging) and production speed stays good.

4. Run benchmarks (100 seeds) to check game outcomes change — fewer timeout
   stalemates, more decisive victories.

### Phase 2: AgentMemory infrastructure

**Files**: new `memory.rs` in v2/, `observation.rs`, `runner.rs`

1. Create `AgentMemory` struct with influence maps, enemy sightings, frontier,
   last_seen, aggregate stats.

2. Influence map computation: in runner.rs before agent polling, compute
   `influence_friendly` and `influence_threat` per player.
   - Friendly: for each own unit, stamp `strength * 0.8^distance` within radius 4.
   - Threat: for visible enemies, stamp actual strength. For fog, decay from
     last sighting at `0.95^elapsed_ticks`.

3. Frontier maintenance: track in observation session. On newly_scouted events,
   incrementally update frontier HashSet.

4. Aggregate stats: compute `scouted_pct`, `visible_enemy_count`,
   `nearest_threat_distance` from observation data.

5. Pass `AgentMemory` to agents alongside `ObservationDelta`. Change agent trait:
   `fn act(&mut self, delta: &ObservationDelta, memory: &AgentMemory) -> Vec<Directive>`

### Phase 3: Decision traces

**Files**: new `trace.rs` in v2/, `agent.rs`, `runner.rs`, `gamelog.rs`

1. Define `DecisionTrace` and `TracedAction` types.

2. Change agent return type to `Vec<TracedDirective>` where:
   ```rust
   pub struct TracedDirective {
       pub directive: Directive,
       pub trace: Option<DecisionTrace>,
   }
   ```

3. Ring buffer in runner — store last 1000 traces per player.

4. Instrument all three agents (spread, striker, turtle) with trace reasons:
   "force_ratio_favorable", "scouting_frontier", "retreating_outnumbered",
   "holding_position", "chasing_weak_enemy", etc.

5. Expose traces in `--postmortem` output.

### Phase 4: Agent behavior using memory

**Files**: `agent.rs`

1. Agents use influence maps for movement: move toward areas where
   `influence_friendly > influence_threat` (exploit strength), avoid areas
   where threat dominates.

2. Scouting behavior: when no enemies visible, send scouts toward frontier
   cells with highest priority score (information gain + threat proximity).

3. Force concentration: when force ratio at a frontier is < 1.5, pull
   units from safe areas toward the frontier to build ratio before engaging.

4. Memory-informed retreat: flee toward nearest friendly influence peak
   (own settlement cluster), not just nearest settlement hex.

### Phase 5: Debug & UI visibility

**Files**: `ascii.rs`, `spectator.rs`, `v2_protocol.rs`, frontend

1. `render_vision(state, player_id)` — ASCII overlay showing `#` visible,
   `.` scouted, `?` fog. Settlement vision sources marked with `S`.

2. `render_influence(memory)` — ASCII threat/friendly map as 0-9 digits.

3. Spectator protocol: add `vision_overlays: Vec<Vec<bool>>` (per-player)
   and `traces: Vec<DecisionTrace>` to SpectatorSnapshot.

4. Frontend: toggle per-player vision. Show unit decision annotations
   on hover or as overlay text.

5. v2bench `--diagnose`: add Check 5 (scouted percentage at t=200)
   and Check 6 (decisive battle ratio — battles where one side had >1.5x ratio).

## Files modified

| File | Change |
|------|--------|
| `v2/vision.rs` | Settlement vision |
| `v2/mod.rs` | New constants (vision radii), `pub mod memory`, `pub mod trace` |
| `v2/agent.rs` | Force ratio threshold, memory-informed decisions, traces |
| `v2/memory.rs` | NEW — AgentMemory, influence maps, frontier, sightings |
| `v2/trace.rs` | NEW — DecisionTrace, TracedAction, ring buffer |
| `v2/observation.rs` | Pass memory alongside delta |
| `v2/runner.rs` | Compute memory before polling, store traces |
| `v2/ascii.rs` | Vision and influence renderers |
| `v2/spectator.rs` | Vision overlays and traces in snapshots |
| `v2/gamelog.rs` | Trace recording for postmortem |
| `web/v2_protocol.rs` | Vision overlay + traces in WS messages |
| `cli/v2bench.rs` | New diagnose checks |

## Verification

```bash
# Phase 1: settlement vision + force ratio
cargo test -p simulate-everything-engine
cargo run --release --bin simulate_everything_cli -- v2bench --diagnose \
  --agents spread,striker --seeds 0-9 --size 30x30

# Phase 2: memory infrastructure
cargo test -p simulate-everything-engine
# Verify influence maps are populated:
cargo run --release --bin simulate_everything_cli -- v2bench --postmortem \
  --agents spread,striker --seeds 42 --size 30x30

# Phase 4: behavioral improvement
cargo run --release --bin simulate_everything_cli -- v2bench --explain \
  --matchups "spread,striker;spread,turtle;striker,turtle" \
  --seeds 0-99 --size 30x30

# Phase 5: debug visibility
curl -s "http://localhost:3333/api/v2/ascii?seed=42&width=30&height=30&ticks=100&vision=0"
```
