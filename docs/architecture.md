# simulate_everything — Architecture

## Modes

### Simulator
Pregenerated replays. Runs a full game synchronously and returns JSON.

- `GET /` — HTML page
- `GET /api/game?seed=N&players=N&turns=N&width=N&height=N` — Run game, return `Replay` JSON (seed random if omitted)
- `GET /api/ascii?seed=N&players=N&turns=N&at=N&width=N&height=N` — ASCII board at specific turn (final if `at` omitted)

Uses `all_builtin_agents()` shuffled by seed. Frontend: `App.tsx`.

### Live PVP
WebSocket-based multiplayer. External agents connect, spectators watch.

- `GET /live` — HTML page
- `ws /ws/agent` — Agent connection (send `Join`, receive `Observation`, send `Actions`)
- `ws /ws/spectate` — Spectator stream
- `POST /api/live/config` — `{"tick_ms": N, "show_numbers": bool}`

Lobby waits for N players (env `GENERALS_PLAYERS`, default 2). Auto-rematches. Frontend: `LiveApp.tsx` with `__WS_PATH__="/ws/spectate"`.

### Round-Robin
Continuous automated tournament. Built-in agents play 1v1 on 23x23 maps.

- `GET /rr` — HTML page
- `ws /ws/rr` — Spectator stream
- `POST /api/rr/config` — `{"tick_ms": N, "show_numbers": bool}`
- `POST /api/rr/pause` / `POST /api/rr/resume` / `POST /api/rr/reset`
- `GET /api/rr/status` — Health metrics (avg/max compute, overrun %, headroom %)

Uses `rr_agents()` (separate pool from simulator). Records to in-memory scoreboard. Frontend: `LiveApp.tsx` with `__WS_PATH__="/ws/rr"`.

### Scoreboard
- `GET /scoreboard` — HTML page
- `GET /api/scoreboard` — JSON `{total_games, agents: [{id, wins, losses, draws, ...}]}`

Frontend: `ScoreboardApp.tsx`, polls every 3s.

## WebSocket Protocol

**Spectator -> Server:** `{"type": "set_speed", "tick_ms": N}`

**Server -> Spectator:** `game_start` (width, height, num_players, agent_names), `frame` (turn, grid, stats, compute_us), `game_end` (winner, turns), `config` (show_numbers, tick_ms)

**Agent -> Server (Live):** `{"type": "join", "name": "..."}`, `{"type": "actions", "actions": [...]}`

**Server -> Agent (Live):** `lobby`, `game_start`, `observation` (fog-of-war applied), `game_end`, `error`

## Engine

### Game Rules
- 2-8 players, fog of war (1-tile radius from owned cells)
- Capture enemy general = eliminate player, inherit territory
- Structures (generals, cities): +1 army/turn. Cities +2 extra on wave turns (total +3), generals +1 extra (total +2). Empty owned land: +1 on wave turns (every 25 turns)
- Combat: attacker sends army-1 (or army/2 if split). Attacker > defender = capture with remainder
- Mountains: impassable. Cities: neutral garrison, capturable, then +1/turn
- Actions: unlimited orders per turn, executed interleaved round-robin across players

### Agents
Implement `trait Agent: Send` with `act(&mut self, obs: &Observation, rng: &mut dyn RngCore) -> Vec<Action>`.

| Agent | ID | Strategy |
|-------|----|----------|
| ExpanderAgent | expander-v2 | Economy-first with phase transitions (Expand→Pressure→Strike), city-obsessed, FOW memory, 25% frontier attack axis |
| SwarmAgent | swarm-v3 | Marching-column agent: expand early, then biggest stack marches toward enemy while frontier keeps expanding, FOW memory |
| PressureAgent | pressure-v3 | Role-based single-objective focus, FOW memory, marauder interception |
| SubprocessAgent | graph-search-v1 | Bridges to Python process via stdin/stdout (env `GENERALS_PYTHON_CLIENT`) |

Two pools: `all_builtin_agents()` (simulator, includes all + Python) and `rr_agents()` (round-robin, curated subset).

Lookup by name: `agent_by_name("pressure")` returns a boxed agent. `builtin_agent_names()` lists all known names. Used by the bench harness.

