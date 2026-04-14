# V3 Domain: M — Movement

Source spec: `docs/specs/v3-entity-unification-2026-04-13.md` (revision 2, Movement section)
Sequencing: `docs/plans/v3-sequencing.md`

## Purpose

Replace V2's hex-to-hex teleportation with continuous steering-based movement.
Entities accelerate, decelerate, and steer through continuous space. The hex grid
provides waypoints via A* pathfinding; steering behaviors provide smooth traversal.

## Design Questions

### M.1 Steering Behaviors

- Which Reynolds behaviors to implement for V3.0? Minimum viable set: Seek,
  Arrive, Separation, Obstacle Avoidance. Cohesion and Alignment for formation
  movement. FlowFieldFollow for mass movement (V3.2?).
- Steering force combination: weighted sum, priority-based, or context steering?
  Weighted sum is simplest but can produce contradictory forces. Priority (highest
  priority behavior that returns nonzero wins) is more predictable. What's right
  for a military simulation?
- Max steering force and max speed: per-entity or per-type? The spec has
  `max_speed` and `steering_force` on Mobile. Should these be base values modified
  by terrain, encumbrance (armor weight), fatigue (stamina), wounds (leg injury)?
- Velocity damping: should entities decelerate when no steering force is applied,
  or maintain speed? Damping feels more realistic (friction) but requires tuning.

### M.2 Pathfinding

- A* on hex graph is proven (already in V2). The path produces a sequence of hex
  centers. How does path smoothing work? String-pulling on the hex center polyline:
  check if you can skip waypoint N by drawing a line from N-1 to N+1 that doesn't
  cross impassable terrain. Iterate until stable.
- Path invalidation: when does a path become stale? If terrain changes (wall
  built), if an entity blocks the path (formation in the way), if the destination
  changes. Recompute on demand or on a timer?
- Path caching: should computed paths be cached (same source hex + dest hex = same
  path)? With moka? Or is A* cheap enough at 30×30 that caching isn't worth it?

### M.3 Formation Movement

- A formation is a set of entities moving as a group. The operations layer assigns
  entities to a stack with a FormationType (Line, Column, Wedge, Square, Skirmish).
- Formation algorithm: given a stack center-of-mass, a facing direction, and a
  formation type, compute target offsets for each entity. Each entity steers
  toward its formation slot.
- What happens when a formation encounters an obstacle? The formation deforms
  (entities route around individually) then reforms on the other side? Or the
  formation stops and the operations layer must split it?
- Formation rotation: when the stack changes direction, all formation offsets
  rotate. Entities steer to new slots. How fast should this be? Instant rotation
  of offsets with entities catching up via steering?

### M.4 Speed Modifiers

- Terrain: the spec mentions slope penalty. Formula: `speed *= 1.0 - slope_factor * gradient`
  where gradient = height difference / hex distance. What values for slope_factor?
- Road bonus: roads exist from V2. `speed *= 1.0 + road_bonus[road_level]`.
  Do roads affect steering (entities prefer to stay on road) or just speed?
- Encumbrance: total weight of equipment reduces max_speed. Linear or threshold-based?
  A soldier in full plate is slower than one in leather. A person carrying cargo
  (pack animal or porter) is slower still.
- Fatigue: low stamina reduces max_speed? Or only affects steering_force (can't
  accelerate as hard but maintains cruising speed)?
- Wounds: leg wounds from the damage system reduce max_speed. Per-wound or binary
  (any leg wound = X% reduction)?

### M.5 Flow Fields (V3.2 but design now)

- Dijkstra fill on hex grid: each hex stores a Vec2 direction toward destination.
  Entity samples via barycentric interpolation of 3 nearest hex centers.
- When to use flow fields vs individual A*? Threshold: if > N entities share the
  same destination region, generate a flow field. Otherwise individual A*.
- Flow field invalidation: same triggers as path invalidation.

## Implementation Scope

| Item | Wave | Depends On | Deliverable |
|------|------|------------|-------------|
| M1 | 1 | S1, S2 | Steering behaviors, velocity integration, speed modifiers |
| M2 | 2 | M1, S1 | A* pathfinding on hex graph, path smoothing, formation movement |

## Key Files (Expected)

- `crates/engine/src/v3/steering.rs` — behavior implementations, force combination
- `crates/engine/src/v3/pathfinding.rs` — A* on hex, path smoothing, flow fields
- `crates/engine/src/v3/formation.rs` — formation types, slot computation, rotation

## Constraints

- Steering must produce stable movement at 1 tick/sec (dt=1.0). No oscillation.
- Formation movement must look coherent at mid zoom — entities in a stack should
  move together, not scatter.
- All velocity/position math uses dt parameter for multi-resolution compatibility.
