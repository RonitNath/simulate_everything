# Spec: V3 Protocol & Web Integration

## Vision

Define the wire protocol, replay format, round-robin loop, and review system for
V3's continuous-position entity simulation. The protocol serves two consumers:
the SolidJS frontend (live spectating, replay scrubbing) and Claude Code (text-
based debugging via review bundles). V3.0 agents are in-process only — no
external agent wire format.

## Use Cases

### UC1: Spectator watches a live RR game
1. Connect to `/ws/v3/rr`. Receive `V3Init` (terrain, height, regions, agent
   names, game number) + full `V3Snapshot` (all entities + all projectiles).
2. Each tick, receive `V3SnapshotDelta` (changed entities, new/removed
   projectiles, hex territory changes).
3. Frontend renders continuous positions, wound indicators, equipment icons,
   projectile arcs. Client-side fog filter lets spectator toggle per-player view.
4. On game end, receive `V3GameEnd` (winner, tick, scores).

### UC2: Spectator flags a moment for review
1. Watching live, spectator sees slingers charging plate infantry. Hits flag
   hotkey. Frontend sends `POST /api/v3/rr/flags` with `{ tick, annotation }`.
2. Server promotes the current trace window (50 ticks before, 50 ticks after
   the flagged tick) from the rolling buffer to a permanent named directory:
   `var/v3_reviews/game_{N}/flag_{tick}/`.
3. Bundle contains: ASCII state dump, agent decision traces, combat log window,
   entity detail dump, and a chunked replay segment for the window.

### UC3: Claude Code debugs a flagged moment
1. Ronit opens a session: "look at var/v3_reviews/game_12/flag_4237/, slingers
   are charging plate."
2. Claude Code reads the agent decision trace — sees what tactical layer
   decided, checks the damage lookup table entries for sling-vs-plate.
3. Claude Code reads the combat log — sees penetration results, wound outcomes.
4. Claude Code reads the ASCII state dump — sees entity positions, formations,
   stack composition.
5. Reports: bug in target selection vs stale damage estimate vs correct behavior.

### UC4: Replay scrubbing
1. Load a replay file. Read the chunk index header.
2. Seek to tick 4000: find chunk 40 (ticks 4000-4099), decompress that one
   chunk. Read the keyframe, apply deltas to reach the target tick.
3. Scrub forward/backward within the chunk by applying/unapplying deltas from
   the nearest keyframe (at most 99 steps).
4. Step across chunk boundaries by loading adjacent chunks on demand.

### UC5: RR loop auto-cycles games
1. Autoplay toggle is on. Game ends, 500ms delay, new game starts.
2. Random agent selection from pool. Scoreboard updated with results.
3. Agent versioning tracked — scoreboard records which version of each agent
   played.
4. User can pause, resume, reset, change tick speed, switch time resolution
   mode via control API.

## Architecture

### Wire Types

**`V3Init`** — sent once on spectator connect:
```
width: u32
height: u32
terrain: Vec<f32>          // static terrain values per hex
height_map: Vec<f32>       // elevation per hex
material_map: Vec<f32>     // material richness per hex
region_ids: Vec<u16>       // region membership per hex
player_count: u8
agent_names: Vec<String>
agent_versions: Vec<String>
game_number: u64
```

**`V3Snapshot`** — full state, sent on connect and as replay keyframes:
```
tick: u64
dt: f32                              // game-seconds this tick covers
full_state: true
entities: Vec<SpectatorEntityInfo>
projectiles: Vec<ProjectileInfo>
stacks: Vec<StackInfo>
hex_ownership: Vec<Option<u8>>       // per-hex territory
hex_roads: Vec<u8>                   // per-hex road level
hex_structures: Vec<Option<u32>>     // per-hex structure entity ID
players: Vec<PlayerInfo>
```