### Map Generation
`MapConfig::for_size(w, h, players)` derives all params from dimensions:
- Mountains: 20% density, placed in clusters (random walk ridges + scattered singles)
- Cities: ~3% of cells, garrison scales with distance from generals (closer = cheaper)
- Generals: margin from edges (~15% of smaller dim), min Manhattan distance (~40% of smaller dim)
- BFS connectivity verified; retries if generals disconnected

### ASCII Renderer
Cell encoding: `....` empty, `####` mountain, `c 42` neutral city, `a  5` player a, `A 38` general, `a~12` owned city.

`screenshot(width, height, grid, turn, stats) -> String` or `frame.ascii(w, h)` for Display.

## CLI (`simulate_everything_cli` binary)

### Simulation mode (default)
```bash
simulate_everything_cli [seed] [players] [max_turns] [--ascii]
```
Runs a single game with shuffled agents. Outputs event JSON to stdout (or ASCII board with `--ascii`).

### Bench mode
```bash
simulate_everything_cli bench [flags]
```
Parallel game runner for agent comparison. All games run on rayon thread pool. Outputs per-game JSON to stdout, summary to stderr.

**Flags:**
| Flag | Default | Description |
|------|---------|-------------|
| `--agents a,b` | `pressure,swarm` | Single matchup (legacy) |
| `--matchups all` | — | Round-robin all non-random agent pairs |
| `--matchups a,b;c,d` | — | Explicit matchup list |
| `--seeds 100-249` | `100-249` | Seed range or comma-separated list |
| `--turns N` | 500 | Max turns per game |
| `--size WxH` | auto | Board dimensions |
| `--top N` | 10 | Number of interesting games to display |
| `--profile` | — | Per-turn JSON output for single seed |
| `--converge` | — | Keep running batches until CI converges |
| `--ci F` | 0.04 | Target CI width for convergence (e.g., 0.03 = 3%) |
| `--max-seeds N` | 5000 | Upper bound on seeds in convergence mode |
| `--batch N` | 100 | Seeds per batch in convergence mode |

**Convergence mode** (`--converge`): runs batches of games, computes Wilson score 95% CI after each batch, stops when CI width < target. Ctrl+C gracefully stops after the current batch and prints partial results. Second Ctrl+C force-quits.

**Game scoring**: each game gets an interestingness score (0-100+) based on late lead changes, comebacks, closeness at 75% mark, game length, and upsets. Top N games displayed in the summary.

**Examples:**
```bash
# Quick regression check
simulate_everything_cli bench --seeds 100-249 --agents pressure,swarm
# Full round-robin with convergence
simulate_everything_cli bench --converge --matchups all --ci 0.03
# Profile a specific interesting game
simulate_everything_cli bench --profile --seeds 202 --agents pressure,swarm
# Tight CI for detecting small improvements
simulate_everything_cli bench --converge --agents pressure,swarm --ci 0.02 --max-seeds 10000
```

## Replay Binary (`simulate_everything_replay`)

Separate binary for generating and serving publishable game replays. No WebSocket handlers, no live game state, no agent subprocess spawning — minimal attack surface.

### Export mode (default)
```bash
simulate_everything_replay --seeds 100-110 --agents pressure,swarm --out ./replays
```
Generates self-contained HTML files with embedded vanilla JS viewer (no build toolchain needed). Each file includes the full replay data and a complete playback UI matching the main frontend's look and feel.

**Flags:**
| Flag | Default | Description |
|------|---------|-------------|
| `--seeds` | `42` | Seed range or comma-separated list |
| `--agents a,b` | `pressure,swarm` | Agent names |
| `--turns N` | 500 | Max turns per game |
| `--size WxH` | auto | Board dimensions |
| `--out DIR` | `./replays` | Output directory |
| `--format json\|html` | `html` | Output format |
| `--title TEXT` | `simulate_everything Replay` | Title for HTML pages |

Multiple seeds produce an `index.html` linking to each replay.

### Serve mode
```bash
simulate_everything_replay serve --dir ./replays --port 8080
```
Minimal static file server (Axum + tower-http ServeDir). No dynamic routes. Requires `serve` feature (enabled by default).

### Integration with bench harness
The bench harness identifies interesting games by score. Export those specific seeds:
```bash
# Bench finds seed 202 is interesting (upset, comeback)
simulate_everything_cli bench --seeds 100-249 --agents pressure,swarm --top 5
# Export that replay for publishing
simulate_everything_replay --seeds 202 --agents pressure,swarm --out ./replays
```

