# V2 Agent Architecture: "Centurion"

> **Design target, not current state.** The current V2 agent is `SpreadAgent` — a single-pass heuristic placeholder (see `crates/engine/src/v2/agent.rs`). This document describes the Centurion architecture that will replace it. The agent architecture will need further revision once V2's remaining systems (convoys, population, roads — see roadmap.md) are implemented, as they significantly expand the agent's decision space.

## Performance Target

The agent must be fast enough that a benchmark harness can run:
- **1,000 games** x **10,000 ticks** x **100,000 hex map** (≈316x316)
- Each game: 2 agents, each polled every 5 ticks = 2,000 agent calls per game
- Total: 2,000,000 agent calls across the harness
- **Target: complete harness in < 30 minutes** on a 12-core machine
- That's ~900μs per agent call average, including observation construction

This rules out anything O(n²) per tick where n = hex count. Every layer must be O(units) or O(units × log(hexes)), never O(hexes²).

---

## Architecture Overview

Three layers, running at different frequencies, communicating through shared state:

```
┌─────────────────────────────────────────┐
│             Strategic Layer              │
│  Runs every ~50 ticks                    │
│  Produces: Posture, RegionPriorities     │
│  Cost budget: ~200μs                     │
├─────────────────────────────────────────┤
│            Operational Layer             │
│  Runs every agent poll (5 ticks)         │
│  Produces: UnitAssignments, Directives   │
│  Cost budget: ~500μs                     │
├─────────────────────────────────────────┤
│             Tactical Layer               │
│  Runs every agent poll, only for         │
│  units near enemies                      │
│  Produces: Engage/Disengage Directives   │
│  Cost budget: ~200μs                     │
└─────────────────────────────────────────┘
```

Total per-poll budget: ~700μs typical (strategic layer amortized across 10 polls).

### Shared state between layers

```rust
struct AgentState {
    // === Updated by Strategic Layer ===
    posture: Posture,                    // Expand, Defend, Attack, Raid
    region_priorities: Vec<RegionScore>, // sorted by priority, top-N only
    threat_direction: Option<HexDir>,    // general direction of known enemy
    
    // === Updated by Operational Layer ===
    unit_roles: HashMap<u32, Role>,      // unit_id → assigned role
    
    // === Precomputed spatial data (updated lazily) ===
    terrain_value_cache: Vec<f32>,       // per-hex, static after game start
    influence_map: Vec<f32>,             // per-hex, own influence (recomputed periodically)
    enemy_influence: Vec<f32>,           // per-hex, enemy influence from visible units
    frontier_hexes: Vec<HexCoord>,       // boundary between explored and unexplored
    
    // === Bookkeeping ===
    last_strategic_tick: u64,
    known_enemy_positions: Vec<(u32, HexCoord, f32, u64)>,  // (id, pos, strength, last_seen_tick)
}
```

The key principle: **layers communicate through shared state, not message passing.** The strategic layer writes `posture` and `region_priorities`. The operational layer reads those and writes `unit_roles`. The tactical layer reads `unit_roles` and the observation to produce directives. No queues, no async, no allocation — just struct field updates.

---

## Layer 1: Strategic

### When it runs
Every 50 ticks (5 seconds game time at 10 ticks/sec). Also triggered immediately when:
- First enemy unit becomes visible
- Own general is under threat (enemy within 5 hexes)
- Significant territory loss (>20% of units killed since last evaluation)

### What it computes

**1. Strategic posture** (~50μs)

```rust
enum Posture {
    Expand,  // no enemy visible, grow economy
    Contest, // enemy visible, competing for territory
    Attack,  // strength advantage, press forward
    Defend,  // strength disadvantage, consolidate
}
```

Decision logic:
```
if no enemy units visible:
    posture = Expand
else:
    own_strength = sum of own unit strengths
    enemy_strength = sum of visible enemy strengths (extrapolate for fog)
    ratio = own_strength / enemy_strength
    if ratio > 1.5: posture = Attack
    elif ratio > 0.8: posture = Contest
    else: posture = Defend
```

Fog extrapolation: if we see 10 enemy units but estimate (from resource math) they could have 20, use 20 for the ratio. Estimate based on: `enemy_estimated_units = (game_tick * estimated_enemy_income) / UNIT_COST`. This is rough but prevents overconfidence.

**2. Region priorities** (~100μs)

