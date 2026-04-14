# simulate_everything — Claude Code Context

## Critical: Server process management

See `.claude/CLAUDE.local.md` for systemd service management, deploy workflow, and local testing.

## Critical: Agent pools

Two separate pools in `crates/engine/src/agent.rs`:
- `all_builtin_agents()` — used by simulator + live. All agents including experimental.
- `rr_agents()` — used by round-robin. Curated subset only.

New agents must be added to the appropriate pool(s).

## Workspace layout

```
crates/protocol/ — shared wire types (V3 init/snapshot/delta) + msgpack encoding
crates/engine/   — game rules, state, agents, mapgen, replay, scoreboard (V1 + V2)
crates/web/      — Axum server: routes, WebSocket handlers, lobby, round-robin loop
crates/cli/      — CLI runner (sim + bench harness)
crates/replay/   — standalone replay exporter + static file server (no WS, no live state)
crates/viewer/   — wgpu WASM viewer (WebGPU terrain renderer, camera, Trunk build)
frontend/        — SolidJS + Vite frontend
docs/            — detailed documentation
```

## Key references

| Topic | Where to look |
|-------|---------------|
| Architecture, modes, API, protocol | `docs/architecture.md` |
| Pressure agent design | `docs/pressure-agent-handoff.md` |
| Agent trait + built-in agents | `crates/engine/src/agent.rs` |
| Expander agent impl | `crates/engine/src/expander_agent.rs` |
| Swarm agent impl | `crates/engine/src/swarm_agent.rs` |
| Pressure agent impl | `crates/engine/src/pressure_agent.rs` |
| V2 engine modules | `crates/engine/src/v2/` |
| V2 agent trait + spread agent | `crates/engine/src/v2/agent.rs` |
| V2 game rules + tick logic | `crates/engine/src/v2/sim.rs` |
| V2 combat + engagement | `crates/engine/src/v2/combat.rs` |
| V2 replay recording | `crates/engine/src/v2/replay.rs` |
| Game rules + step logic (V1) | `crates/engine/src/game.rs` |
| Map generation | `crates/engine/src/mapgen.rs` |
| RR loop + config | `crates/web/src/roundrobin.rs` |
| V2 RR loop | `crates/web/src/v2_roundrobin.rs` |
| Live lobby + game loop | `crates/web/src/lobby.rs` |
| All HTTP routes | `crates/web/src/main.rs` |
| WS protocol types | `crates/web/src/protocol.rs` |
| V2 WS protocol | `crates/web/src/v2_protocol.rs` |
| V3 engine modules | `crates/engine/src/v3/` |
| V3 entity model + state | `crates/engine/src/v3/state.rs` |
| V3 sim tick | `crates/engine/src/v3/sim.rs` |
| V3 agent architecture | `crates/engine/src/v3/agent.rs` |
| V3 wire types + msgpack | `crates/protocol/` |
| V3 WS builders + delta tracker | `crates/web/src/v3_protocol.rs` |
| V3 RR loop | `crates/web/src/v3_roundrobin.rs` |
| V3 review system | `crates/web/src/v3_review.rs` |
| V3 frontend types | `frontend/src/v3types.ts` |
| wgpu viewer crate | `crates/viewer/` |
| Viewer terrain renderer | `crates/viewer/src/heightmap.rs` |
| Viewer camera + input | `crates/viewer/src/camera.rs`, `src/input.rs` |
| Viewer GPU setup | `crates/viewer/src/gpu.rs` |
| Terrain shader (WGSL) | `crates/viewer/src/shaders/terrain.wgsl` |
| Replay exporter + viewer | `crates/replay/src/main.rs` |
| Systemd service template | `simulate_everything.service.example` |
| Python agent example | `examples/agent.py` |
| Stream E plan (agent behavior) | `docs/plans/v3-streamE-agent-behavior.md` |
| Stream F plan (compositional world) | `docs/plans/v3-streamF-compositional-world.md` |
| Neural evolution future plan | `docs/plans/future-neural-evolution.md` |
| V3 entity unification spec | `docs/specs/v3-entity-unification-2026-04-13.md` |
| Version roadmap | `docs/roadmap.md` |
| V3 sequencing graph | `docs/plans/v3-sequencing.md` |

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SIMEV_BIND_ADDR` | `0.0.0.0` | IP address to bind the server to |
| `SIMEV_PORT` | `3333` | Port to listen on |
| `SIMEV_STATIC_DIR` | `frontend/dist` | Path to built frontend assets |
| `SIMEV_VIEWER_DIR` | `crates/viewer/dist` | Path to built standalone viewer assets |
| `SIMEV_PLAYERS` | (varies) | Number of players |
| `SIMEV_TICK_MS` | (varies) | Tick interval in milliseconds |
| `SIMEV_SEED` | (random) | RNG seed |
| `SIMEV_V2_RR_REVIEW_DIR` | `var/v2_rr_reviews` | Directory for persisted flagged V2 RR review bundles |
| `SIMEV_V3_RR_REVIEW_DIR` | `var/v3_reviews` | Directory for persisted V3 review bundles |
| `RUST_LOG` | `info` | Tracing filter (e.g. `simulate_everything_engine::v2=debug`) |

## Frontend

Use `bun`, not `npm`, for all frontend JS/TS tooling:
```bash
cd frontend && bun install && bun run build
```

## Commits and docs hygiene

**Commit your work.** Before ending a session or after completing a logical unit of work, stage and commit. Use conventional-ish messages: `feat(engine): add pressure agent`, `fix(web): port conflict on restart`. Reference what changed, not what you did.

**Keep docs current.** If you change behavior covered by `docs/architecture.md` or this file — routes, agent pools, protocol messages, game rules, mapgen params, RR defaults, env vars — update the docs in the same commit as the code change. Stale docs are worse than no docs.

**What to update when:**
- Added/removed an agent or changed pools → update agent table in `docs/architecture.md` + "Agent pools" section above
- Changed API routes/params/protocol → update the relevant section in `docs/architecture.md`
- Changed game rules (growth, combat, fog) → update "Game Rules" in `docs/architecture.md`
- Changed mapgen defaults or algorithm → update "Map Generation" in `docs/architecture.md`
- Changed RR defaults (board size, player count, tick) → update `docs/architecture.md` RR section
- Changed systemd service or deploy process → update "Server process management" in `.claude/CLAUDE.local.md`
- Added new env vars → update env var table above and in `docs/architecture.md`
- Added new files/modules → update "Workspace layout" above and "Key references" table

## Testing agents

Use the simulator API — no restarts needed for the same binary:
```bash
curl -s "http://localhost:3333/api/ascii?seed=100&players=2&turns=500&at=50"
curl -s "http://localhost:3333/api/v2/ascii?seed=100&width=30&height=30&ticks=200"
```

## RR control

```bash
curl -s http://localhost:3333/api/rr/status
curl -s -X POST http://localhost:3333/api/rr/config -H 'Content-Type: application/json' -d '{"tick_ms":250}'
curl -s -X POST http://localhost:3333/api/rr/pause
curl -s -X POST http://localhost:3333/api/rr/resume
curl -s -X POST http://localhost:3333/api/rr/reset
```

## V2 RR control

```bash
curl -s http://localhost:3333/api/v2/rr/status
curl -s -X POST http://localhost:3333/api/v2/rr/config -H 'Content-Type: application/json' -d '{"tick_ms":100}'
curl -s -X POST http://localhost:3333/api/v2/rr/pause
curl -s -X POST http://localhost:3333/api/v2/rr/resume
curl -s -X POST http://localhost:3333/api/v2/rr/reset
curl -s -X POST http://localhost:3333/api/v2/rr/flags -H 'Content-Type: application/json' -d '{"game_number":1,"tick":123}'
curl -s http://localhost:3333/api/v2/rr/reviews
```

## V3 RR control

```bash
curl -s http://localhost:3333/api/v3/rr/status
curl -s -X POST http://localhost:3333/api/v3/rr/config -H 'Content-Type: application/json' -d '{"tick_ms":100,"mode":"tactical","autoplay":true}'
curl -s -X POST http://localhost:3333/api/v3/rr/pause
curl -s -X POST http://localhost:3333/api/v3/rr/resume
curl -s -X POST http://localhost:3333/api/v3/rr/reset
curl -s -X POST http://localhost:3333/api/v3/rr/flags -H 'Content-Type: application/json' -d '{"game_number":1,"tick":100,"annotation":"test"}'
curl -s -X POST http://localhost:3333/api/v3/rr/capture/start -H 'Content-Type: application/json' -d '{"game_number":1}'
curl -s -X POST http://localhost:3333/api/v3/rr/capture/stop -H 'Content-Type: application/json' -d '{"game_number":1}'
curl -s http://localhost:3333/api/v3/rr/reviews
```