**`V3SnapshotDelta`** — per-tick update:
```
tick: u64
dt: f32
full_state: false
entities_appeared: Vec<SpectatorEntityInfo>   // newly visible, full data
entities_updated: Vec<EntityUpdate>           // changed fields only
entities_removed: Vec<u32>                    // entity IDs no longer present
projectiles_spawned: Vec<ProjectileInfo>
projectiles_removed: Vec<u32>
stacks_created: Vec<StackInfo>
stacks_updated: Vec<StackUpdate>
stacks_dissolved: Vec<u32>
hex_changes: Vec<HexDelta>                   // only changed hexes (dirty bitset)
players: Vec<PlayerInfo>
```

**`SpectatorEntityInfo`** — flat struct with optional fields:
```
id: u32
owner: Option<u8>
x: f32, y: f32, z: f32              // continuous world position (f32 — sub-mm at 30km)
hex_q: i32, hex_r: i32              // derived hex (convenience)
facing: Option<f32>                  // radians
entity_kind: EntityKind              // Person | Structure
role: Option<Role>                   // Farmer, Soldier, Builder, etc.
blood: Option<f32>                   // 0.0-1.0
stamina: Option<f32>                 // 0.0-1.0
wounds: Vec<(BodyZone, WoundSeverity)>  // zone + 2-bit severity (None/Light/Serious/Critical)
weapon_type: Option<String>          // equipped weapon name
armor_type: Option<String>           // dominant armor name
resource_type: Option<ResourceType>
resource_amount: Option<f32>
structure_type: Option<StructureType>
build_progress: Option<f32>          // 0.0-1.0 for incomplete structures
contains_count: usize                // population/garrison count
stack_id: Option<u32>                // stack membership
current_task: Option<String>         // human-readable task label
```

Spectators see all entities with the visible-enemy wound tier (zone + 2-bit
severity). No fog of war for spectators — client-side filtering only.

**`ProjectileInfo`** — separate from entities, tight struct:
```
id: u32
x: f32, y: f32, z: f32
vx: f32, vy: f32, vz: f32
damage_type: DamageType              // Slash, Pierce, Crush
```

**`StackInfo`**:
```
id: u32
owner: u8
members: Vec<u32>                    // entity IDs
formation: FormationType             // Line, Column, Wedge, Square, Skirmish
center_x: f32, center_y: f32
facing: f32
```

**`PlayerInfo`** — faction-level aggregates:
```
id: u8
population: u32
territory: u32                       // hex count
food_level: u8                       // 0-3 bucket
material_level: u8                   // 0-3 bucket
alive: bool
score: u32
```

**`HexDelta`**:
```
index: u32
owner: Option<u8>
road_level: Option<u8>
structure_id: Option<Option<u32>>    // Some(None) = structure removed
```

### Agent Observation Protocol (In-Process, Not Wire)

Not part of the wire protocol — included here as the internal contract that
the E domain implements and the A domain consumes.

**First tick**: `FullObservation` — all visible entities (with own=full wound
detail, enemy=zone+severity approximation), terrain, economic state, stacks.

**Subsequent ticks**: `ObservationDelta`:
```
tick: u64
dt: f32
entities_appeared: Vec<EntityObservation>    // newly visible, full snapshot
entities_updated: Vec<EntityUpdate>          // changed since last tick
entities_lost: Vec<u32>                      // left vision range (just IDs)
stacks_created: Vec<StackObservation>
stacks_updated: Vec<StackUpdate>
stacks_dissolved: Vec<u32>
combat_events: Vec<CombatEvent>              // hits/blocks/kills in vision
economic_state: EconomicSummary              // faction resources, production
```

Visibility tiers for entity observation:
- **Own entities**: Full `Vec<Wound>` (zone, severity, bleed_rate, age),
  exact blood/stamina, full equipment, current task
- **Visible enemies**: `Vec<(BodyZone, WoundSeverity)>` — zone + 2-bit
  approximation. Position, facing, weapon/armor type visible.
- **Not visible**: Nothing. Entity appears in `entities_lost`.