Divide the map into regions (e.g., 10x10 hex blocks). For each region, compute a priority score:

```
score = terrain_value_sum * terrain_weight
      + enemy_presence * threat_weight  
      + frontier_proximity * explore_weight
      + general_proximity * defense_weight
```

Weights shift based on posture:
- Expand: terrain_weight=3, explore_weight=2, threat_weight=0, defense_weight=1
- Contest: terrain_weight=2, explore_weight=1, threat_weight=2, defense_weight=1
- Attack: terrain_weight=1, explore_weight=0, threat_weight=3, defense_weight=1
- Defend: terrain_weight=1, explore_weight=0, threat_weight=1, defense_weight=3

Keep only top-5 regions. This is what the operational layer uses to decide where to send units.

**3. Influence maps** (~50μs, amortized)

Two influence maps, each a `Vec<f32>` the size of the hex grid:

- **Own influence**: for each own unit, add `strength / (1 + distance)` to hexes within radius 5. This decays with distance. Computed incrementally — when units move, subtract old contribution, add new.
- **Enemy influence**: same, but for visible enemy units. Stale entries (last seen > 100 ticks ago) decay.

These are used by the operational layer to identify: where are we strong? Where are we weak? Where is the frontier?

**Optimization**: influence maps don't recompute from scratch. They're maintained incrementally. When a unit moves from hex A to hex B, subtract its influence from A's neighborhood and add to B's. Cost: O(units_moved × radius²) per tick, not O(units × hexes).

---

## Layer 2: Operational

### When it runs
Every agent poll (every 5 ticks).

### What it computes

**1. Task generation and unit assignment** (~150μs)

Instead of fixed role ratios, generate a ranked list of **tasks** (concrete objectives) and assign units to them via greedy matching. This adapts naturally to map topology and game state.

```rust
enum Task {
    Produce { hex: HexCoord, terrain_value: f32 },  // sit on hex, generate resources
    Claim { hex: HexCoord, terrain_value: f32 },     // move to unclaimed high-value hex
    Fight { target: u32, position: HexCoord },        // move toward enemy unit
    Scout { hex: HexCoord },                          // explore fog boundary
    Guard { hex: HexCoord },                          // defend near general
}

struct ScoredTask {
    task: Task,
    priority: f32,  // strategic value of completing this task
}
```

**Task generation** (from strategic layer outputs):

```
For each unoccupied high-value hex within frontier:
    emit Claim { hex, terrain_value } with priority = terrain_value * posture.expand_weight

For each own unit on a high-value hex (already producing):
    emit Produce { hex, terrain_value } with priority = terrain_value * posture.economy_weight
    (this "task" keeps the unit in place — it's already assigned)

For each visible enemy unit:
    emit Fight { target, position } with priority = threat_level * posture.fight_weight

For each fog boundary hex near high-priority regions:
    emit Scout { hex } with priority = region_priority * posture.scout_weight

Emit Guard tasks for 2-3 hexes around the general:
    priority = posture.defense_weight * 5.0
```

**Greedy matching** (market-based assignment):

```
Sort tasks by priority (descending)
For each task in priority order:
    Find the unassigned unit with lowest cost to accomplish this task
    cost = hex_distance(unit.position, task.target) + role_switch_penalty
    Assign unit to task
    (If no unassigned unit within reasonable distance, skip task)
```

Cost: O(tasks × units). With ~30 tasks and ~200 units, that's 6000 comparisons — trivial.

**Stability**: assignments are sticky. A unit keeps its task unless:
- The task is completed (hex claimed, enemy killed)
- A higher-priority task appears that this unit is uniquely suited for
- The unit dies
- Posture changes (triggers full reassignment)

Between reassignments, only new/completed tasks cause updates. Most polls: 0-5 reassignments.

**Posture weights** (tuning knobs that shift task priorities):

| Posture | expand_weight | economy_weight | fight_weight | scout_weight | defense_weight |
|---------|---------------|----------------|--------------|--------------|----------------|
| Expand  | 3.0           | 2.0            | 0.5          | 2.0          | 1.0            |
| Contest | 1.5           | 2.0            | 2.5          | 1.0          | 1.0            |
| Attack  | 0.5           | 1.5            | 3.0          | 0.5          | 1.5            |
| Defend  | 0.5           | 2.0            | 2.0          | 0.5          | 3.0            |

**2. Destination computation** (~200μs)

