# V3 Sequencing Graph

Created: 2026-04-13
Updated: 2026-04-14
Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2)

## Status: Partial Integration

V3 is substantial, but it is not fully integrated end-to-end in the shared engine/runtime.

Current baseline after the `v3-shared-exec`, `v3-sim-tick`, `v3-rr-runtime`, `v3-state-surfaces`, and `v3-shared-economy` passes:
- shared V3 command application now lives in the engine and is reused by `v3bench`
- the engine now owns a shared agent phase (`run_agent_phase` / `tick_with_agents`) instead of leaving sim integration as a TODO
- `v3bench` and V3 RR now start from the same shared economy-ready bootstrap instead of bench-local support structures
- the engine now owns per-tick food/material production, food consumption, and immediate equipment spawning from shared stockpiles
- the V3 RR loop now runs through the same engine-owned agent phase as bench/sim
- live `/v3/rr` and `/v3/replay` now share the same stack-delta merge logic
- protocol territory/player/task state and strategic perception are now derived from engine state
- roads, settlement founding, supply routes, and richer material-processing loops still contain placeholders and module-only landings

Companion docs:
- [V3 Engine Audit](../v3-engine-audit-2026-04-14.md)
- [V3 Capability Matrix](../v3-capability-matrix.md)

## Wave Status

| Wave | Items | Status |
|------|-------|--------|
| 0 | S1, S2, D1, W1, R1 | Landed |
| 1 | M1, D2, W2, A1, R2 | Landed with integration gaps |
| 2 | M2, W3, A2, E1, P1 | Partial |
| 3 | A3, A4, A5, E2, P2, R3, R4 | Partial |
| 4 | P3, P4 | Partial |
| 4 | R5 | Deferred from V3.0 |

Additional deliverables outside the original sequencing continue to evolve independently and should not be used as evidence that the original V3 spec is complete.

## Open Integration Work

- Finish movement/pathfinding/formation integration in the live tick
- Finish movement/pathfinding/formation integration in the live tick
- Finish settlement, supply-route, and richer production-chain integration in the shared economy
- Finish remaining protocol/economy surfaces that still have no shared engine backing

## Domain Snapshot

| Domain | Spec File | Current Status |
|--------|-----------|----------------|
| **S — Spatial** | `docs/specs/v3-S-spatial.md` | Strong primitive landing; selective runtime integration |
| **M — Movement** | `docs/specs/v3-M-movement.md` | Partial |
| **D — Damage** | `docs/specs/v3-D-damage.md` | Landed |
| **W — Weapons** | `docs/specs/v3-W-weapons.md` | Partial |
| **A — Agents** | `docs/specs/v3-A-agents.md` | Partial |
| **R — Renderer** | `docs/specs/v3-R-renderer.md` | R1-R4 partial, R5 deferred |
| **P — Protocol** | `docs/specs/v3-P-protocol.md` | Partial |

## Sequencing Reference

The original sequencing remains useful as a dependency map, but its completion state must be read through the audit and capability matrix rather than as a shipped-status claim.

```
WAVE 0 (foundations, parallel)
├── S1: Vec3 type, hex-as-projection, pixel_to_hex, spatial index
├── S2: Collision system (separation steering + terrain boundary)
├── D1: Body zone types, Wound struct, bleed/blood/stamina primitives
├── W1: Weapon/Armor property structs, DamageType enum, MaterialType enum
└── R1: PixiJS scaffold (camera, hex grid, continuous-position rendering)

WAVE 1 (core systems, depend on wave 0)
├── M1: Steering behaviors (seek, arrive, separation, obstacle avoidance)
├── D2: Impact resolution pipeline (7-step)
├── W2: Melee attack resolution (swing → impact pipeline)
├── A1: Agent layer types (strategic/operational/tactical directives)
└── R2: Entity rendering with continuous positions, height shading

WAVE 2 (ranged + economy, depend on wave 1)
├── M2: A* pathfinding on hex graph + path smoothing
├── W3: Projectile entities, arc physics, LOS, impact detection
├── A2: Operations layer (role assignment, equipment, stacks, supply)
├── E1: Entity model (component bags, containment, mapgen)
└── P1: Wire protocol types (EntityInfo with Vec3, wounds, equipment)

WAVE 3 (agent intelligence + integration, depend on wave 2)
├── A3: Tactical layer (matchup reasoning, formation, facing, retreat)
├── A4: Strategy layer personalities (Spread, Striker, Turtle)
├── A5: Damage lookup table + observation journal
├── E2: Sim tick loop (integrate all systems)
├── P2: RR loop adaptation + replay format
├── R3: Projectile rendering, wound indicators, equipment at close zoom
└── R4: Viewport culling

WAVE 4 (polish + V2 ports)
├── P3: Port live status WS streaming to V3
├── P4: Port review/capture/flag system to V3
└── R5: Height contour lines, isometric toggle (DEFERRED)
```
