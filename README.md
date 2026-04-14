# simulate_everything

`simulate_everything` is a Rust workspace for running, benchmarking, and visualizing territory-control strategy simulations.

It includes:

- A core engine for map generation, fog-of-war gameplay, agents, replays, and scoreboards
- A CLI for one-off simulations and agent benchmarks
- A web server for replay viewing, live games, round-robin tournaments, and scoreboard pages
- A replay exporter that emits self-contained HTML replays
- A Python client for subprocess and live-agent experimentation
- An in-progress `v2` simulation stack built around hex maps and higher-scale experiments

## Workspace

```text
crates/engine   Core game logic, built-in agents, mapgen, replay types, v2 engine
crates/cli      `simulate_everything_cli` binary for single runs and benchmark harnesses
crates/web      `simulate_everything` Axum server and HTML templates
crates/replay   `simulate_everything_replay` exporter + static replay server
frontend/       SolidJS + Vite frontend used by the web server
python-client/  Python graph-search client for live play / subprocess integration
docs/           Architecture notes and v2 design docs
```

## Requirements

- Rust toolchain with Cargo
- Node.js for the frontend build
- Python 3 if you want to run the Python agent client

## Quick Start

Build the Rust workspace:

```bash
cargo build
```

Build the frontend:

```bash
cd frontend
npm install
npm run build
```

Run the web app:

```bash
SIMEV_STATIC_DIR=$PWD/frontend/dist cargo run -p simulate-everything-web
```

The server listens on `127.0.0.1:3333` by default.

Useful pages:

- `/` simulator replay viewer
- `/live` live multiplayer / external agents
- `/rr` continuous round-robin tournament
- `/scoreboard` aggregate tournament standings
- `/v2` v2 simulator
- `/v2/rr` v2 round-robin

## Main Binaries

### `simulate_everything_cli`

Run a single simulated game:

```bash
cargo run -p simulate-everything-cli -- 42 2 500
```

Print the final board as ASCII instead of JSON events:

```bash
cargo run -p simulate-everything-cli -- 42 2 500 --ascii
```

Run the benchmark harness:

```bash
cargo run -p simulate-everything-cli -- bench --agents pressure,swarm --seeds 100-249
```

Useful benchmark modes:

- `--matchups all` to run round-robin pairings
- `--profile` for per-turn timing on a single seed
- `--converge` to keep running until the Wilson CI target is reached
- `--size WxH` to override the auto-selected map size

The binary name is `simulate_everything_cli`.

### `simulate_everything`

The Axum server exposes simulator, live, tournament, and replay APIs.

Start it with the built frontend available at `SIMEV_STATIC_DIR`:

```bash
SIMEV_STATIC_DIR=$PWD/frontend/dist cargo run -p simulate-everything-web
```

Key HTTP endpoints:

- `GET /api/game` run a full replay and return JSON
- `GET /api/ascii` render a snapshot as ASCII
- `ws /ws/agent` connect an external live agent
- `ws /ws/spectate` watch live games
- `ws /ws/rr` watch the round-robin stream
- `GET /api/scoreboard` fetch standings JSON
- `GET /api/v2/game` and `GET /api/v2/ascii` run v2 simulations

Key environment variables:

- `SIMEV_STATIC_DIR` path to `frontend/dist`
- `SIMEV_BIND_ADDR` bind address, default `127.0.0.1`
- `SIMEV_PORT` listen port, default `3333`
- `SIMEV_PLAYERS` live lobby size, default `2`
- `SIMEV_TICK_MS` live tick speed, default `250`
- `SIMEV_SEED` initial live-game seed, default `42`
- `SIMEV_PYTHON_CLIENT` path used by the subprocess bridge agent
- `RUST_LOG` tracing level

The binary name is `simulate_everything`.

### `simulate_everything_replay`

Export publishable self-contained replay files:

```bash
cargo run -p simulate-everything-replay -- --seeds 202 --agents pressure,swarm --out ./replays
```

Serve an exported replay directory:

```bash
cargo run -p simulate-everything-replay -- serve --dir ./replays --port 8080
```

## Python Client

Install dependencies:

```bash
cd python-client
pip install -r requirements.txt
```

Connect the live client to a running server:

```bash
python main.py --name GraphBot --host localhost --port 3333
```

Useful flags:

- `--ascii-every N` print periodic board snapshots
- `--verbose` print extra per-turn debug output

The binary name is `simulate_everything_replay`.

## Built-in Agents

Named agents available through `--agents` and `agent_by_name(...)`:

- `expander`
- `swarm`
- `pressure`
- `random`

The simulator shuffles from `all_builtin_agents()`, which adds the Python-backed `graph-search` subprocess agent when `SIMEV_PYTHON_CLIENT` is set. The round-robin pool is a curated subset: `expander`, `swarm`, and `pressure`.

## API Notes

Simulator replay endpoint example:

```bash
curl 'http://127.0.0.1:3333/api/game?seed=42&players=2&turns=500'
```

ASCII snapshot example:

```bash
curl 'http://127.0.0.1:3333/api/ascii?seed=42&players=2&turns=500&at=100'
```

## Documentation

Relevant docs in this repo:

- `docs/architecture.md`
- `docs/v2-engine-spec.md`
- `docs/v2-agent-spec.md`
- `docs/v2-future-layers.md`
- `docs/pressure-agent-handoff.md`

## Development Notes

- The workspace is Rust 2024 edition.
- The frontend uses SolidJS, Vite, and vanilla-extract.
- The replay exporter embeds its own viewer assets and does not need the frontend build output.
- The `v2` stack is present in the engine and web server and is clearly still under active iteration.

## License

See [LICENSE](LICENSE).
