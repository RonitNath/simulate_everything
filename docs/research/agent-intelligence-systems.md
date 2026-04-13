# Agent Intelligence Systems Research

Research compiled 2026-04-13 from game AI literature, StarCraft bot architectures,
Game AI Pro books, Paradox game internals, and robotics exploration algorithms.

## 1. Influence Maps

**What they are**: Parallel grids (same dimensions as game map) storing per-cell
floating-point influence values. Each player gets separate layers. Sources (units,
buildings) stamp their strength onto the map, then influence propagates outward
with decay.

**Data structure**: Flat `Vec<f32>` indexed by cell index. One layer per signal type
(friendly strength, enemy threat, resource value). Multiple layers compose by
addition or max.

**Propagation algorithm** (Game AI Pro 2, Ch. 30 — Dave Mark):

```
// Approach A: Distance falloff (most common in StarCraft bots)
for each source:
    cell = source.hex
    for neighbor in within_radius(cell, radius):
        dist = hex_distance(cell, neighbor)
        map[neighbor] += source.strength * decay^dist
// decay typically 0.5-0.7, radius 4-8 cells

// Approach B: Iterative blur
// Stamp unit strength at its cell, then N passes of neighbor averaging.
// Cheaper when unit count >> cell count.
```

**Resolution**: StarCraft bots (UAlbertaBot, Steamhammer) use coarse 8x8 or 16x16
grids. For our hex game at 30x30 (900 cells), one cell per hex is fine — no coarsening
needed.

**Update frequency**: Every 24-48 frames in SC bots (~1-2 sec). For our game, every
`AGENT_POLL_INTERVAL` (5 ticks) is natural.

**Tension map**: `friendly_influence - enemy_threat`. Positive = safe, negative =
contested/dangerous. Shows frontlines and flanking opportunities automatically.

**Key insight from SC bots**: Maintain TWO maps — one for own forces, one for enemy.
The difference tells you where you're strong/weak. UAlbertaBot uses separate maps
for ground/air threats.

Sources:
- Game AI Pro 2, Ch. 30: Modular Tactical Influence Maps (Dave Mark)
- Grant Gangi: Influence Maps Part 1 (grant.tuxinator.net)
- GameDev.net: Core Mechanics of Influence Mapping

## 2. Fog of War / Enemy Memory

**What SC bots actually do**: Store `last_seen_position`, `last_seen_frame`,
`last_seen_type` per enemy unit. Units not seen for >N frames get flagged
"possibly moved." Most BWAPI bots keep last-known position and accept staleness.

**Better approach (competition bots)**:

```rust
struct TrackedEnemy {
    last_pos: Axial,
    last_seen_tick: u64,
    strength: f32,
    unit_speed: f32,  // estimated max speed
}

fn confidence_radius(enemy: &TrackedEnemy, now: u64) -> f32 {
    let elapsed = (now - enemy.last_seen_tick) as f32;
    enemy.unit_speed * elapsed  // max possible displacement
}

fn estimated_strength(enemy: &TrackedEnemy, now: u64) -> f32 {
    let elapsed = (now - enemy.last_seen_tick) as f32;
    enemy.strength * 0.95_f32.powf(elapsed)  // decay confidence
}
```

When evaluating a region's threat: if enemy *could* be here (distance < confidence_radius),
add `threat * (1.0 - distance / confidence_radius)`.

**Nobody uses full Bayesian inference in practice.** Gaussian spread at unit max speed
is the practical ceiling. Simple decay + position extrapolation covers 90% of cases.

## 3. Blackboard Architecture

**Minimal viable blackboard** (GameDev.net squad AI tutorial): A typed struct that
agent subsystems read/write each tick.

```rust
struct Blackboard {
    threats: Vec<(Axial, f32, u64)>,     // (pos, strength, tick_seen)
    scouting_targets: Vec<Axial>,
    engagement_decisions: Vec<EngageOrder>,
    economy_priority: f32,               // 0=military, 1=economy
    frontier: HashSet<usize>,            // unscouted boundary cells
    scouted_pct: f32,
}
```

**Lifecycle per tick**:
1. **Perception**: update influence maps, write threats/scouting targets
2. **Evaluation**: each behavior scores proposed actions
3. **Arbitration**: highest-priority actions win, emit directives

No behavior tree or GOAP planner needed. The blackboard is just a struct passed by
`&mut` — zero allocation per tick.

## 4. Force Evaluation / Engagement Decisions

