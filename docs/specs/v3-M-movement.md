# Spec: V3 Movement Domain (M)

## Vision

Replace V2's discrete hex-to-hex teleportation with continuous, physics-based movement.
Entities have velocity vectors. Steering behaviors produce acceleration. Position integrates
each tick. The hex grid serves as a navigation graph for pathfinding — not the movement
substrate. Speed is fully derived from physics each tick: no stored max_speed, just the
product of base capability and current conditions.

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Movement section)
Sequencing: `docs/plans/v3-sequencing.md`
Depends on: S domain (landed)

## Use Cases

1. **Single entity movement** — An agent issues a move command with a destination. Pathfinding
   produces hex waypoints via A*. Path smoothing (string-pulling) removes unnecessary
   waypoints. Steering behaviors drive the entity along the smoothed path. Entity accelerates,
   cruises, and decelerates naturally via Arrive behavior at the final waypoint.

2. **Formation movement** — Operations layer assigns entities to a stack with a formation type
   (Column, Line, or Wedge). Formation computes target offsets from center-of-mass rotated by
   facing. Each entity steers toward its formation slot while the group moves toward the
   destination. Formation deforms around obstacles, reforms after clearing them.

3. **Obstructed movement** — Entity encounters impassable terrain or a blocked path. Movement
   stops. System signals "path blocked" upward. Agent decides next action (re-route, wait,
   request new orders). Movement layer does not autonomously re-route.

4. **Varied terrain traversal** — Entity moves across terrain with varying slope, surface
   material, and road coverage. Speed changes continuously as modifiers update each tick.
   Heavy armor on steep muddy slopes is visibly punishing. Light troops on roads move fast.

5. **Wounded movement** — Entity with leg laceration moves slower (wound factor < 1.0). Entity
   with leg fracture is immobile (wound factor = 0.0). Entity with low stamina sprints slower
   (stamina factor reduces derived speed). Entity below blood threshold (< 0.2) collapses.

## Architecture

### Components

**Mobile** (revised from V2 — replaces stored speed/cooldown with continuous physics):
```rust
pub struct Mobile {
    pub vel: Vec3,              // Current velocity
    pub steering_force: f32,    // Max acceleration magnitude
    pub radius: f32,            // Collision radius (~10m person, ~30m cart)
    pub waypoints: Vec<Vec3>,   // Smoothed path (next waypoint first)
}
```

No `max_speed` field. Speed is derived each tick:
```
derived_speed = base_capability * slope_factor * surface_factor
              * encumbrance_factor * wound_factor * stamina_factor
```

All factors in `[0.0, 1.0]`. `base_capability` is derived from entity physiology (species,
leg count, body type). Example base values (m/s): swordsman 3.0, spearman 2.8, archer 3.0,
slinger 3.5, worker 2.5, ox cart 2.0. These are starting points to be tuned via benchmarks.

### Steering Behaviors (M1)

Craig Reynolds behaviors producing acceleration vectors:

| Behavior | Purpose |
|----------|---------|
| **Seek** | Steer toward target position |
| **Arrive** | Seek with deceleration near target (slowing radius) |
| **Separation** | Push away from nearby entities to prevent overlap |
| **Obstacle Avoidance** | Steer around structure entities (walls, buildings) |
| **Cohesion** | Steer toward center of nearby group (formation use) |
| **Alignment** | Match velocity of nearby group (formation use) |

#### Force Combination: Priority Tiers with Weighted Sum

Behaviors are assigned to priority tiers. Within a tier, forces are weighted-summed. The
highest-priority tier that produces nonzero output wins — lower tiers are discarded.

| Priority | Behaviors | Rationale |
|----------|-----------|-----------|
| 1 (highest) | Obstacle Avoidance | Never walk through walls |
| 2 | Separation | Don't overlap other entities |
| 3 | Seek/Arrive, Cohesion, Alignment | Navigation and formation keeping |

Tier weights are tunable. Start with equal weights within a tier, adjust based on observed
simulation behavior.

#### Velocity Integration