For each unit needing a new destination (no current destination, or role changed):

- **Producer**: find the highest terrain_value hex within 10 hexes that isn't occupied by another own unit. Use the terrain_value_cache (sorted list of hexes by value, precomputed once).
- **Expander**: find the highest-value hex on the frontier (boundary of explored territory). Frontier hexes are tracked incrementally — when vision expands, add new frontier hexes, remove interior ones.
- **Fighter**: move toward the nearest visible enemy unit, or toward the highest-priority region if no enemy is visible.
- **Scout**: move toward the nearest fog boundary hex, preferring directions with unexplored high-priority regions.
- **Guard**: move toward a hex within 3 of the general, preferring hexes that are between the general and the threat direction.

**Pathfinding optimization**: don't pathfind every unit every poll. Only pathfind when a unit needs a new destination. The simulation's built-in pathfinding handles execution — the agent just sets the destination. Cost: O(pathfinds × hex_count) per poll, but pathfinds are bounded by role changes, not unit count. Typically 5-15 pathfinds per poll.

**3. Production decisions** (~10μs)

```
if resources >= UNIT_COST:
    if total_units < desired_count(posture, income_rate):
        emit Directive::Produce
```

`desired_count` is simple: `min(income_rate / upkeep_per_unit, max_useful_units)`. Don't produce units you can't feed. Don't produce if you have more units than the map can meaningfully deploy.

**4. Emit movement directives** (~100μs)

For each unit whose destination changed since last poll, emit `Directive::Move { unit_id, q, r }`. Skip units that are already moving toward the right destination.

---

## Layer 3: Tactical

### When it runs
Every agent poll, but only processes units that have visible enemies within 4 hexes. This keeps the cost proportional to combat activity, not total unit count.

### What it computes

**1. Threat assessment** (~50μs per combat zone)

For each own unit adjacent to an enemy:

```rust
struct ThreatAssessment {
    own_unit: u32,
    own_strength: f32,
    enemy_unit: u32,
    enemy_strength: f32,
    enemy_engaged_edges: u8,     // how many edges is the enemy already engaged on
    own_reinforcement_eta: u8,   // ticks until nearest own reinforcement arrives
    enemy_reinforcement_eta: u8, // ticks until nearest visible enemy reinforcement
}
```

**2. Engagement decisions (simulation-based)**

Instead of simple strength ratio thresholds, simulate the proposed engagement forward ~50 ticks using the actual damage formula. This is cheap (~0.1ms per simulation) and accounts for multi-edge effectiveness, reinforcement timing, and strength decay dynamics.

```rust
fn should_engage(own: &Unit, enemy: &Unit, state: &AgentState) -> bool {
    // Simulate 50 ticks of combat with current edge counts
    let own_edges_after = own.engagements.len() + 1;
    let enemy_edges_after = enemy.engagements.len() + 1;
    
    let mut own_hp = own.strength;
    let mut enemy_hp = enemy.strength;
    
    for tick in 0..50 {
        let own_eff = 1.0 / (own_edges_after as f32).sqrt();
        let enemy_eff = 1.0 / (enemy_edges_after as f32).sqrt();
        
        own_hp -= enemy_hp * DAMAGE_RATE;       // enemy deals full damage to us
        enemy_hp -= own_hp * DAMAGE_RATE * own_eff; // we deal effectiveness-scaled damage
        // (simplified — full sim accounts for all concurrent engagements on both units)
        
        if own_hp <= 0.0 || enemy_hp <= 0.0 { break; }
    }
    
    // Engage if we're projected to survive and deal more total damage than we take
    let own_damage_taken = own.strength - own_hp.max(0.0);
    let enemy_damage_taken = enemy.strength - enemy_hp.max(0.0);
    enemy_damage_taken > own_damage_taken * 1.2  // 20% margin for safety
}
```

Additional heuristics layered on top of simulation:

```
if enemy is already engaged on N edges:
    # Flanking — almost always favorable because their effectiveness drops
    # Simulation will confirm, but fast-path: if N >= 2, engage unless we're below 30 strength

if own_reinforcement_eta < enemy_reinforcement_eta:
    # We'll get help first — bias toward engaging even at slight disadvantage

if enemy.is_general:
    # High-value target — accept worse odds (enemy_damage > own_damage * 0.8)
```

**3. Flanking coordination**

