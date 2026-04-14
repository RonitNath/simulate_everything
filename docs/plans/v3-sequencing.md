# V3 Sequencing Graph

Created: 2026-04-13
Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2)

## Domain Decomposition

Seven independent design domains. Each can run a /design agent in parallel.
Implementation depends on the sequencing graph below.

| Domain | Plan File | What It Covers |
|--------|-----------|----------------|
| **S — Spatial** | `v3-S-spatial.md` | Vec3 positions, hex-as-projection, pixel_to_hex, hex boundary hysteresis, spatial index (hex grid as hash), collision (separation + terrain), z-axis infrastructure |
| **M — Movement** | `v3-M-movement.md` | Steering behaviors (seek, arrive, separation, cohesion, obstacle avoidance, flow field follow), velocity integration, path smoothing (string-pulling), formation movement, speed modifiers (terrain, encumbrance, slope) |
| **D — Damage** | `v3-D-damage.md` | Impact resolution pipeline (7 steps), body zones, wounds, bleed/blood system, stamina/blocking, material properties (hardness, thickness, sharpness), penetration physics, damage type interactions (slash/pierce/crush), stagger, height modifiers on hit location |
| **W — Weapons** | `v3-W-weapons.md` | Attack pipeline (melee through ranged as parameter spectrum), weapon entities with material properties, armor entities with zone coverage, projectile entities with arc physics, line-of-sight for flat trajectories, gravity for arcs, friendly fire, equipment slots on body zones |
| **A — Agents** | `v3-A-agents.md` | Three-layer architecture (strategy/operations/tactical), strategic directives, operational commands, tactical commands, damage lookup table (moka), observation journal, weapon-armor matchup reasoning, formation types, stack management |
| **R — Renderer** | `v3-R-renderer.md` | PixiJS WebGL, continuous-position rendering, zoom/pan camera, LOD tiers, height visualization (shading + contours), projectile rendering, wound indicators, Flatbush/RBush spatial indexing, SolidJS overlay panels |
| **P — Protocol** | `v3-P-protocol.md` | Wire types (EntityInfo with Vec3 + wounds + equipment), spectator snapshot format, round-robin loop adaptation, replay format, V2 web infrastructure that ports (live status WS, review/capture, flag system) |

## Design Dependencies

Design agents can run on all 7 domains simultaneously. Each domain's spec
stands alone. However, they reference shared concepts:

```
S (Spatial) ← foundation, no dependencies
M (Movement) ← reads from S (Vec3, spatial index, collision)
D (Damage) ← reads from S (Vec3 for angle calc), standalone otherwise
W (Weapons) ← reads from D (impact pipeline), S (projectile positions), M (projectile movement)
A (Agents) ← reads from all above (issues movement commands, combat commands, reads damage tables)
R (Renderer) ← reads from S (continuous positions), W (projectile entities)
P (Protocol) ← reads from S (Vec3 wire format), D (wound info), W (equipment info)
```

All 7 can run /design in parallel because the spec already defines the interfaces.
The design agents refine implementation details within each domain.

## Implementation Sequencing

After design, implementation proceeds in waves. Each wave's work items can run
in parallel. A wave starts only when all its dependencies from prior waves are
complete.

```
WAVE 0 (foundations, parallel)
├── S1: Vec3 type, hex-as-projection, pixel_to_hex, spatial index
├── S2: Collision system (separation steering + terrain boundary)
├── D1: Body zone types, Wound struct, bleed/blood/stamina primitives
├── W1: Weapon/Armor property structs, DamageType enum, MaterialType enum
└── R1: PixiJS scaffold (camera, hex grid, continuous-position rendering)

WAVE 1 (core systems, depend on wave 0)
├── M1: Steering behaviors (seek, arrive, separation, obstacle avoidance)  [needs S1, S2]
├── D2: Impact resolution pipeline (7-step)                                [needs D1, W1]
├── W2: Melee attack resolution (swing → impact pipeline)                  [needs D2, S1]
├── A1: Agent layer types (strategic/operational/tactical directives)       [needs W1]
└── R2: Entity rendering with continuous positions, height shading          [needs S1, R1]

WAVE 2 (ranged + economy, depend on wave 1)
├── M2: A* pathfinding on hex graph + path smoothing                       [needs M1, S1]
├── W3: Projectile entities, arc physics, LOS, impact detection            [needs W2, M1, S1]
├── A2: Operations layer (role assignment, equipment, stacks, supply)       [needs A1, M2]
├── E1: Entity model (component bags, containment, mapgen)                 [needs S1, D1, W1]
└── P1: Wire protocol types (EntityInfo with Vec3, wounds, equipment)       [needs E1]

WAVE 3 (agent intelligence + integration, depend on wave 2)
├── A3: Tactical layer (matchup reasoning, formation, facing, retreat)     [needs A2, W3, D2]
├── A4: Strategy layer personalities (Spread, Striker, Turtle)             [needs A3]
├── A5: Damage lookup table (moka) + observation journal                   [needs D2, W2, W3]
├── E2: Sim tick loop (integrate all systems)                              [needs E1, M1, D2, W2, W3]
├── P2: RR loop adaptation + replay format                                 [needs P1, E2]
├── R3: Projectile rendering, wound indicators, equipment at close zoom    [needs R2, P1]
└── R4: LOD tiers, viewport culling, chunk textures                        [needs R3]

WAVE 4 (polish + V2 ports)
├── P3: Port live status WS streaming to V3                                [needs P2]
├── P4: Port review/capture/flag system to V3                              [needs P2]
└── R5: Height contour lines, isometric toggle (future prep)               [needs R4]
```

## Critical Path

```
S1 → M1 → M2 → A2 → A3 → A4        (spatial → movement → agents)
     ↘
D1 → D2 → W2 → W3 → A3             (damage → weapons → tactical)
          ↘
     W1 → E1 → E2 → P2              (equipment → entity model → sim → protocol)
```

Longest path: **S1 → M1 → M2 → A2 → A3 → A4** (6 waves deep through the agent stack).

Frontend is off critical path: R1 → R2 → R3 → R4 can proceed independently once
S1 and P1 are available.

## Parallel Capacity

| Wave | Max concurrent implementers |
|------|----------------------------|
| 0 | 5 (S1, S2, D1, W1, R1) |
| 1 | 5 (M1, D2, W2, A1, R2) |
| 2 | 5 (M2, W3, A2, E1, P1) |
| 3 | 7 (A3, A4, A5, E2, P2, R3, R4) |
| 4 | 3 (P3, P4, R5) |
