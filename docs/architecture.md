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

## CLI (`generals` binary)

### Simulation mode (default)
```bash
generals [seed] [players] [max_turns] [--ascii]
```
Runs a single game with shuffled agents. Outputs event JSON to stdout (or ASCII board with `--ascii`).

### Bench mode
```bash
generals bench [flags]
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
generals bench --seeds 100-249 --agents pressure,swarm
# Full round-robin with convergence
generals bench --converge --matchups all --ci 0.03
# Profile a specific interesting game
generals bench --profile --seeds 202 --agents pressure,swarm
# Tight CI for detecting small improvements
generals bench --converge --agents pressure,swarm --ci 0.02 --max-seeds 10000
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
generals bench --seeds 100-249 --agents pressure,swarm --top 5
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
