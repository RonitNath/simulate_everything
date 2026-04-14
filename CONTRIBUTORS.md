# Contributors

This repo is operated as a small, high-autonomy simulation workspace. Optimize for accurate changes, clean operational surfaces, and minimal surprise in the live server paths.

## Working Rules

- Own the full loop: inspect, implement, verify, report.
- Preserve gameplay terminology where it is mechanical, but keep repo and product naming aligned with `simulate_everything`.
- Avoid broad refactors unless they pay for themselves in operability or clarity.
- Treat the web binary and service example as production-facing surfaces.
- Do not revert unrelated local changes you did not make.
- If you rewrite history, snapshot the working tree first and call out the new force-push requirement.

## Repo Map

```text
README.md                          Top-level usage and local runbook
CONTRIBUTORS.md                    Contributor instructions and shorthand
simulate_everything.service.example Example systemd unit for the web server

crates/engine/                     Core rules, agents, map generation, replay model, v2 engine
crates/cli/                        Batch simulation and benchmark harness
crates/web/                        Axum server, WebSocket protocols, live/rr/v2 routes
crates/replay/                     Replay export pipeline and static replay server

frontend/src/                      SolidJS apps for sim, live, rr, scoreboard, v2
frontend/src/styles/               vanilla-extract theme and board styling

python-client/                     External reference client / subprocess agent
docs/                              Architecture notes and design specs
scripts/                           Small repo maintenance helpers
```

## Architecture Shorthand

- `engine`: source of truth for rules, state transitions, observations, and built-in agents
- `cli`: offline runner; use it for reproducible sims, benchmarks, CI-style convergence runs
- `web`: operator surface; simulator, live play, RR, scoreboard, and v2 endpoints
- `replay`: publication surface; emits portable standalone replay artifacts
- `frontend`: visualization layer only; gameplay authority stays in Rust
- `python-client`: experimental / bridge surface; not the canonical engine
- `v2`: newer hex-based simulation track under active iteration

## Common Entry Points

- `cargo run -p simulate-everything-web`
- `cargo run -p simulate-everything-cli -- bench --agents pressure,swarm`
- `cargo run -p simulate-everything-replay -- --seeds 42`
- `cd frontend && bun run build`

## Naming Rules

- External product name: `simulate_everything`
- Web binary: `simulate_everything`
- CLI binary: `simulate_everything_cli`
- Replay binary: `simulate_everything_replay`
- Keep gameplay terms like `general`, `city`, `fog`, `army`, and `round-robin` when they describe mechanics

## Verification Checklist

- Rust crates compile after package or import renames
- Frontend metadata and docs match the current binary names
- systemd example points at the current release artifact
- README and architecture docs stay consistent with the actual CLI surface
- History rewrites are followed by an explicit note that `git push --force-with-lease` is required