Agent returns `Vec<Command>` every tick (possibly empty). Command categories:

- **Entity commands**: Target a specific EntityId. Tasks: farm, build, trade,
  garrison, produce, patrol, gather.
- **Stack commands**: Target a StackId. Orders: move (with formation + facing),
  engage target stack, retreat to rally point, escort, patrol route.
  Stack lifecycle: `CreateStack { members, formation }`, `ModifyStack { add,
  remove }`, `DissolveStack`.
- **Economic commands**: Faction-level. Set production priority, allocate
  materials, establish trade route.

StackIds are engine-assigned. Agent proposes creation, engine validates (all
alive, all owned, not in another stack), assigns ID. Agent sees the StackId in
the next tick's delta under `stacks_created`.

Agent handles its own polling cadence internally (strategy ~50s, ops ~5s,
tactical every tick for engaged stacks). The RR loop sends deltas every tick
and receives commands every tick. Empty command list = no action.

### Replay Format

Chunked keyframe + delta format with independent compression.

**File structure**:
```
[Header]
  magic: b"SV3R"
  version: u16
  game_seed: u64
  agent_names: Vec<String>
  agent_versions: Vec<String>
  init: V3Init (terrain, height, etc.)
  chunk_count: u32
  chunk_index: Vec<ChunkEntry>       // tick_start, tick_end, byte_offset, byte_length

[Chunk 0] (gzipped independently)
  keyframe: V3Snapshot               // full state at tick 0
  deltas: [V3SnapshotDelta; 99]      // ticks 1-99
  commands: [Vec<Command>; 100]      // agent commands per tick, per player
  dt_per_tick: [f32; 100]            // dt value for each tick (captures mode switches)

[Chunk 1] (gzipped independently)
  keyframe: V3Snapshot               // full state at tick 100
  deltas: [V3SnapshotDelta; 99]      // ticks 101-199
  commands: [Vec<Command>; 100]
  dt_per_tick: [f32; 100]

...
```

**Chunk size**: 100 ticks per chunk. Each chunk independently gzipped.

**Seeking**: Read header index → find chunk by tick range → decompress single
chunk → apply deltas from keyframe to target tick. At most 99 delta
applications. Sub-second on any modern hardware.

**Two streams per tick**:
1. World state (keyframe + deltas) — what happened
2. Agent commands — what was decided

Playback applies stored commands to stored state. No agent code runs. Old
replays work with new agent code. Determinism guarantee: same seed + same
command sequence = same world state (verification invariant, not playback
mechanism).

**V3.0 serialization**: JSON + gzip per chunk. Binary format (bincode/
MessagePack) deferred to V3.1 if size is a problem.

**Estimated size**: 500 entities × ~80 bytes × 3000 ticks = ~120MB
uncompressed. With gzip (~5:1 on JSON), ~25MB. Keyframe overhead ~1%.

### Round-Robin Loop

Same lifecycle structure as V2. Key adaptations:

**Game lifecycle**:
1. Autoplay toggle (default: on). When on, new game starts 500ms after
   previous ends.
2. Random agent selection from `rr_agents()` pool.
3. Generate map from incremented seed.
4. Broadcast `V3Init` + full `V3Snapshot`.
5. Tick loop: engine `tick(state, dt)` → broadcast delta → record trace →
   record replay chunk → sleep to meet wall-clock interval.
6. On game end: broadcast `V3GameEnd`, update scoreboard, finalize any
   pending review captures.

**Time resolution modes**:
- Strategic: dt = 3600.0 (1hr game-time per tick). Economy and movement.
- Tactical: dt = 1.0 (1s). Default. Full combat fidelity.
- Cinematic: dt = 0.01 (10ms). Frame-by-frame combat.

Mode switching is a control command. dt changes for subsequent ticks. Wall-
clock tick interval (tick_ms) is independent — controls visual pacing only.
The replay records dt per tick.

