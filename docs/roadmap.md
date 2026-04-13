# Generals — Version Roadmap

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

**Goal**: Defense in depth works. Sieges are logistics operations. Terrain matters at every scale. Wars are won through economy and infrastructure, not just army size. The spectator sees fortification lines, siege camps, supply convoys, and the slow grinding pressure of attrition.

### Building and fortification

**Hex improvements** (structures on hexes):
- Camp → Depot → Workshop → Tower → Keep → Citadel
- Material quality tiers: Earthwork → Timber → Stone → Masonry
- Each tier: different build speed, material cost, integrity decay rate, combat multiplier
- Maintenance as ongoing cost: unmaintained structures decay. Over-building creates a maintenance burden.
- Garrison capacity: structures hold N soldiers who fight at the structure's defense multiplier

**Edge barriers** (walls on hex edges):
- Ditch → Palisade → Wall → Gatehouse
- Same material quality tiers
- `built_by` determines which side gets defensive bonus
- Gates: passable to builder's units, barrier to enemies
- Walls restrict movement for everyone including the builder (trade routes, population movement)

**Multi-hex structures** (patterns, not special entities):
- Roman marching camp: 1 center hex (camp) + ditch edges. Cheap, fast, temporary. Built every night by trained soldiers.
- Motte and bailey: keep hex + palisade bailey hexes. Early permanent fort.
- Star citadel: citadel center + stone wall edges on surrounding hexes. Overlapping defensive coverage from hex geometry. Nearly impregnable without siege.
- Great Wall: linear chain of wall edges + watchtower hexes + interior road. Enormous cost, channels enemy movement to gates.

**Siege equipment** (built at siege sites from transported materials):
- Battering ram: attacks edge barriers, degrades integrity
- Siege tower: negates wall height advantage for attackers
- Siege ramp: workers pile earth against wall, slow but eventually negates wall entirely
- Sapping: tunnel under wall from underground layer, collapse it

**Bridges**:
- Timber bridge (fast, enables foot/pack), stone bridge (slow, expensive, enables carts, durable), pontoon bridge (military engineers, temporary, fast army crossing)

### Underground layer
- Same hex grid at negative layer depth, connected to surface by dug entrances
- Mining: workers dig tunnels, extract ore/stone. Resource hexes hint at what's below.
- Siege mining: tunnel under enemy wall, collapse. Defender can counter-mine.
- Trenches: shallow surface excavation, provides wall-like defense without wall-like cost. Trench lines with communication trenches.
- Hidden movement: tunnel exits behind enemy lines for surprise attacks.

### Water transport
- Rafts, barges, ships with capacity/speed tradeoffs
- Rivers from height field provide natural logistics corridors (10-50× land transport efficiency)
- Harbors as hex improvements enabling water transport
- Upstream vs downstream speed differential
- River control = logistics control

### Terrain depth
- Full biome simulation: moisture carried by wind, rain shadows behind mountains, vegetation from temperature × moisture
- Height field affects: vision (higher sees further), combat (uphill penalty), movement (steep = slow/impassable), construction cost, water flow
- Region identity: named regions with strategic character, referenced by agents and spectator narration
- Foraging: armies extract food from hex, damaging fertility. Scorched earth destroys own fertility to deny enemy forage. Recovery is slow.
- Water access as terrain constraint: low water hexes cost extra rations, very low = impassable without engineering

### Population growth
- Logistic growth: `growth_rate = base_rate * food_satisfaction * (1 - pop/carrying_capacity)`
- Food surplus → growth, deficit → death (starvation spiral: fewer farmers → less food → more death)
- Population concentrates in fertile regions, creates demographic pressure for expansion

### Morale
- Per-unit morale from: supply state, combat outcomes, leadership proximity, fatigue
- Low morale: reduced combat effectiveness, eventual rout (flee before zero strength)
- Rout cascade: one unit routing debuffs adjacent friendlies
- Surrender: encircled + demoralized units surrender rather than fight to death
- Starvation and morale interact: starving units lose morale fast

