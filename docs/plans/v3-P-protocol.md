# V3 Domain: P — Protocol and Web Integration

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Observation/Protocol section)
Sequencing: `docs/plans/v3-sequencing.md`

## Purpose

Update the wire protocol for V3's continuous positions, wound/equipment state,
and projectile entities. Adapt the round-robin loop and replay format. Port V2
web infrastructure (live status WS, review/capture, flag system) to V3.

## Design Questions

### P.1 Wire Types

- EntityInfo in the spec has ~15 optional fields. Should the protocol send all
  fields for all entities (simple, fat), or should it use a discriminated union
  (PersonInfo | StructureInfo | ProjectileInfo) to reduce payload? Discriminated
  union is more efficient but harder to extend.
- Position format: Vec3 as three f64s per entity. At 500 entities, that's 12KB
  of position data per tick. Acceptable? Could compress to f32 for wire (losing
  precision that only matters in the engine). Or delta-encode positions (send
  diff from previous tick). Delta encoding is complex but dramatically reduces
  payload for entities that didn't move.
- Wound info: full wound list per entity is expensive to send. For spectators,
  send aggregate: `wound_count: u8, blood: f32` is enough. Agents get full detail
  for own entities, aggregate for visible enemies.
- Equipment info: for spectators, `weapon_type: Option<String>, dominant_armor: Option<String>`.
  Not the full material properties — just enough for the renderer to show the
  right icon.
- Projectile info: `id, x, y, z, vx, vy, vz, damage_type`. Projectiles are
  short-lived entities with high update frequency. Send them in a separate array
  from persistent entities? Or mixed in?

### P.2 Snapshot Format

- V2 sends full snapshots every tick with delta hex_changes. V3 entities have
  continuous positions that change every tick, so delta-encoding is harder.
- For V3.0 at 30×30 with ~500 entities: full entity list per tick. Each entity
  ~60 bytes → 30KB per tick at 1 tick/sec. With projectiles, maybe 35KB. Over
  WebSocket this is fine.
- For future scale (10k entities): delta encoding becomes necessary. Design the
  snapshot format now to allow future delta mode even if V3.0 sends full snapshots.
  Approach: each snapshot has a `full: bool` flag. full=true sends everything.
  full=false sends only changed entities (position delta > threshold, or state
  changed).
- Hex data (terrain, territory): sent once on init, then only changes. This is
  already the V2 pattern (v2_init + v2_snapshot). Keep it.

### P.3 Round-Robin Loop

- The V2 RR loop in `v2_roundrobin.rs` manages: game lifecycle, agent polling,
  spectator broadcast, review buffer, pause/resume/reset. The structure survives
  to V3 — it's game-mode logic, not engine logic.
- What changes: agent initialization (three-layer agents), snapshot building
  (EntityInfo from Engine entities), tick cadence (dt-based instead of fixed
  tick count).
- Agent poll cadence: strategy every ~50 game-seconds, operations every ~5,
  tactical every tick for engaged stacks. The RR loop needs to track game_time
  and dispatch agent layers at the right cadences.
- Spectator catchup: late-joining spectators need full state. V2 sends v2_init +
  full v2_snapshot. V3 sends v3_init (terrain, height, regions) + full entity
  snapshot. Same pattern, different content.

### P.4 Replay Format

- V2 replay records per-tick snapshots in a Vec. V3 replay needs continuous
  positions, wounds, equipment. Same approach: Vec of snapshots, each containing
  the full entity list for that tick.
- Replay size: at 500 entities × 60 bytes × 3000 ticks = 90MB uncompressed.
  That's large. Compression: gzip the replay JSON? Or switch to a binary format
  (bincode, MessagePack)? For V3.0: JSON + gzip. Binary format in V3.1 if size
  is a problem.
- Replay reconstruction: the damage pipeline is deterministic, so you could
  reconstruct wounds from the replay by re-running combat. But storing wound
  state directly in replay snapshots is simpler and allows the frontend to show
  wound state without re-simulation. Store wounds.

### P.5 V2 Web Infrastructure Port

These V2 features port to V3 with minimal changes:

- **Live status WS streaming** (from `v2-rr-live-status-hover-followup.md`):
  The `v2_rr_status` message is game-mode metadata (pause state, tick speed,
  capturable range). None of this is entity-model-specific. Rename to
  `v3_rr_status`, keep the same fields. If this was implemented in V2, port
  directly. If not, implement fresh on V3.
- **Review/capture system**: flag-tick and segment capture are about snapshotting
  the game state at specific ticks. The review bundle format changes (V3 entities
  instead of V2 units), but the capture/storage/retrieval machinery is the same.
- **Hover inspector**: the concept (hover hex → show details) survives. The data
  model changes (show wounds, equipment, continuous position). Tick-gated hover
  behavior applies regardless of entity model.

### P.6 API Routes

- Rename V2 routes to V3? Or keep /api/v2/ for backwards compatibility?
  Recommendation: create /api/v3/ routes, keep /api/v2/ as legacy (V2 engine
  still exists, still useful for development). The V3 RR is at /v3/rr, the V3
  simulator at /v3.
- New route considerations: /api/v3/equipment-catalog (list available weapon/armor
  types for UI display)? Or is this sent in the init message?

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| P1 | 2 | E1 | Wire protocol types (EntityInfo, ProjectileInfo, snapshot format) |
| P2 | 3 | P1, E2 | RR loop adaptation, replay format, spectator catchup |
| P3 | 4 | P2 | Port live status WS streaming |
| P4 | 4 | P2 | Port review/capture/flag system |

## Key Files (Expected)

- `crates/web/src/v3_protocol.rs` — wire types, serialization
- `crates/web/src/v3_roundrobin.rs` — RR loop adapted for V3 engine
- `crates/engine/src/v3/replay.rs` — replay recording and reconstruction
- `frontend/src/v3types.ts` — TypeScript types matching wire protocol

## Constraints

- Snapshot payload must stay under 100KB per tick at 500 entities (for smooth
  WebSocket streaming).
- Spectator catchup must complete in < 100ms (single full snapshot).
- Replay format must be deterministic: same seed + same agents = same replay.
- All V2 web features that exist must have V3 equivalents. Nothing regresses.