**Spectator management**:
- Broadcast channel for live deltas.
- Cached `V3Init + V3Snapshot` for late-joining spectators (<100ms catchup).
- No server-side fog filtering — spectators get full visibility, client
  applies per-player filter.

**Agent polling**: Every tick, send `ObservationDelta` to each in-process
agent. Receive `Vec<Command>` (possibly empty). No fixed polling interval —
deltas every tick, agents decide internally when to act.

**Scoreboard**: Track wins/losses/draws per agent name + version. Persist
across games. Display via status API.

**Control API** (same surface as V2, new namespace):
- `GET  /api/v3/rr/status` — paused, tick_ms, game_number, dt, mode,
  capturable bounds, active capture, autoplay state
- `POST /api/v3/rr/config` — set tick_ms, mode (strategic/tactical/cinematic),
  autoplay toggle
- `POST /api/v3/rr/pause`
- `POST /api/v3/rr/resume`
- `POST /api/v3/rr/reset`

**RrStatus WS message** — broadcast on state change:
```
game_number: u64
current_tick: u64
dt: f32
mode: TimeMode                       // Strategic | Tactical | Cinematic
paused: bool
tick_ms: u64
autoplay: bool
capturable_start_tick: u64
capturable_end_tick: u64
active_capture: Option<CaptureInfo>
```

### Review System

**Trace recording** — always on in RR mode:

The RR loop maintains a rolling trace buffer. Every tick:
1. ASCII state snapshot (entity positions, health, stacks, formations)
2. Agent decision traces (structured `Vec<AgentTrace>` enums per agent)
3. Combat log entries (who hit who, weapon, armor, penetrated, severity)
4. Replay chunk data (world state + commands)

Buffer overwrites continuously. No disk I/O unless flagged.

**Flagging** — promotes a window to permanent storage:

`POST /api/v3/rr/flags` with `{ game_number, tick, annotation }`.

Server captures a window: 50 ticks before to 50 ticks after the flagged tick.
Writes to `var/v3_reviews/game_{N}/flag_{tick}/`.

**Review bundle directory structure**:
```
var/v3_reviews/game_12/flag_4237/
├── summary.json          # metadata: game, tick, annotation, agent names/versions, seed
├── ascii_state.txt       # entity positions, health, stacks, formations at flagged tick
├── decision_trace.json   # per-agent, per-tick decision log for the window
├── combat_log.json       # raw combat observations for the window
├── entity_detail.json    # full component state for entities near the action
└── replay/               # chunked replay segment for the 100-tick window
    ├── header.json       # init data + single chunk index
    └── chunk_0.json.gz   # keyframe at window start + deltas
```

**Text-readable formats** — the primary consumer is Claude Code:
- `ascii_state.txt`: Grid with entity markers, legend with positions/health/
  stacks. Like `/api/v2/ascii` but for V3 entities.
- `decision_trace.json`: Array of per-tick entries. Each entry has per-agent
  arrays of structured `AgentTrace` enums. Queryable by field — e.g., "all
  tactical traces where damage_estimate < 0.1 and action == Engage" is a
  grep, not a parse. Example:
  ```json
  { "tick": 4237, "agents": {
      "Striker_v2": [
        { "Strategy": {
            "posture": "Attack",
            "trigger": "enemy territory expanded into region NE",
            "alternatives_considered": [["Defend", 0.3], ["Expand", 0.2]]
        }},
        { "Operations": {
            "task_type": "EquipStack",
            "target": "stack 3 (12 soldiers + 2 archers)",
            "reason": "assigned to attack region NE, nearest available force",
            "resource_cost": 45.0
        }},
        { "Tactical": {
            "stack": 3,
            "action": "Engage",
            "target_stack": 7,
            "damage_estimate": 0.4,
            "alternatives": [[5, "Engage", 0.15], [9, "Engage", 0.08]]
        }},
        { "Tactical": {
            "stack": 3,
            "action": "Engage",
            "target_stack": 12,
            "damage_estimate": 0.02,
            "alternatives": []
        }}
      ]
  }}
  ```
  In the example above, the last trace shows stack 3 engaging stack 12 with
  a 0.02 damage estimate — a sling-vs-plate mismatch that's trivially
  filterable: `jq '.agents[][].Tactical | select(.damage_estimate < 0.1)'`.