```
accel = combined_steering_force (clamped to steering_force magnitude)
vel += accel * dt
speed = vel.length()
if speed > derived_speed:
    vel = vel.normalize() * derived_speed
pos += vel * dt
```

#### Damping Model (Derived from Entity Type)

Two models, derived from what the entity is — no flag to configure:

- **Legged entities** (humans, animals): Inertia-based. Maintain velocity when no steering
  force applied. Cannot instantly stop or change direction. Deceleration requires active
  opposing steering force. A sprinting soldier must actively brake.

- **Wheeled/pulled entities** (carts, wagons): Friction-based. Passively decelerate when
  driving force removed. A cart stops rolling when oxen stop pulling. Damping coefficient
  per vehicle type.

- **Projectiles**: Ballistic (gravity only). Handled by D domain, not M.

Derivation: entities with legs (Person component with bipedal/quadrupedal physiology) use
inertia. Entities that are vehicles or pulled loads use friction.

### Speed Modifiers (M1)

All multiplicative. Independent physical constraints compound:

| Factor | Source | Formula |
|--------|--------|---------|
| **Slope** | `Heightfield::slope_at(pos, direction)` | `1.0 - slope_penalty * gradient` |
| **Surface friction** | `GeoMaterial::friction()` at nearest vertices | `1.0 / friction` (Rock 0.9 = fast, Sand 1.3 = slow) |
| **Road bonus** | Road entities in spatial index at current hex | `1.0 + road_bonus[level]` (capped at 1.0 as a factor) |
| **Encumbrance** | Total equipment weight vs entity strength | `1.0 - (weight / max_carry)` clamped to [0.0, 1.0] |
| **Wounds** | Leg wound severity from D domain | Laceration: ~0.5×. Fracture: 0.0 (immobile) |
| **Stamina** | Current stamina from D domain | Scales max sprint capability. Low stamina = slower max speed |

Stamina interaction: steering force determines effort level. Higher force = sprint = more
stamina drain. At low stamina, derived speed decreases, so even full steering force produces
less actual speed. Natural feedback loop — exhausted entities slow down.

### Pathfinding (M2)

**A* on hex graph.** Reuse V2's proven approach, adapted for V3 spatial primitives.

- **Input**: Source hex, destination hex, faction ID (for fog of war)
- **Graph**: Hex adjacency. Edge cost from terrain (slope, surface, roads).
- **Fog of war**: Pathfinding queries the faction's revealed map. Unknown hexes treated as
  passable (optimistic). If entity encounters unrevealed impassable terrain, movement stops
  and signals upward.
- **Output**: Sequence of hex centers as Vec3 waypoints.

**Path smoothing (string-pulling):** Iterate waypoints. For each triplet (N-1, N, N+1),
check if a straight line from N-1 to N+1 avoids impassable terrain (ray test against hex
boundaries). If clear, remove waypoint N. Repeat until stable.

**Caching:** A* results cached with moka, keyed on `(source_hex, dest_hex, faction_id)`.
Invalidated when terrain changes within the path's bounding box or when the faction's fog
of war reveals new information affecting the path.

**Path invalidation:** Movement stops and signals "path blocked." The agent layer decides
whether to re-path, wait, or request new orders. Movement does not autonomously re-route.

**Scaling note:** Current implementation targets existing map sizes (~30×30). For 100k+
tile maps, hierarchical pathfinding and flow fields (Dijkstra fill with per-hex direction
vectors) will be needed. Deferred to V3.2.

### Formation Movement (M2)

Three formation types for V1:

| Type | Shape | Use Case |
|------|-------|----------|
| **Column** | Single-file or narrow column | Marching, road travel |
| **Line** | Wide front, shallow depth | Battle line, maximizing frontage |
| **Wedge** | V-shape, leader at point | Assault, breaking through |

Deferred: Square, Skirmish.

#### Algorithm

1. Compute stack center-of-mass and facing direction.
2. For the formation type, compute target slot offsets relative to center, rotated by facing.
3. Each entity steers toward its assigned slot using Seek/Arrive.
4. Cohesion and Alignment behaviors keep the group coherent during movement.

