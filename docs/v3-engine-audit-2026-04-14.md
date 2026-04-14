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

The `v3-shared-exec` pass improved that baseline by moving V3 command application into the shared engine, the `v3-sim-tick` pass added an engine-owned agent phase that `v3bench` reuses, the `v3-rr-runtime` pass routed V3 RR through that same path, the `v3-state-surfaces` pass replaced several placeholder protocol/perception fields with engine-derived state, and the `v3-shared-economy` pass moved stockpile production/consumption and immediate equipment spawning into shared engine code. The next major runtime gaps are now the still-partial movement/pathfinding integration plus the richer settlement/supply/material-processing parts of the economy spec.

## Top Findings

1. Sequencing overstated completion. The previous `docs/plans/v3-sequencing.md` claimed V3.0 implementation complete even though shared-engine command execution, live RR execution, and several protocol/perception fields were still incomplete.

2. Agent command execution was harness-owned. Before the integration passes, `v3bench` owned the primary implementation of operational/tactical command mutation while the shared sim tick still had an explicit TODO and V3 RR validated commands then dropped them.

3. Movement/pathfinding/formation landed mostly as primitives. The live sim uses only a subset of the promised movement stack. Pathfinding, smoothing, terrain-derived speed factors, and formation-slot placement remain only partially integrated.

4. Perception and protocol were partially scaffolded. Strategic perception and the spectator protocol have now moved off the worst placeholders for territory, player aggregates, structures, and task labels. Shared engine task state and stockpiles now back more of those surfaces, but roads and deeper economy semantics are still incomplete.

5. Combat learning and ranged integration are partial. Damage tables, combat observations, and projectile physics exist, but the full feedback loop and end-to-end ranged path are not fully integrated in shared runtime flow.

## Current Baseline

What is genuinely solid in the shared engine:
- entity model, containment, and mapgen
- damage pipeline and wound/vitals interplay
- melee attack progression and cooldown logic
- spatial primitives, index, and core collision/query helpers
- review/logging substrate
- derived territory/player/task state used by protocol and strategic perception

What remains incomplete at runtime:
- full movement/pathfinding/formation integration
- settlement, supply-route, and richer material-processing systems
- roads and other protocol surfaces that still have no shared engine backing
- damage-table learning

## Recommended Next Order

1. Finish movement/pathfinding/formation integration in the shared runtime.
2. Extend the shared economy beyond immediate stockpile production into settlement, supply-route, and richer material-processing flows.
3. Replace the remaining economy placeholders with shared engine state now that bench adapters are retired.
4. Wire combat observations back into the damage-table learning loop.
5. Finish the remaining movement and combat-parity integration once parallel swordplay work settles.