- `combat_log.json`: Array of combat events. Each: tick, attacker_id,
  defender_id, weapon_type, armor_type, body_zone, penetrated (bool),
  wound_severity, attacker_stack, defender_stack.
- `entity_detail.json`: Full component dump for entities in the flagged
  area — wounds (full detail), equipment, stamina, blood, stack membership,
  current task, formation position.

**Segment capture** — same as V2:
- `POST /api/v3/rr/capture/start` — begin recording from current tick
- `POST /api/v3/rr/capture/stop` — finalize segment, persist to disk
- Segment bundles include the same text-readable files for the full segment.

**Review listing**:
- `GET    /api/v3/rr/reviews` — list all bundles (pending + saved)
- `GET    /api/v3/rr/reviews/{id}` — load a specific bundle summary
- `DELETE /api/v3/rr/reviews/{id}` — delete a bundle

### API Routes

New `/api/v3/` namespace. V2 routes remain for V2 engine.

| Route | Method | Purpose |
|-------|--------|---------|
| `/ws/v3/rr` | WS | Live spectator stream |
| `/api/v3/rr/status` | GET | RR status (mode, dt, autoplay, etc.) |
| `/api/v3/rr/config` | POST | Set tick_ms, mode, autoplay |
| `/api/v3/rr/pause` | POST | Pause simulation |
| `/api/v3/rr/resume` | POST | Resume simulation |
| `/api/v3/rr/reset` | POST | Abort game, start new |
| `/api/v3/rr/flags` | POST | Flag a tick for review |
| `/api/v3/rr/capture/start` | POST | Begin segment capture |
| `/api/v3/rr/capture/stop` | POST | End segment capture |
| `/api/v3/rr/reviews` | GET | List review bundles |
| `/api/v3/rr/reviews/{id}` | GET | Load review bundle |
| `/api/v3/rr/reviews/{id}` | DELETE | Delete review bundle |

### Cross-Domain Contracts

**P → A (agent decision trace)**:
Every agent layer must emit structured trace enums to a `Vec<AgentTrace>`
decision log per tick when `trace: bool` is true on the agent context. Traces
are typed data (not free-form strings) so they're queryable programmatically
for pattern detection, regression testing, and dashboards. All trace variants
implement `Debug`/`Display` for human readability.

Trace enum variants (defined in A domain, serialized by P domain):
```
AgentTrace:
  Strategy {
    posture: Posture,                    // Attack | Defend | Expand
    trigger: String,                     // what caused this evaluation
    alternatives_considered: Vec<(Posture, f32)>  // (option, score)
  }
  Operations {
    task_type: OpTaskType,               // AssignRole | EquipStack | BuildRoute | ...
    target: String,                      // entity/stack/hex description
    reason: String,                      // why this task was chosen
    resource_cost: Option<f32>           // estimated resource expenditure
  }
  Tactical {
    stack: u32,                          // StackId
    action: TacticalAction,             // Engage | Retreat | Reposition | Hold
    target_stack: Option<u32>,           // enemy StackId if applicable
    damage_estimate: Option<f32>,        // expected damage rate from lookup table
    alternatives: Vec<(u32, TacticalAction, f32)>  // (target, action, score)
  }
```

The P domain defines the bundle format; the A domain defines the enum variants
and emission points. Trace is always on in RR mode.

**P → E (entity component access)**:
The spectator snapshot builder and ASCII state dumper need read access to all
entity components (position, wounds, equipment, stack membership, task). The E
domain must expose a query API for this — P does not reach into component
storage directly.