**Lanchester's Square Law**: Combat power scales with N^2 when all units can fire
simultaneously. For N friendly vs M enemy of equal strength, friendly wins if N > M,
surviving force = sqrt(N^2 - M^2).

**Practical engagement formula** (StarCraft bots):

```rust
fn should_engage(my_units: &[Unit], enemy_units: &[Unit]) -> bool {
    let my_power: f32 = my_units.iter()
        .map(|u| u.strength * u.strength)  // square law
        .sum();
    let enemy_power: f32 = enemy_units.iter()
        .map(|u| u.strength * u.strength)
        .sum();
    my_power > enemy_power * 1.3  // 1.3x threshold common
}
```

**Thresholds used in practice**:
- Most SC bots: 1.2x-1.5x advantage to engage
- Steamhammer: ~1.2x
- The square law means 1.4:1 numerical advantage ~ 2:1 combat power
- Terrain: multiply defender power by 1.2-1.5 for chokepoints

**Distance weighting**: Units 3 hexes away contribute less than adjacent:
`weight = 1.0 / (1.0 + distance)`. Units farther away take turns to arrive.

**Key detail**: DPS * HP (or strength * strength for Lanchester) is the standard
"military value" heuristic.

Source: Synnaeve & Bessiere, "Lanchester Laws for StarCraft Combat Prediction" (AIIDE 2012)

## 5. Frontier-Based Exploration

**Origin**: Yamauchi 1997 (robotics). Frontier = boundary between scouted and
unscouted territory. A frontier hex is scouted and adjacent to at least one
unscouted hex.

**Incremental maintenance** (critical for performance):

```
on_hex_revealed(hex):
    for neighbor in hex_neighbors(hex):
        if is_scouted(neighbor) and has_unscouted_neighbor(neighbor):
            frontier.insert(neighbor)
        else:
            frontier.remove(neighbor)
    if has_unscouted_neighbor(hex):
        frontier.insert(hex)
```

O(k) per reveal where k = hexes revealed by one unit move (typically 7-19 for
radius-5 vision). No full scan needed.

**Frontier clustering**: Group contiguous frontier hexes into regions using flood
fill or union-find. Assign scouts to regions, not individual hexes.

**Priority scoring**:

```
score(frontier_cell) =
    w1 * information_gain        // count of fog hexes within vision radius
    + w2 * threat_proximity      // distance to known/predicted enemy positions
    + w3 * resource_likelihood   // terrain-based prior for good settlement sites
    + w4 * staleness             // current_tick - last_seen[cell]
    - w5 * travel_cost           // hex distance from nearest available scout
```

**Multi-unit coordination**: Greedy assignment with exclusion — sort frontier regions
by score, assign nearest scout to highest-scored region, remove from pool. O(n*m)
for n scouts and m regions. StarCraft bots use a blackboard "claimed targets"
variant.

## 6. Decision Traces / Explainability

**Paradox approach** (EU4/EU5/CK3/Stellaris): `ai_will_do` weighted decision tables.
Each AI decision has a base weight multiplied by conditional factors. Debug tooltip
shows final weight and which modifiers applied. Gold standard for strategy game AI
debugging.

**Minimal recording format**:

```rust
struct DecisionTrace {
    tick: u64,
    unit_id: u32,
    action: TracedAction,          // enum: Move, Engage, Disengage, Scout, Hold
    reason: &'static str,          // "force_ratio_favorable", "scouting_frontier"
    scores: SmallVec<[(&'static str, f32); 4]>,  // top factors
}
```

**Key design**: Use `&'static str` for reason tags — zero allocation. Store in a ring
buffer (last N decisions). Dump on demand for debugging.

**Runtime cost**: One struct copy per unit per poll — negligible. Ring buffer caps memory.

**"Record the winner, not all candidates"** — both Paradox and Civilization use this
approach in production. Full candidate logging only under a compile-time debug flag.

## 7. Hex Grid Spatial Queries

For <100 units, **brute force is fine**:

```rust
fn units_within(center: Axial, range: i32, units: &[Unit]) -> Vec<&Unit> {
    units.iter().filter(|u| hex_distance(center, u.pos) <= range).collect()
}
```

~100 comparisons per query — trivially fast.

For >500 units: `HashMap<Axial, Vec<UnitId>>` spatial hash — O(1) per cell,
iterate ~19 hexes in radius-2 ring. Standard approach in hex games.

Our game has <100 units typically. Brute force is correct.