### Equipment and technology
- Equipment as resource: soldiers need equipment (material cost), equipment quality affects combat
- Technology progression unlocks: construction tiers, transport modes, equipment quality, economic improvements
- Small tech tree (8-15 nodes) with meaningful branches
- Military tech = short-term unit superiority. Economic tech = long-term production advantage.
- Training units from generic recruits + equipment = specialized soldier

### Unit types (basic)
- Infantry: holds ground, defensive backbone, garrisons forts
- Cavalry: fast, scouts, raids convoys, flanks in combat
- Workers: build, farm, mine, transport — the economy
- Scouts: fast, fragile, extended vision radius
- Rock-paper-scissors dynamics from engagement model + speed + strength differences

---

## V4 — Statecraft

**Goal**: The game becomes a simulation of governance, not just warfare. The player's role shifts from "move these units" to "govern this nation." The interesting failures are political. Large empires face internal pressure that counterbalances snowballing.

### Commanders and delegation
- Commanders are trained entities: competence stats, personality traits (aggressive/cautious/brilliant/incompetent)
- Assigned to armies, they make local decisions autonomously between orders
- Communication delay: orders from general travel at message speed (proportional to distance). Commanders interpret strategic intent with local information.
- Commander quality matters: a brilliant subordinate compensates for bad grand strategy. An incompetent one squanders a good army.
- Promoting/demoting commanders has consequences — loyalty, morale of their troops

### Information as a resource
- Scouting: dedicated scout units, observation towers, signal fires
- Information travel time: what happened at the frontier reaches the capital ticks later
- Strategic fog: the ruler doesn't see what commanders see in real time
- Intelligence: captured enemy scouts, intercepted convoys reveal enemy plans
- Deception: feints, false movements, information denial
- Signal network: faster communication infrastructure (signal towers, relay riders) = faster response

### Loyalty and factions
- Commanders accumulate influence through success and troop loyalty
- Over-centralization vs delegation tradeoff: tight control = slow response + bottleneck at ruler. Delegation = faster response + risk of disloyalty.
- Distant provinces drift: loyalty decays with distance from capital, cultural difference, communication delay
- Rebellion: disloyal commanders with loyal troops can revolt. Conquered territories may rebel if under-garrisoned.
- Faction pressure scales with empire size: more territory = more commanders = more loyalty management = harder to hold together. This is the snowball counterbalance.

### Political and social simulation
- Population has social structure: classes, occupations, cultural identity
- War weariness: prolonged war reduces civilian morale and growth
- Legitimacy: based on military success, economic prosperity, justice (not over-taxing, not losing wars badly)
- Cultural assimilation: conquered populations slowly integrate or resist depending on treatment
- Migration: population moves toward prosperity and safety, away from war and famine
- Civil unrest from: taxation, conscription, famine, military losses, perceived incompetence

### Player interaction model
- Grand strategy notes to your AI: "Reinforce the northern theater." "Prioritize economy." "I don't trust Commander X." "Accept peace terms if offered."
- The AI interprets and executes — but interpretation depends on commander competence, loyalty, and communication delay
- Risk: promoting commanders who make bad decisions, trusting disloyal governors, misjudging enemy intent
- The player governs; the simulation plays

### Diplomacy
- AI-to-AI diplomacy between factions: alliances, truces, tribute, marriage, betrayal
- Driven by strategic calculus (threat assessment, resource comparison, geographic opportunity)
- Treaties as game objects: terms, duration, violation consequences
- Coalition dynamics in multi-player: kingmaker problems, balance-of-power politics

---

## V5 — Technology and Range

**Goal**: The full arc from bronze spears to rifled artillery. Ranged combat changes everything — fortification design, unit composition, tactical doctrine. The same terrain and logistics systems from V2-V4 handle it without architecture changes.

### Ranged combat
- Archers, slingers, javelins: range as hex distance, line of sight from height field
- Projectile mechanics: arc, drop-off, blocked by terrain/walls
- Ranged units thin formations before melee contact — fundamental tactical shift
- Counter-play: shields, cover, closing distance quickly (cavalry charge through arrow fire)