**P → R (frontend types)**:
`frontend/src/v3types.ts` mirrors the wire protocol types. R domain (renderer)
consumes these types. P defines them, R renders them.

## Security

- Spectators are read-only WebSocket consumers. No mutations via WS.
- Control API (pause/resume/reset/config/flags/capture) is unauthenticated,
  localhost-only. Same trust model as V2.
- No external agent wire format in V3.0. Agents are in-process Rust code.
  External agent support (with command validation, ownership checks, rate
  limiting) is a future version.
- Review bundles written to local filesystem only. No network exposure of
  bundle contents beyond the review listing API.

## Privacy

No PII handled. All entities are simulated. Review bundles contain game state
only.

## Audit

Mutations logged via control API (pause, resume, reset, config changes, flags,
captures). Game results logged to scoreboard. Agent decision traces provide
full audit trail for in-game decisions.

## Convention References

- `CLAUDE.md` — workspace layout, agent pools, env vars, RR control
- `.claude/CLAUDE.local.md` — systemd service management, deploy workflow
- `docs/architecture.md` — V2 protocol, routes, game rules

## Convention Observations

- V2 uses a flat `SpectatorEntity` with optional fields. V3 continues this
  pattern (not discriminated unions). If entity kinds proliferate beyond
  Person/Structure, the flat struct may become unwieldy — observe after V3.0.
- V2 review bundles are single JSON files. V3 splits into a directory of
  purpose-specific files (ascii, traces, combat log, replay). This is a
  deliberate divergence — text-debuggability requires separate files that
  Claude Code can read independently.
- Agent decision trace is a new contract that doesn't exist in V2. It crosses
  the P/A domain boundary via structured `AgentTrace` enums (not free-form
  strings). If trace overhead becomes measurable, revisit — but enum
  construction behind a flag is expected to be negligible.

## Scope

### V3.0 (ship this)
- Wire protocol types (V3Init, V3Snapshot, V3SnapshotDelta, ProjectileInfo,
  StackInfo, SpectatorEntityInfo, PlayerInfo, HexDelta)
- Spectator WebSocket endpoint with init + delta streaming
- Late-joiner catchup (cached init + full snapshot, <100ms)
- Client-side fog-of-war filter (spectator toggles per-player view)
- RR loop adapted for V3 engine (fixed dt, mode switching, autoplay toggle,
  scoreboard with agent versioning)
- Replay format (chunked keyframe + delta, gzipped, indexed, two streams)
- Review system with text-debuggable bundles (ASCII state, decision traces,
  combat log, entity detail, replay segment)
