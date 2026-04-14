# Simulate Everything — Version Roadmap

## Vision

An AI-native real-time strategy simulation where emergent behavior produces historically plausible warfare. Two (or more) AI civilizations expand, build infrastructure, raise armies, and fight — not because someone coded specific strategies, but because the simulation rewards the same patterns that won real wars. The spectator watches Rome vs Carthage played out by agents that discover why those strategies work.

The game is the simulation. The agents are the players. The fun is watching emergent strategy arise from honest mechanics.

---

## V2 — Foundation

**Goal**: Playable hex-based real-time strategy with physical economy. Two AI nations expand, build roads, raise armies, fight over contested regions. Spectator experience is watchable and strategically legible.

### Built (phases 1-7 complete)

The V2 engine is implemented and running. See `v2-engine-spec.md` for full details.

**Hex grid**
- Axial coordinates (q, r), flat-top orientation, even-r offset storage
- 6 equidistant neighbors, uniform distance, no diagonal exploits

**Continuous tick simulation**
- 10hz default tick rate
- Units have movement cooldowns, not turns
- Web server controls playback speed (real-time, accelerated, as-fast-as-possible)

**Discrete unit entities**
- Units are companies with strength 100→0, not anonymous integers on cells
- General is a unit — killable, protectable

**Edge-based engagement combat**
- Engagements tracked per hex-edge (0-5)
- Effectiveness = 1/sqrt(N engaged edges) — flanking and encirclement emerge naturally
- 3+ edges engaged = surrounded, cannot disengage
- Disengagement costs 30% strength
- No auto-combat: agents explicitly order engagements

**Single-resource economy (placeholder)**
- Stationary units on terrain generate resources proportional to terrain_value
- Resources credited directly to player pool (teleportation — replaced by convoys below)
- 10 resources = produce 1 new unit at general

**Terrain generation (basic)**
- 3-octave Perlin noise producing terrain_value 0.0-3.0 per hex
- Strategic value heatmap for balanced general placement
- Minimum distance constraints between spawns

**Fog of war**
- Vision radius per unit (3 hexes = 37 hex area)
- Terrain always visible, units hidden by fog
- Spectator sees everything plus vision boundaries

**SpreadAgent (placeholder)**
- Single-pass heuristic: produce when affordable, fan out toward center, engage when advantageous
- Phase 8 target: replace with Centurion multi-layer architecture (see v2-agent-spec.md)

**Web integration**
- V2 round-robin mode (SpreadAgent vs SpreadAgent)
- WebSocket spectator stream with replay support
- API endpoints for simulation, ASCII view, RR control

### Remaining V2 work (not yet built)

These systems complete the V2 vision. They are designed (see discussion docs) but not implemented.

**Two-resource physical economy**
- **Food**: produced by farmers on fertile hexes, consumed by all units per tick, perishable in stockpile. Starvation degrades strength then morale.
- **Material**: produced by workers on resource hexes (forest→wood, hills→stone), consumed by construction and equipment. Not perishable.
- Resources do NOT teleport. They exist at specific hexes and must be physically moved.

**Physical convoys**
- Convoy is an entity: cargo type + amount, position, destination, transport mode
- Transport modes with real tradeoffs:

  | Mode | Capacity | Road required | Speed | Food cost |
  |------|----------|---------------|-------|-----------|
  | Porter (worker) | Low | None | Slow everywhere | Low |
  | Pack animal | Medium | Trail minimum | Medium | Medium |
  | Ox cart | High | Dirt minimum | Slow on road | Medium |
  | Horse cart | Medium-high | Dirt minimum | Fast on road | High |

- Carts cannot move off-road. Pack animals need at least trails. Porters work anywhere.
- Convoys can be escorted (military unit assigned) or unescorted (vulnerable to raiding)

**Roads as constructed infrastructure**
- Quality levels: None → Trail → Dirt → Paved
- Higher tiers require material transported to the build site (recursive: roads speed their own construction outward)
- Terrain multiplies construction cost (flat cheap, mountain enormous)
- Roads determine which transport modes are viable, which determines supply throughput, which determines power projection range