## Frontend

SolidJS + Vite + vanilla-extract CSS. Built to `frontend/dist/` by systemd on deploy.

| File | Mode | Notes |
|------|------|-------|
| `App.tsx` | Simulator | Fetches replay JSON, playback controls |
| `LiveApp.tsx` | Live + RR | WebSocket spectator, shared via `__WS_PATH__` / `__PAGE__` globals |
| `ScoreboardApp.tsx` | Scoreboard | Polls JSON every 3s |
| `Board.tsx` | All | Grid renderer, army brightness, player colors |
| `Nav.tsx` | All | Mode navigation links |

## Environment Variables
| Var | Default | Used by |
|-----|---------|---------|
| `GENERALS_PLAYERS` | 2 | Live lobby size |
| `GENERALS_TICK_MS` | 250 | Live tick speed |
| `GENERALS_SEED` | 42 | Live first game seed |
| `GENERALS_STATIC_DIR` | — | Path to `frontend/dist/` |
| `GENERALS_PYTHON_CLIENT` | — | Path to Python agent dir |
| `RUST_LOG` | — | Tracing level |

---

## V2 Engine

V2 is a ground-up redesign of the game engine. Full design spec: `docs/v2-engine-spec.md`. Centurion agent architecture spec: `docs/v2-agent-spec.md`. Unit behavior overhaul research and planning notes: `docs/v2-unit-behavior.md`.

**Key differences from V1:**

| Dimension | V1 | V2 |
|-----------|----|----|
| Grid | Square tiles | Hex grid (axial coordinates) |
| Time | Discrete turns | Continuous ticks (10 ticks/second target) |
| Units | Army values per cell | Entity units with individual IDs and strength |
| Combat | Instant capture on move | Edge-based engagement: units lock and drain each other over ticks |
| Terrain | Mountains / cities / empty | Hex terrain carries productivity, stockpiles, roads, height, moisture, rivers, and biome/region data |
| Map size | Variable | 30×30 default for RR |

**Constants (from `crates/engine/src/v2/mod.rs`):**

| Constant | Value | Meaning |
|----------|-------|---------|
| `FOOD_RATE` | 0.1 / tick | Food income per stationary, unengaged unit |
| `MATERIAL_RATE` | 0.05 / tick | Material income per stationary, unengaged unit |
| `FARMER_RATE` / `WORKER_RATE` | 0.04 / 0.03 | Population cohort production rates |
| `UNIT_FOOD_COST` | 8.0 | Food cost to produce one unit |
| `UNIT_MATERIAL_COST` | 5.0 | Material cost to produce one unit |
| `UPKEEP_PER_UNIT` | 0.02 / tick | Food upkeep per unit |
| `STARVATION_DAMAGE` | 0.5 | Strength lost per tick when upkeep cannot be paid |
| `SETTLEMENT_THRESHOLD` | 10 | Population required for a hex to count as a settlement |
| `SETTLER_CONVOY_SIZE` | 10 | Population loaded into one settler convoy |
| `SETTLEMENT_SUPPORT_RADIUS` | 1 hex | Radius that auto-accrues production into a settlement stockpile |
| `FRONTIER_DECAY_RATE` | 0.02 / tick | Unsupported frontier stockpile spoilage rate |
| `TIMEOUT_TICKS` | 3000 | Hard timeout before winner-by-score is used |
| `SOLDIERS_PER_UNIT` | 5 | Trained population consumed per produced unit |
| `INITIAL_STRENGTH` | 100.0 | Starting strength for a new unit |
| `DAMAGE_RATE` | 0.05 | Fraction of strength dealt per tick in combat |
| `DISENGAGE_PENALTY` | 0.3 | Strength loss on breaking engagement |
| `BASE_MOVE_COOLDOWN` | 1 tick | Minimum ticks between moves |
| `TERRAIN_MOVE_PENALTY` | 0.5 | Additional cooldown scaling per terrain point |
| `VISION_RADIUS` | 5 hexes | Base fog-of-war visibility range before height bonus |
| `INITIAL_UNITS` | 5 | Units each player starts with |
| `TICKS_PER_SECOND` | 10 | Simulation rate |
| `AGENT_POLL_INTERVAL` | 5 ticks | How often agents are queried for directives |

