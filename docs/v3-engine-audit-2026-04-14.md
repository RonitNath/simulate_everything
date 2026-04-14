# V3 Engine Audit

Date: 2026-04-14

Scope: original V3 engine/spec work only. This audit excludes newer in-flight systems under separate development, including the drill runner and remote-drive CLI surfaces.

Primary references:
- `docs/specs/v3-*`
- `docs/plans/v3-*`
- `docs/plans/archive/v3-*`
- `crates/engine/src/v3/**`
- `crates/cli/src/v3bench.rs` where it reveals shared-engine vs harness-only behavior

Companion docs:
- [V3 Capability Matrix](./v3-capability-matrix.md)
- [V3 Sequencing](./plans/v3-sequencing.md)

## Summary

The original V3 engine work is real, but several items were marked complete after landing as modules, tests, or `v3bench` glue rather than shared-engine runtime behavior.

The main failure mode was:
1. spec/module lands
2. later work assumes it is live in the shared engine
3. sequencing/docs mark it complete
4. the missing integration remains in TODO or harness-only code

The `v3-shared-exec` pass improves that baseline by moving V3 command application into the shared engine and making `v3bench` consume it, but it does not yet wire the shared sim tick or RR loop to execute those commands.

## Top Findings

1. Sequencing overstated completion. The previous `docs/plans/v3-sequencing.md` claimed V3.0 implementation complete even though shared-engine command execution, live RR execution, and several protocol/perception fields were still incomplete.

2. Agent command execution was harness-owned. Before this pass, `v3bench` owned the primary implementation of operational/tactical command mutation while the shared sim tick still had an explicit TODO and V3 RR validated commands then dropped them.

3. Movement/pathfinding/formation landed mostly as primitives. The live sim uses only a subset of the promised movement stack. Pathfinding, smoothing, terrain-derived speed factors, and formation-slot placement remain only partially integrated.

4. Perception and protocol remain partially scaffolded. Strategic perception still returns placeholder-grade territory/economy/threat summaries, and several V3 protocol fields are emitted as empty or zeroed placeholders.

5. Combat learning and ranged integration are partial. Damage tables, combat observations, and projectile physics exist, but the full feedback loop and end-to-end ranged path are not fully integrated in shared runtime flow.

## Current Baseline

What is genuinely solid in the shared engine:
- entity model, containment, and mapgen
- damage pipeline and wound/vitals interplay
- melee attack progression and cooldown logic
- spatial primitives, index, and core collision/query helpers
- review/logging substrate

What remains incomplete at runtime:
- sim tick execution of agent outputs
- V3 RR execution through the shared engine path
- full movement/pathfinding/formation integration
- non-placeholder perception/protocol/economy surfaces

## Recommended Next Order

1. Wire `crates/engine/src/v3/sim.rs` to call the shared V3 command executor.
2. Route V3 RR/live execution through that same engine-owned path.
3. Collapse bench-only economy adapters into shared engine systems.
4. Replace placeholder perception/protocol fields with engine-derived data.
5. Finish the remaining movement and combat-parity integration once parallel swordplay work settles.