- Control API (/api/v3/rr/*)
- RrStatus WS message with mode/dt/autoplay
- Frontend TypeScript types (v3types.ts)
- Rolling trace buffer (always-on in RR, overwritten unless flagged)
- Delta snapshot encoding (DeltaTracker diffs consecutive ticks, broadcasts
  only changed fields — position/facing/vitals with epsilon thresholds)
- CombatLog on GameState, drained per tick into review bundles
- AgentTrace enum (Strategy/Operations/Tactical) emitted per layer invocation,
  structured data in decision_trace.json (queryable via jq)
- Frontend delta application (V3GameState + initGameState + applyDelta)

### Deferred
- **External agent wire format** — serialization layer over command enums.
  Deferred until external agent support is needed.
- **Binary replay format** — bincode/MessagePack. Deferred to V3.1 if JSON +
  gzip size is a problem.
- **Chunked replay file format** — keyframe + delta chunks with gzip and byte
  offset index. Deferred until standalone replay export is needed. V3.0
  records review bundles per-flag/segment, not full game replays.
- **Replay recording compression** — stream-level compression during
  recording. V3.0 compresses per-chunk at finalization.
- **Multi-game replay archives** — bundling multiple game replays. V3.0 is
  one file per game.
- **Scoreboard persistence** — wins/losses/draws per agent. V3.0 has the
  data structures but no scoreboard accumulation across games yet.
- **Agent command execution** — operational and tactical commands are
  validated but not applied. The engine needs command executors.

## Verification

- [x] Spectator connects to `/ws/v3/rr`, receives V3Init + V3Snapshot,
      then V3SnapshotDelta per tick.
- [x] Late-joining spectator receives cached full snapshot.
- [ ] Spectator can toggle per-player fog-of-war view (client-side filter).
- [x] Control API: pause, resume, reset, config (tick_ms, mode, autoplay)
      all function correctly. Status reflects current state.
- [x] Mode switching: changing to strategic/tactical/cinematic updates dt
      for subsequent ticks.
- [x] Autoplay toggle: on = auto-cycle games, off = stop after current game.
- [ ] Scoreboard tracks wins/losses/draws per agent name + version.
- [x] Flag a tick: review bundle appears in `var/v3_reviews/game_{N}/flag_{T}/`
      with all 5 files (summary, ascii_state, decision_trace, combat_log,
      entity_detail).
- [x] ASCII state dump is human/Claude-readable: entity positions, health,
      stacks, formations in text format.
- [x] Decision trace contains per-agent, per-tick structured AgentTrace
      enums (Strategy/Operations/Tactical) queryable via jq.
- [x] Combat log contains full hit resolution detail: weapon, armor, zone,
      penetrated, severity (CombatObservation from sim tick).
- [x] Segment capture (start/stop) produces a bundle with the same format.
- [x] Review listing API returns all bundles. Bundles deletable.
- [ ] Replay file: seek to arbitrary tick via chunk index, decompress single
      chunk, apply ≤99 deltas. Sub-second. (Chunked format deferred.)
- [x] Delta encoding: DeltaTracker diffs entities/projectiles/stacks per tick,
      broadcasts only changed fields with epsilon thresholds.
- [x] Frontend TypeScript types + applyDelta() for client-side delta application.
- [ ] All V2 web features have V3 equivalents. Nothing regresses.

## Deploy Strategy

P domain is part of the `simulate_everything` binary. No separate deployment.
Follows existing systemd service pattern in `.claude/CLAUDE.local.md`. Frontend
build via `cd frontend && bun install && bun run build`.

New env var: `SIMEV_V3_RR_REVIEW_DIR` (default: `var/v3_reviews`) — directory
for persisted V3 review bundles.

## Files Modified

### New files
- `crates/web/src/v3_protocol.rs` — wire types, serialization, snapshot builders,
  DeltaTracker for delta encoding
- `crates/web/src/v3_roundrobin.rs` — RR loop adapted for V3 engine
- `crates/web/src/v3_review.rs` — review system (trace buffer, flag, capture,
  bundle writing, listing)
- `frontend/src/v3types.ts` — TypeScript types matching wire protocol, delta
  application helpers (V3GameState, initGameState, applyDelta)

### Modified files
- `crates/web/src/main.rs` — add V3 routes, WS handler, control API
- `crates/engine/src/v3/state.rs` — add CombatLog to GameState
- `crates/engine/src/v3/sim.rs` — record CombatObservation during impact resolution
- `crates/engine/src/v3/agent.rs` — add AgentTrace enum, emit traces from LayeredAgent
- `crates/engine/src/v3/combat_log.rs` — add Clone derive
- `CLAUDE.md` — add V3 routes, env vars, key references

## Implementation History

| Chunk | Commit | Deliverable |
|-------|--------|-------------|
| P1 | `dbc1a70`, `b7bcd23` | Wire protocol types, snapshot builders, v3types.ts |
| P2 | `8bda64d` | RR loop, spectator WS, control API, RrStatus |
| P3 | `762753d` | Review system (trace buffer, flag, capture, bundles) |
| P-traces | `9abfad7` | CombatLog on GameState, AgentTrace enum, real decision traces |
| P-deltas | `9322895` | DeltaTracker, delta encoding, frontend applyDelta |