**Obstacle handling:** Formation deforms around obstacles — entities route individually to
the far side, then reform into formation slots. The formation system temporarily relaxes
slot enforcement while entities are navigating around the obstacle.

**Rotation:** When stack facing changes, all slot offsets rotate instantly. Entities steer
to new positions. The visual effect is entities smoothly redistributing — the formation
"pivots" as entities catch up via steering.

**Slot computation** is a pure function: `(formation_type, entity_count, facing) → Vec<Vec2>`
offsets. No mutable state in the formation system itself.

## Scope

### V1 (ship this)

**M1 — Wave 1:**
- Steering behaviors: Seek, Arrive, Separation, Obstacle Avoidance, Cohesion, Alignment
- Priority-tier force combination with weighted sum within tiers
- Velocity integration with dt parameter
- Derived speed from base capability × modifier stack (all six factors)
- Damping model: inertia for legged entities, friction for vehicles (derived from entity type)
- Hex boundary hysteresis (using S domain's `update_hex_membership`)

**M2 — Wave 2:**
- A* pathfinding on hex graph with faction fog of war
- Path smoothing via string-pulling
- A* result caching with moka
- Formation movement: Column, Line, Wedge
- Formation slot computation, obstacle deformation, rotation

### Deferred

| Item | Why | When |
|------|-----|------|
| Flow fields | Not needed at current map sizes | V3.2 (100k+ tiles) |
| Hierarchical pathfinding | Same | V3.2 |
| Square formation | Not needed for V1 scenarios | Post-V3.0 |
| Skirmish formation | Same | Post-V3.0 |
| FlowFieldFollow steering | Paired with flow fields | V3.2 |
| Autonomous re-routing | Agent layer responsibility, not movement | Never in M domain |
| Air/sea movement | Out of scope for ground movement | Separate domain |

## Verification

All M domain verification is unit tests. Integration/visual verification deferred to E2.

**M1 acceptance criteria:**
- Entity given a waypoint reaches it without oscillation
- Entity decelerates smoothly near target (Arrive behavior)
- Two entities approaching each other maintain separation (no overlap)
- Entity steers around a structure entity (obstacle avoidance)
- Legged entity maintains velocity when steering force removed (inertia)
- Wheeled entity decelerates when driving force removed (friction)
- Speed modifiers compound correctly: wounded + encumbered + uphill < healthy + light + flat
- Derived speed is zero when leg fracture wound is present

**M2 acceptance criteria:**
- A* produces valid path between two hexes on a map with obstacles
- String-pulling reduces waypoint count (straight-line paths have minimal waypoints)
- Cached paths are returned for identical (source, dest, faction) queries
- Cache invalidates when terrain changes within path bounding box
- Column formation: entities line up behind leader
- Line formation: entities spread across facing direction
- Wedge formation: entities form V-shape behind point entity
- Formation rotation: changing facing produces correct new slot positions

## Files Modified

New files:
- `crates/engine/src/v3/steering.rs` — Steering behaviors, force combination, velocity integration
- `crates/engine/src/v3/pathfinding.rs` — A* on hex graph, path smoothing, caching
- `crates/engine/src/v3/formation.rs` — Formation types, slot computation, rotation
- `crates/engine/src/v3/movement.rs` — Top-level movement system: speed derivation, damping, tick integration

Modified files:
- `crates/engine/src/v3/mod.rs` — Add module exports for steering, pathfinding, formation, movement
- `crates/engine/src/v3/spatial.rs` — May need minor additions if speed derivation needs new queries (unlikely given S domain completeness)

## Convention References

- `CLAUDE.md` — Workspace layout, testing patterns, commit conventions
- `docs/plans/v3-sequencing.md` — Wave dependencies (M1 needs S1+S2, M2 needs M1+S1)
- `docs/specs/v3-entity-unification-2026-04-13.md` — Authoritative spec for movement semantics

## Open Questions

None — all design questions from the original plan (`docs/plans/v3-M-movement.md`) are
resolved by this spec.
