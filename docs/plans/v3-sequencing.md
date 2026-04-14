# V3 Sequencing Graph

Created: 2026-04-13
Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2)

## Status: V3.0 Implementation Complete

All 25 work items across 5 waves shipped. R5 deferred from V3.0 scope.

| Wave | Items | Status |
|------|-------|--------|
| 0 | S1, S2, D1, W1, R1 | Done |
| 1 | M1, D2, W2, A1, R2 | Done |
| 2 | M2, W3, A2, E1, P1 | Done |
| 3 | A3, A4, A5, E2, P2, R3, R4 | Done |
| 4 | P3, P4 | Done |
| 4 | R5 | Deferred from V3.0 |

Additional deliverables (not in original sequencing):
- V3 bench CLI with interestingness scoring, matchup matrix
- Arena duel mode (null-vs-striker, mutual combat)
- Melee approach steering, idle drift fix, threat detection
- FormStack command execution

## Open Items (post V3.0 scope)

- **Arena replay capture** — write V3Snapshot JSONL from CLI for renderer playback
- **Territory overlay** — renderer waiting on engine territory implementation
- **Inspector panel** — click-selected entity detail (SolidJS component)
- **Tooltip enhancement** — entity details on hover
- **R5** — chunk textures, isometric toggle, contour lines

## Domain Decomposition

Seven independent design domains, all specs complete.

| Domain | Spec File | Status |
|--------|-----------|--------|
| **S — Spatial** | `docs/specs/v3-S-spatial.md` | Done |
| **M — Movement** | `docs/specs/v3-M-movement.md` | Done |
| **D — Damage** | `docs/specs/v3-D-damage.md` | Done |
| **W — Weapons** | `docs/specs/v3-W-weapons.md` | Done |
| **A — Agents** | `docs/specs/v3-A-agents.md` | Done |
| **R — Renderer** | `docs/specs/v3-R-renderer.md` | R1-R4 done, R5 deferred |
| **P — Protocol** | `docs/specs/v3-P-protocol.md` | Done |

## Implementation Sequencing (reference)

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