**Population as labor**
- Population unit = cohort (~100 people). The atomic unit of labor.
- Roles: Idle | Farmer | Worker | Builder | Teamster | Soldier
- One person, one role at a time. Every person eats regardless of role.
- Role transitions have real time and material costs:
  - Civilian roles: fast, cheap
  - Idle → Soldier: slow (~100 ticks), costs material (equipment)
  - Soldier → Farmer: fast, but equipment sits idle, training decays
- Training: 0.0 (raw conscript) to 1.0 (veteran). Accumulates while in Soldier role, decays when not. Veterans are irreplaceable short-term.
- Every soldier was a farmer. Every farmer could be a soldier. But not both at once.

**Depots**
- Hex improvement that stores food and material
- Reduces food spoilage rate
- Creates the tambo/depot-chain pattern: stock depots along march routes, armies draw locally

**Terrain generation (full)**
- Region graph generated first (fairness constraints per player), then terrain fills in
- Height field from layered Perlin/simplex noise, parameterized per region archetype
- Hydraulic erosion pass (particle-based, ~10K droplets)
- Flow accumulation → river network with Strahler numbers
- Simple moisture model (noise + rain shadow from prevailing wind + elevation)
- Biome from Whittaker diagram lookup (temperature × moisture)
- Resource placement constrained to appropriate biomes
- Fairness validation: each player's Voronoi partition gets equivalent food capacity, material capacity, expansion directions, defensible terrain
- Region naming from archetype + distinctive features

**Multi-layer agent architecture (Centurion)**
- Strategic layer (every ~50 ticks): posture (expand/contest/attack/defend), region priorities, incremental influence maps
- Operational layer (every ~5 ticks): task generation + greedy matching of units to tasks
- Tactical layer (every poll): simulation-based engagement decisions for units near enemies
- Policy-based agents, no LLMs. Performance target: <1ms per poll.
- See v2-agent-spec.md for full design.

**Sim harness**
- Batch-run thousands of games for tuning
- Metrics: game length, territory over time, economic curves, engagement outcomes
- All tuning numbers are guesses until validated through the harness

### What V2 is NOT
- No building beyond roads and depots
- No fortifications or walls
- No siege equipment
- No unit type variety (all soldiers identical)
- No technology tree
- No morale (beyond starvation effects)
- No commanders or delegation
- No underground or air layers
- No ranged combat

---

## V3 — Depth

**Goal**: Every entity is autonomous. Terrain matters at every scale. Wars are
won through economy and infrastructure, not just army size. The spectator sees
individual stories — a farmer who became a soldier, a smith forging swords at
dawn, a ditch dug over three game-days that diverts a river and changes a
battle. Behavior emerges from physical simulation and needs-driven agents, not
from hardcoded unit types or role assignments.

V3 replaces V2's separate entity types with a single composable Entity primitive
(see `docs/specs/v3-entity-unification-2026-04-13.md`). Every person, structure,
tool, and resource is an entity with physical properties. Capabilities emerge
from composition, not categories.

### Implemented (Streams A-D + Phase 0)

**Entity unification + continuous spatial model**
- Single `Entity` with optional components (person, mobile, combatant, vitals, etc.)
- Continuous 3D positions; hex grid is derived projection with hysteresis
- Containment hierarchy (entities inside entities)
- Multi-resolution spatial index: fine (10m), hex (150m), coarse (500m)

**Material-interaction combat**
- 7-step damage pipeline: impact geometry → surface angle → block → armor → wound → bleeding → stagger
- Weapons and armor defined by physical properties (material, sharpness, weight, reach)
- Verlet body model with 16 skeletal points for hit detection
- Kinetic chain for momentum propagation across limbs

**Terrain operation log (Stream D)**
- Analytic geometric operations (ditch, wall, crater, road, flatten, furrow, bore) stacked per hex
- Evaluated procedurally at arbitrary resolution — 40 bytes per ditch, not 10k vertices
- Compaction when stack exceeds threshold; rasterization cache for viewer GPU upload
- Scales to continental maps: 30km map with 100k edits = ~4MB terrain state

**wgpu WASM viewer (Stream B)**
- WebGPU terrain renderer with clipmap LOD (1m/4m/16m/64m rings)
- Heightmap chunk dirtying + incremental GPU upload
- Entity rendering, hex overlay, camera controls