**V2 engine modules (`crates/engine/src/v2/`):**

| Module | Role |
|--------|------|
| `state` | `GameState`, `Unit`, `Player`, `Population`, `Convoy`, terrain/region data |
| `sim` | Tick loop: stockpile accrual, population growth/training, upkeep/starvation, convoy/unit movement, combat, win condition |
| `combat` | Edge-based engagement: lock, damage, disengage |
| `hex` | Axial coordinate math, neighbor enumeration, distance |
| `mapgen` | Terrain pipeline: productivity, height, moisture, rivers, biomes, regions, initial populations |
| `observation` | Fog-of-war filtered observation including population, stockpiles, roads, convoys |
| `directive` | Directives for movement, combat, production, role assignment, convoys, depots, and roads |
| `agent` | `Agent` trait + `SpreadAgent` |
| `pathfinding` | Hex A* for movement |
| `vision` | Visibility computation |
| `ascii` | ASCII renderer for debugging |
| `replay` | Fidelity-preserving replay snapshots for units, stockpiles, population, convoys, and final-state reconstruction |
| `runner` | Synchronous game runner (used by `/api/v2/game`) |

**V2 engine internals:**

- Units, population, and convoys are stored in `SlotMap`s keyed by opaque engine IDs (`UnitKey`, `PopKey`, `ConvoyKey`) so hot paths can do direct lookups instead of scanning vectors.
- `GameState` maintains a rebuilt-per-tick `SpatialIndex` cache for unit occupancy lookups on a hex and its adjacent ring.
- Unit movement resolves simultaneously per tick: one-step intents are gathered first, same-target conflicts are resolved deterministically, and only winning moves are committed.
- Normal military movement no longer produces same-hex stacking. Enemy swap attempts, enemy same-target contests, and blocked advances into occupied enemy hexes create limited auto-engagement instead.
- A light movement-facing zone-of-control rule prevents a unit that is already enemy-adjacent from sliding deeper through another enemy-adjacent hex unless it is actually increasing distance from its current threats.
- Replay and spectator payloads keep stable numeric `id` fields for frontend compatibility even though the engine uses slotmap keys internally.
- Agent polling now uses `InitialObservation` plus per-poll `ObservationDelta`; the engine keeps a lightweight dirty-cell mask and each caller owns an `ObservationSession` to track prior visibility/scouted state.
- `observe()` still exists as a convenience for tests, replay reconstruction, and benchmarking; it materializes a full observation from the same delta-oriented internals.

**V2 economy and settlement rules:**

- Population exists per hex, but only hexes with at least `SETTLEMENT_THRESHOLD` population count as settlements.
- Only settlements grow population, can build depots/roads, and act as stockpile anchors.
- Production inside `SETTLEMENT_SUPPORT_RADIUS` of a settlement auto-accrues into that settlement's stockpile.
- Production outside settlement support remains on the producing hex until convoyed away.
- Convoys are raided if an enemy unit occupies the convoy hex or any adjacent hex after convoy movement; captured cargo is transferred into the raider's hex stockpile.
- Unsupported frontier stockpiles decay by `FRONTIER_DECAY_RATE` each tick.
- Natural migration is slow adjacent drift from established settlements into owned fertile frontier hexes.
- Deliberate long-range expansion uses settler convoys carrying `SETTLER_CONVOY_SIZE` population.
- V2 games end either by elimination or by `TIMEOUT_TICKS`, where the timeout winner is chosen by weighted score: population 40%, territory 30%, military strength 20%, stockpiles 10%.
- In debug builds, each tick runs a conservation-oriented economy check against tracked production/consumption/destruction totals, then also asserts finite/non-negative assets and player totals that match owned stockpiles.

**V2 fog-of-war observation model:**

- `visible` is the live per-tick vision mask used for units, population, convoys, stockpiles, and ownership.
- `scouted` is persistent terrain memory per player. Once a hex has been seen, static fields remain known even after it leaves vision.
- `terrain`, `material_map`, `road_levels`, and `height_map` are masked by `scouted`.
- Starting areas are pre-scouted from each player's initial unit vision.

### V2 Web Routes

