# generals — Claude Code Context

## Critical: Server process management

The web server binds to `localhost:3333` (hardcoded). Only one instance can run.

- **Never start `cargo run --bin simulate_everything` while systemd is running.** Check with `systemctl is-active simulate_everything`.
- **Never `pkill simulate_everything`** — use `systemctl stop`. pkill triggers restart loops.
- **To deploy changes:** `sudo systemctl restart simulate_everything.service` (rebuilds via ExecStartPre).
- **Port conflicts on restart:** The old process may hold the port. Kill it first: `sudo fuser -k 3333/tcp; sleep 1; sudo systemctl restart simulate_everything`.

## Critical: Agent pools

Two separate pools in `crates/engine/src/agent.rs`:
- `all_builtin_agents()` — used by simulator + live. All agents including experimental.
- `rr_agents()` — used by round-robin. Curated subset only.

New agents must be added to the appropriate pool(s).

## Workspace layout

```
crates/engine/   — game rules, state, agents, mapgen, replay, scoreboard
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
| Pressure agent impl | `crates/engine/src/pressure_agent.rs` |
| Game rules + step logic | `crates/engine/src/game.rs` |
| Map generation | `crates/engine/src/mapgen.rs` |
| RR loop + config | `crates/web/src/roundrobin.rs` |
| Live lobby + game loop | `crates/web/src/lobby.rs` |
| All HTTP routes | `crates/web/src/main.rs` |
| WS protocol types | `crates/web/src/protocol.rs` |
| Replay exporter + viewer | `crates/replay/src/main.rs` |
| Systemd service | `simulate_everything.service` |
| Python agent example | `examples/agent.py` |

## Commits and docs hygiene

**Commit your work.** Before ending a session or after completing a logical unit of work, stage and commit. Use conventional-ish messages: `feat(engine): add pressure agent`, `fix(web): port conflict on restart`. Reference what changed, not what you did.

**Keep docs current.** If you change behavior covered by `docs/architecture.md` or this file — routes, agent pools, protocol messages, game rules, mapgen params, RR defaults, env vars — update the docs in the same commit as the code change. Stale docs are worse than no docs.

**What to update when:**
- Added/removed an agent or changed pools → update agent table in `docs/architecture.md` + "Agent pools" section above
- Changed API routes/params/protocol → update the relevant section in `docs/architecture.md`
- Changed game rules (growth, combat, fog) → update "Game Rules" in `docs/architecture.md`
- Changed mapgen defaults or algorithm → update "Map Generation" in `docs/architecture.md`
- Changed RR defaults (board size, player count, tick) → update `docs/architecture.md` RR section
- Changed systemd service or deploy process → update "Server process management" above
- Added new env vars → update env var table in `docs/architecture.md`
- Added new files/modules → update "Workspace layout" above and "Key references" table

## Testing agents

Use the simulator API — no restarts needed for the same binary:
```bash
curl -s "http://localhost:3333/api/ascii?seed=100&players=2&turns=500&at=50"
```

## RR control

```bash
curl -s http://localhost:3333/api/rr/status
curl -s -X POST http://localhost:3333/api/rr/config -H 'Content-Type: application/json' -d '{"tick_ms":250}'
curl -s -X POST http://localhost:3333/api/rr/pause
curl -s -X POST http://localhost:3333/api/rr/resume
curl -s -X POST http://localhost:3333/api/rr/reset
```