**Three-layer agent dispatch**
- Strategy (Spread/Striker/Turtle personalities), Operations, Tactical
- Strategy reads fog-of-war-filtered StrategicView
- Tactical triggers on engagement proximity

### In progress (Streams E-F)

**Agent behavior system (Stream E)**
- Needs-driven utility scoring (hunger, safety, duty, rest, social, shelter)
- HTN decomposition: goals → action queues via composable domain methods
- Dual-mode execution: tick-by-tick (physics) and batch-resolve (fast-forward)
- Decision frequency LOD + archetype clustering for continental scale
- Resolution demand generalized beyond combat: negotiation, construction, competition
- Social state: personality vectors, relationship cache, opinion dynamics
- See `docs/plans/v3-streamE-agent-behavior.md`

**Compositional world model (Stream F)**
- Physical properties on all entities (mass, hardness, temperature, material)
- Tool effectiveness through property interaction, not hardcoded per-structure-type
- Affordance queries: HTN methods find tools/materials by physical constraints
- Material transformation: "forge" = co-located fire source + anvil + hammer, not a special type
- See `docs/plans/v3-streamF-compositional-world.md`

### Remaining V3 scope

**Building and fortification** — structures as entities with physical properties.
Construction through terrain ops (ditches, walls) and entity placement. Material
quality affects integrity. Maintenance as ongoing cost.

**Terrain depth** — height field affects vision, combat, movement, construction
cost. Terrain ops (Stream D) enable player-scale modifications: trenches, roads,
leveled building sites, irrigation ditches.

**Population dynamics** — logistic growth driven by food satisfaction and carrying
capacity. Migration toward prosperity. Starvation cascades.

**Morale** — per-entity morale from need satisfaction, combat outcomes, social
state. Low morale → reduced effectiveness → rout → desertion. Emerges from
the needs system (Stream E), not from a separate morale mechanic.

**Equipment** — entities with weapon/armor properties. Material-interaction
combat already handles equipment effects. Production through compositional
world model (Stream F): entities shape materials using tools.

---

## V4 — Neural Evolution + Statecraft

**Goal**: Agents learn. Neural networks replace hand-tuned classical systems at
five insertion points. Entities evolve across generations — skill, strategy, and
social behavior emerge from selection pressure, not from authored rules. The game
becomes a simulation of governance where political failures are as interesting as
military ones.

V4 builds on V3's classical agent behavior system (Stream E) by inserting NEAT-
evolved neural networks at the interfaces the classical system defines. The
classical system remains as bootstrap policy and fallback.

### Neural evolution (see `docs/plans/future-neural-evolution.md`)

**Five insertion points:**
1. Utility scoring — `(needs, context) → goal_scores`. Replaces response curves.
2. HTN method selection — `(state, methods) → method_choice`. Replaces cost heuristic.
3. Body control — `(body_state, environment) → joint_forces`. Replaces discrete combat menu. Skill = network complexity. Emergent martial arts.
4. Tactical coordination — `(group_state, enemies) → signals`. Replaces formation commands.
5. Social reasoning — `(personality, relationships) → comm_strategy`. Replaces opinion heuristics.

**TensorNEAT** for batch inference (variable-topology nets tensorized, batched
via SIMD). **rtNEAT** for continuous evolution (no generational pause — offspring
replace worst performers on death). Cross-generational inheritance with mutation.

**Cultural evolution**: different lineages develop genuinely different behavioral
patterns over hundreds of generations. Not designed — emergent from differential
survival in different environments.

### Statecraft

**Delegation and commanders**
- Entities autonomously make local decisions (V3 needs/HTN system handles this)
- Communication delay: strategic directives travel at message speed
- Commander quality = evolved neural net sophistication
- Delegation emerges: faction leader adjusts need weights, commanders interpret locally

**Information as a resource**
- Scouting, observation, signal networks
- Information travel time proportional to distance
- Strategic fog: ruler sees aggregates, commanders see locals
- Deception: evolved social reasoning nets learn to bluff

**Loyalty and factions**
- Faction loyalty as emergent property of social interactions (V3 Stream E social state)
- Loyalty decays with distance, cultural difference, unmet needs
- Rebellion: entities with low loyalty + high capability may defect
- Empire size creates management pressure — the snowball counterbalance

