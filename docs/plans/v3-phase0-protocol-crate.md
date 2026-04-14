# Phase 0: Protocol Crate Extraction

Status: **ready for implementation**
Prerequisite for: Streams A, B, C
Linear: reference IA issue if one exists

## Goal

Extract shared wire types into `crates/protocol/` with MessagePack
serialization. Both the server (native) and viewer (wasm32) depend on
this crate. Single source of truth for the wire format.

## User story

**As the platform**, tick data types are defined once and compiled for
both native server and WASM viewer, eliminating type drift between
sender and receiver.

## Current state

Wire types live in `crates/web/src/v3_protocol.rs` (server-only).
The frontend has mirrored TypeScript types in `frontend/src/v3types.ts`.
Serialization is JSON via `serde_json`. No shared crate exists.

## Scope

### New: `crates/protocol/`

```
crates/protocol/
├── Cargo.toml
└── src/
    ├── lib.rs          -- re-exports
    ├── init.rs         -- V3Init (map dimensions, full heightmap, entity list)
    ├── tick.rs         -- V3Snapshot, V3SnapshotDelta, terrain deltas
    ├── entity.rs       -- SpectatorEntityInfo, EntityKind, Role, BodyZone, WoundSeverity
    ├── terrain.rs      -- HeightmapPatch, MaterialPatch (sub-region updates)
    └── body.rs         -- BodyPointData (16-point body model, for future use)
```

```toml
[package]
name = "simulate-everything-protocol"

[dependencies]
serde = { version = "1", features = ["derive"] }
rmp-serde = "1"
glam = { version = "0.30", features = ["serde"] }
```

Must compile for both targets:
```bash
cargo check -p simulate-everything-protocol
cargo check -p simulate-everything-protocol --target wasm32-unknown-unknown
```

### Modified: `crates/web/`

- `v3_protocol.rs`: Remove struct definitions, import from `simulate-everything-protocol`
- Keep the `build_entity_list()`, `DeltaTracker`, and WS broadcast logic
  (these are server concerns, not wire types)
- Serialization: add msgpack encoding alongside JSON. Server sends msgpack
  by default; JSON available via query param for debugging (`?format=json`)

### Modified: `Cargo.toml` (workspace)

- Add `crates/protocol` to workspace members
- Add `simulate-everything-protocol` dependency to `crates/web` and `crates/engine`

## Implementation steps

1. Create `crates/protocol/` with types extracted from `v3_protocol.rs`
2. Add `rmp-serde` dependency, implement `encode()`/`decode()` helpers
3. Update `crates/web` to import types from protocol crate
4. Update `crates/engine` to import shared types where needed (BodyZone, WoundSeverity are currently in engine — decide canonical location)
5. Add msgpack WS encoding in `v3_protocol.rs` broadcast path
6. Verify: `cargo check --workspace`, `cargo test --workspace`
7. Verify: `cargo check -p simulate-everything-protocol --target wasm32-unknown-unknown`

## Design decisions

- **BodyZone, WoundSeverity**: Move canonical definitions to protocol crate.
  Engine re-exports them. This avoids the engine depending on protocol
  for internal types — protocol depends on nothing, engine depends on
  protocol for shared enums.
- **glam::Vec3**: Protocol uses `[f32; 3]` arrays on the wire, not glam
  types directly. Conversion happens at encode/decode boundaries. This
  keeps the wire format stable if glam changes.
- **Backwards compatibility**: JSON encoding remains available via
  `?format=json` query param on WS connect. Existing PixiJS frontend
  continues to work during migration.

## Verification

- [ ] `cargo check --workspace` passes
- [ ] `cargo check -p simulate-everything-protocol --target wasm32-unknown-unknown` passes
- [ ] `cargo test --workspace` passes (all existing tests)
- [ ] WS sends msgpack by default; `?format=json` sends JSON
- [ ] Existing PixiJS frontend works with `?format=json`
- [ ] Round-trip test: encode → decode for all message types