When the tactical layer identifies an enemy unit engaged on 1 edge, it checks if any own units are adjacent on other edges. If so, it prioritizes engaging from a second edge — the effectiveness drop from 100% to 71% on the enemy makes flanking almost always worth it, even with a weaker unit.

This doesn't require explicit coordination messages between units. The tactical layer sees all own units and all visible enemies in a single pass. It can identify flanking opportunities by iterating: "for each engaged enemy, which of my free units are adjacent on unengaged edges?"

**4. Disengagement decisions**

```
if own_strength < 30 and enemy_engaged_edges < 3:
    → DisengageAll (50% of 30 = 15, limp away, live to fight another day)
    
if own_strength < 10:
    → Don't disengage (50% of 10 = 5, not worth surviving at that strength)
    → Hold and die, deal remaining damage
    
if engaged on 3+ edges:
    → Cannot disengage (surrounded, fight to the death)
```

---

## Spatial Data Structures

### Hex grid spatial index

For queries like "find all units within N hexes of position P", maintain a spatial hash:

```rust
struct SpatialIndex {
    // bucket_size = 8 hexes. Each bucket maps to a Vec of unit IDs.
    buckets: HashMap<(i32, i32), Vec<u32>>,
    bucket_size: i32,
}

impl SpatialIndex {
    fn units_near(&self, q: i32, r: i32, radius: i32) -> impl Iterator<Item = u32> {
        // Check buckets that could contain units within radius
        let bq = q / self.bucket_size;
        let br = r / self.bucket_size;
        let bucket_radius = (radius / self.bucket_size) + 1;
        // iterate neighboring buckets, filter by actual hex distance
    }
}
```

This makes "find nearby units" O(1) amortized instead of O(total_units). Updated incrementally when units move.

### Terrain value cache

Precomputed once at game start:
```rust
struct TerrainCache {
    // Sorted list of (hex_coord, terrain_value), descending by value
    hexes_by_value: Vec<(HexCoord, f32)>,
    
    // Per-region (10x10 block) aggregate terrain value
    region_values: Vec<f32>,
    region_width: usize,
}
```

The agent never scans the full hex grid. It uses `hexes_by_value` to find good spots and `region_values` for strategic evaluation.

### Frontier tracking

```rust
struct FrontierTracker {
    // Set of hexes that are at the boundary of explored territory
    frontier: HashSet<HexCoord>,
    // Set of hexes that have been explored (visible at some point)
    explored: BitVec,
}
```

Updated incrementally each poll: when new hexes become visible, check if they or their neighbors are frontier hexes. Remove interior hexes from frontier. Cost: O(newly_visible_hexes) per poll.

---

## Message Flow (Per Agent Poll)

```
1. Receive Observation from engine
   
2. Update spatial data structures:                    ~50μs
   - Update spatial index for moved units
   - Update frontier tracker from visibility
   - Update known_enemy_positions
   
3. IF tick - last_strategic_tick >= 50:               ~200μs (amortized to ~20μs/poll)
   - Run Strategic Layer
   - Update posture, region_priorities
   - Recompute influence maps (incremental)
   - last_strategic_tick = tick
   
4. Run Operational Layer:                             ~400μs
   - Role assignment (if posture changed)
   - Destination computation (for units needing new dest)
   - Production decision
   - Emit Move directives
   
5. Run Tactical Layer:                                ~200μs
   - For each own unit with enemies within 4 hexes:
     - Threat assessment
     - Engage/disengage/hold decision
   - Emit Engage/Disengage directives
   
6. Return Vec<Directive> to engine

Total: ~650μs typical, ~850μs when strategic layer runs
```

---

## Optimization Strategies

### Amortized computation
- Strategic layer runs 1/10th as often as operational
- Influence maps update incrementally (delta from unit movement), full recompute only on major events
- Role assignment is stable — only reassign on posture change or unit death
- Pathfinding only for units that need new destinations (not all units every poll)

