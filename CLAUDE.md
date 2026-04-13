# generals — Claude Code Context

## Critical: Server process management

See `.claude/CLAUDE.local.md` for systemd service management, deploy workflow, and local testing.

## Critical: Agent pools

Two separate pools in `crates/engine/src/agent.rs`:
- `all_builtin_agents()` — used by simulator + live. All agents including experimental.
- `rr_agents()` — used by round-robin. Curated subset only.

New agents must be added to the appropriate pool(s).

## Workspace layout

```
crates/engine/   — game rules, state, agents, mapgen, replay, scoreboard (V1 + V2)
crates/web/      — Axum server: routes, WebSocket handlers, lobby, round-robin loop
crates/cli/      — CLI runner (sim + bench harness)
crates/replay/   — standalone replay exporter + static file server (no WS, no live state)
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
| Replay exporter + viewer | `crates/replay/src/main.rs` |
| Systemd service template | `simulate_everything.service.example` |
| Python agent example | `examples/agent.py` |

## Environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `GENERALS_BIND_ADDR` | `0.0.0.0` | IP address to bind the server to |
| `GENERALS_PORT` | `3333` | Port to listen on |
| `GENERALS_STATIC_DIR` | `frontend/dist` | Path to built frontend assets |
| `GENERALS_PLAYERS` | (varies) | Number of players |
| `GENERALS_TICK_MS` | (varies) | Tick interval in milliseconds |
| `GENERALS_SEED` | (random) | RNG seed |
| `RUST_LOG` | `info` | Tracing filter (e.g. `simulate_everything_engine::v2=debug`) |

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
```