| Route | Method | Description |
|-------|--------|-------------|
| `GET /v2` | HTTP | V2 simulator HTML page |
| `GET /v2/rr` | HTTP | V2 round-robin spectator HTML page |
| `GET /api/v2/game` | HTTP | Run a V2 game synchronously, return replay JSON |
| `GET /api/v2/ascii` | HTTP | Run a V2 game, return ASCII board at a given tick |
| `GET /ws/v2/rr` | WebSocket | V2 RR spectator stream |
| `POST /api/v2/rr/config` | HTTP | Set tick speed (`{"tick_ms": N}`) |
| `POST /api/v2/rr/pause` | HTTP | Pause V2 RR loop |
| `POST /api/v2/rr/resume` | HTTP | Resume V2 RR loop |
| `POST /api/v2/rr/reset` | HTTP | Reset current game and start a new one |
| `GET /api/v2/rr/status` | HTTP | Health metrics |

### V2 WebSocket Protocol

All V2 WebSocket messages are JSON with a `type` discriminant. Defined in `crates/web/src/v2_protocol.rs`.

**Server → Spectator:**

| `type` | Fields | Description |
|--------|--------|-------------|
| `v2_init` | `width`, `height`, `terrain: Vec<f32>`, `material_map: Vec<f32>`, `height_map: Vec<f32>`, `region_ids: Vec<u16>`, `player_count`, `agent_names` | Sent once per game. Carries the static terrain layers needed to render the board and seed reconnect catchup. |
| `v2_snapshot` | `tick`, `full_state`, `units`, `engagements`, `convoys`, `hex_changes`, `settlements`, `players` | Sent every tick. `full_state=false` frames carry only dynamic hex deltas plus full entity lists; reconnect catchup uses a `full_state=true` snapshot representing the current board. |
| `v2_game_end` | `winner: Option<u8>`, `tick: u64`, `timed_out: bool` | Sent when the game ends. `winner` is the elimination winner or the score winner at timeout; `timed_out` distinguishes those cases. |
| `v2_config` | `tick_ms?: u64` | Sent when tick speed changes. |

`v2_snapshot.players` uses compact HUD buckets instead of exact economy floats: `population`, `territory`, `food_level`, and `material_level`.

Late-joining spectators receive a catchup burst of the last `v2_init` plus a `full_state=true` `v2_snapshot` before being subscribed to live deltas.

### V2 Round-Robin

Implemented in `crates/web/src/v2_roundrobin.rs` (`V2RoundRobin`). Runs continuously in a background Tokio task.

- 2-player games on 30×30 hex maps.
- Seeds increment from 1000 each game.
- Max 3000 ticks per game before forced `game_end` and score-based winner selection.
- Agents are polled every `AGENT_POLL_INTERVAL` (5) ticks.
- Currently runs **SpreadAgent vs SpreadAgent** — both players use the same placeholder agent.
- Supports pause / resume / reset without process restart.
- Spectators receive a broadcast from a `tokio::broadcast::Sender<V2ServerToSpectator>` (capacity 512).
- The RR loop maintains a full current snapshot for reconnect catchup even though live traffic is delta-based.

### V2 Agents

**Trait** (`crates/engine/src/v2/agent.rs`):

```rust
pub trait Agent: Send {
    fn name(&self) -> &str;
    fn init(&mut self, obs: &InitialObservation);
    fn act(&mut self, delta: &ObservationDelta) -> Vec<Directive>;
    fn reset(&mut self) {}
}
```

Agents own their local cached view; `SpreadAgent` materializes a full observation internally and patches it with each delta before making decisions. Directives are accumulated and applied to the game state by `directive::apply_directives`. The `reset` hook is called between games.

**SpreadAgent** — current placeholder:

- Balances general-hex population between farmers, workers, and soldier training.
- Builds a depot and basic roads around the general, launches settler convoys once the core is large enough, and opportunistically launches resource convoys from owned stockpile hexes.
- Produces units from trained soldiers when the general hex has enough local food/material stockpile.
- Moves non-general units with the original spread/lanes heuristic and engages opportunistically.

SpreadAgent is a structural placeholder. The target architecture is **Centurion** (see `docs/v2-agent-spec.md`): a hierarchy of specialized sub-agents (economic, tactical, strategic) with shared state and coordinated directives.