**Diplomacy**
- AI-to-AI negotiation via social reasoning nets
- Treaties as game objects with enforceable terms
- Alliance formation from shared personality + shared threats
- Deception and betrayal emerge from adversarial social evolution

---

## V5 — Scale + Technology

**Goal**: Continental scale. Millions of entities across 300km+ maps over
centuries of game time. Technology progression from bronze to gunpowder. The
full arc of civilization: agriculture → urbanization → warfare → politics →
collapse → renewal.

### Continental scale
- LOD tiers: strategic (1 game-hour tick), tactical (1 game-second), close (20Hz full physics)
- Hybrid tier promotion: resolution demand triggers (stakes × uncertainty × conflict intensity)
- Batch resolution: entities fast-forward through action queues at strategic tier
- Archetype clustering: millions of entities, hundreds of decision passes
- Terrain operation log: continent-sized maps with million-edit histories in ~40MB

### Technology progression
- No tech tree. Technology emerges from material interaction discovery.
- Entities discover that heating iron + hammering produces better tools (compositional world model)
- Knowledge propagation via social interactions — training, apprenticeship, trade
- Regional technological divergence from isolation + different environments
- Military tech transitions (bronze → iron → steel, melee → ranged → gunpowder)
  change combat dynamics without architecture changes — same damage pipeline,
  different material properties

### Water and underground layers
- Rivers from heightfield provide logistics corridors
- Water transport entities (rafts, barges, ships)
- Underground layer for mining, siege tunneling, hidden movement
- Terrain ops (ditches, bores) connect surface to underground

### Advanced infrastructure
- Road networks as terrain ops (road variant) that modify movement speed
- Specialized districts emerge from spatial clustering of activities
- Infrastructure networks (aqueducts, canals) as terrain modifications
- Trade routes as persistent entity paths through road networks

---

## Dependency Chain

```
V2 (hex, tick, entities, economy, convoys, roads, population, agents)
 └→ V3 (entity unification, continuous space, material physics, terrain ops,
        autonomous agents, HTN behavior, compositional world model)
     └→ V4 (neural evolution at 5 insertion points, emergent martial arts,
            cultural evolution, statecraft, diplomacy)
         └→ V5 (continental scale, LOD tiers, batch resolution, technology
                progression, water/underground layers, advanced infrastructure)
```

Each version is playable and watchable on its own. V2 agents reason about
territory and combat. V3 agents are autonomous with coherent individual
narratives. V4 agents evolve — skill and strategy emerge from selection
pressure. V5 agents operate at civilization scale across centuries.

## Design Principles (all versions)

1. **Simulation first.** Model the world truthfully. Interesting gameplay emerges from honest mechanics.
2. **No special categories.** There are no unit types, building types, or resource types. There are entities with physical properties. Capabilities emerge from composition. A "forge" is tools co-located with a heat source. A "soldier" is a person with combat skill and a weapon.
3. **Every entity is autonomous.** Entities pursue their own needs. Strategic AI adjusts priorities and provides opportunities — it doesn't micromanage. Narrative coherence per entity is a hard requirement.
4. **Leverage compute.** AI agents handle complexity that would overwhelm humans. Agents evaluate thousands of situations per second. Spectators see consequences.
5. **Visible decisions.** Every agent decision is inspectable. "Why did this farmer walk to the river?" has a concrete answer in the needs/goals/action queue.
6. **Each version stands alone.** V2 without V3 is a game. V3 without V4 is a game. Each addition enriches without requiring the next.
7. **Physical movement is real.** All resource movement is physical entities on the map. No teleportation, no abstract flow networks.
8. **Population is the substrate.** Every action is a person doing a thing. Every person eats. Every decision is a labor allocation tradeoff.
9. **Actions have consequences.** Terrain ops persist. Social interactions change relationships. Economic decisions compound. A ditch dug in year 1 is still there in year 50.
10. **Scale through analytic representations.** Dense grids don't survive continental scale. Terrain ops, archetype clustering, decision LOD, and batch resolution are not optimizations — they're architectural requirements.
11. **Information has travel time.** (V4+) What you know depends on where your scouts are and how fast reports reach you. Uncertainty is a mechanic, not a limitation.
12. **Evolution over authoring.** (V4+) Neural nets evolve behavior that humans can't predict or author. The classical system is scaffolding for the evolutionary system.