### Avoiding allocation
- Reuse `Vec<Directive>` across polls (clear + push, don't allocate)
- Influence maps are persistent `Vec<f32>`, updated in-place
- Spatial index uses pre-allocated bucket `Vec`s
- `AgentState` is a single struct, no Box/Arc/dynamic dispatch

### Spatial locality
- Process units in spatial order (by bucket) to maximize cache hits when reading hex grid data
- Terrain cache is a contiguous `Vec<f32>`, accessed by hex index
- Influence maps are contiguous `Vec<f32>`, same layout as terrain

### Early termination
- Tactical layer only activates for units near enemies (skip 80-90% of units in most ticks)
- Operational layer skips units that haven't changed state since last poll
- Strategic layer skips if nothing material changed (no new enemies, no significant unit loss)

### Parallelism (harness level)
- Individual agent calls are single-threaded (fast enough, avoids coordination overhead)
- The **harness** parallelizes across games: 1,000 games across 12 cores = ~83 games per core
- Each game is independent — no shared state between games
- Expected harness time: 1000 games × 10000 ticks × 2ms/tick ÷ 12 cores ≈ 28 minutes

---

## Benchmarking Plan

### Micro-benchmarks (per-layer)
```
strategic_layer_bench:   1000 calls on 100k hex map → report p50, p99 latency
operational_layer_bench: 1000 calls with 200 units  → report p50, p99 latency
tactical_layer_bench:    1000 calls with 20 combat pairs → report p50, p99 latency
full_agent_bench:        1000 polls on live game state → report p50, p99 latency
```

### Macro-benchmarks (game-level)
```
single_game_bench:  1 game, 10k ticks, 100k map, 2 agents → total wall time, ticks/sec
harness_bench:      100 games, 10k ticks, 100k map → total wall time, games/sec
scaling_bench:      vary map size (10k, 50k, 100k, 500k) → agent latency vs map size
unit_scaling_bench: vary unit count (50, 200, 500, 1000) → agent latency vs unit count
```

### Profiling targets
- Agent call latency must be < 1ms at p99 on 100k hex map with 200 units
- No per-poll heap allocation (after warmup)
- Influence map update must be < 100μs for 20 unit movements
- Pathfinding must be < 50μs per query on 100k hex map (BFS with early exit)

---

## Research-Informed Design Notes

These design notes draw on common patterns from competitive programming game AIs and RTS bot architectures:

**Combat simulation beats threshold heuristics.** Fast forward simulation is often dramatically better than fixed engage/disengage thresholds. Our tactical layer follows that pattern by simulating 50 ticks of the proposed engagement using the actual damage formula.

**Market-based assignment beats fixed ratios.** Shared-state coordination lets units flow toward the most-needed role instead of rigid quotas. Our task generation + greedy matching uses that same idea in a centralized form. Fixed percentage ratios (50% producers, 30% expanders) do not adapt to map topology — a map with one rich valley and lots of wasteland needs very different allocation than a uniform map.

**Influence maps are a broadly useful strategic representation.** Scalar fields remain one of the most practical ways to summarize territorial pressure. Incremental update (delta when units move) is essential for performance. Our dual influence map (own + enemy) uses that pressure-map pattern directly.

**Amortization over time is the key performance technique.** Strong agents make careful use of their compute budget. Strategic layer running 1/10th as often, pathfinding only on destination change, role assignment only on posture change. This is not a shortcut; it is the right architecture for this scale.

**Spatial decomposition is non-negotiable at scale.** Spatial indexing becomes mandatory once unit counts rise. Our bucket-based spatial hash (bucket size = 8 hexes) is a standard fit for uniform grids with fixed interaction radius.

**The foveated attention model works.** Process the full map at low resolution (region summaries), then spend detailed work on hotspots. Our tactical layer only activates at full fidelity for units near enemies, which follows that pattern directly.

---

## Evolution Path

### Centurion v1 (V2 launch)
- Fixed role ratios per posture
- Simple engagement heuristic (strength ratio threshold)
- BFS pathfinding
- No memory of enemy behavior

### Centurion v2
- Adaptive role ratios (learn from win/loss what ratios work)
- Engagement considers reinforcement timing
- Coordinated flanking (identify 2-unit flanking opportunities)
- Enemy general search pattern (systematic fog exploration)

### Centurion v3
- Forward simulation (simulate "what if I engage here?" for 50 ticks)
- Territory value projection (predict where enemy will expand based on terrain)
- Feints (move units toward one region, then redirect — exploits enemy reaction time)
- Economic disruption (target enemy units on high-value hexes specifically)

### Future: LLM-augmented agent
- Use the ASCII observation format to feed game state to an LLM
- LLM provides strategic-layer decisions (posture, priorities)
- Operational and tactical layers remain algorithmic (too latency-sensitive for LLM)
- The LLM "general" issues orders; the code "officers" execute them