### Full unit type system
- Rock-paper-scissors on hex with edge-based engagement:
  - Heavy infantry holds against cavalry charge
  - Cavalry flanks around infantry to hit archers
  - Archers thin advancing infantry at range
  - Siege engines break walls that infantry can't crack
  - Skirmishers harass and screen
- Unit composition as strategic decision: army composition determines what it can and can't do

### Hot weapons transition
- Gunpowder as late-tech unlock
- Changes engagement math: range dominates melee, volume of fire matters, cover is critical
- Fortification evolution: medieval walls → star forts (angled walls deflect cannon, overlapping fields of fire — already supported by hex geometry)
- Trench warfare emerges naturally: defense dominates offense when both sides have firearms + trenches (V3 underground layer)
- Artillery: long range, breaks fortifications, suppresses infantry. Counter: concealment, mobility, counter-battery

### Air layer
- Observation first: balloons, elevated scouts. Vision bonus from height.
- Later: air units that ignore surface terrain. Fast, visible, can't hold ground.
- Surface height affects air interaction: mountains bring surface closer to air layer

### Advanced naval
- Open-water navigation (not just rivers)
- Naval combat: boarding (melee), ramming, later cannon
- Amphibious operations: landing armies from ships
- Blockade: cut coastal supply and trade
- Naval logistics: sea transport even more efficient than river

### Economic warfare
- Trade routes between regions/factions as game objects
- Blockade and embargo as strategic tools
- Currency and trade manipulation (luxury goods for diplomacy/morale)
- Piracy and commerce raiding

### Advanced building
- Specialized city districts: residential, commercial, industrial, military, religious
- Wonders: enormous construction projects with civilization-wide effects
- Infrastructure networks: aqueducts, canal systems, road networks as strategic assets

---

## Dependency Chain

```
V2 (hex, tick, entities, economy, convoys, roads, population, agents)
 └→ V3 (building, siege, underground, water, terrain depth, morale, equipment, tech, unit types)
     └→ V4 (commanders, information, loyalty, factions, social simulation, diplomacy, player-as-ruler)
         └→ V5 (ranged, hot weapons, full unit types, air, naval, economic warfare, advanced building)
```

Each version is playable and watchable on its own. Each adds a layer of strategic depth without invalidating the previous. The agent architecture scales naturally: V2 agents reason about territory and combat. V3 agents reason about infrastructure and economy. V4 agents delegate to sub-agents (commanders). V5 agents manage combined arms and combined domains (land, sea, air).

## Design Principles (all versions)

1. **Simulation first.** Model the world truthfully. Interesting gameplay emerges from honest mechanics.
2. **AI-readable.** Deep simulation, structured observation interface. Layered observations keep per-decision context manageable.
3. **Leverage compute.** AI agents handle complexity that would overwhelm humans. Detailed supply chains, morale propagation, terrain effects — agents evaluate thousands of tiles per second. Agents handle complexity; spectators see consequences.
4. **Visible decisions.** Every agent decision is visible on the map and debatable by spectators. Spectators should be able to say "why did Blue do that?" and form opinions.
5. **Behavioral defaults.** Units have autonomous behaviors for routine decisions. Strategic AI issues directives, not micromanagement.
6. **Each version stands alone.** V2 without V3 is a game. V3 without V4 is a game. Each addition enriches without requiring the next.
7. **The convoy is the primitive.** All resource movement is physical entities on the map. No teleportation, no abstract flow networks.
8. **Population is the substrate.** Every action is a person doing a thing. Every person eats. Every decision is a labor allocation tradeoff.
9. **Construction is permanent investment with ongoing cost.** Infrastructure defines your empire's shape and capability. Maintenance means you can over-build. Roads and fortifications are commitments to a strategic posture.
10. **Information has travel time.** (V4+) What you know depends on where your scouts are and how fast reports reach you. Uncertainty is a mechanic, not a limitation.
