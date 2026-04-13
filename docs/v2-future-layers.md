# simulate_everything — Future Layers Beyond V2

This document captures the long-term design vision discussed during the V2 design session. These are NOT part of the V2 implementation. They are future layers that the V2 foundation is designed to support.

## Layer 1: Unit Types

Add unit variety beyond the generic company:

| Unit | Strength | Speed | Upkeep | Cost | Role |
|------|----------|-------|--------|------|------|
| Infantry | 100 | 1 cell/3 ticks | 1/tick | 10 | Holds ground. Defensive backbone. Garrisons forts. |
| Knight/Cavalry | 60 | 1 cell/1 tick | 2/tick | 20 | Fast. Scouts, raids, flanks. Chases retreating units. |
| Archer | 50 | 1 cell/3 ticks | 1.5/tick | 15 | Ranged attack (2-3 cells). Weak in melee. |
| Siege | 150 | 1 cell/6 ticks | 3/tick | 40 | Breaks fortifications. Range 2. Slow, devastating. |

Unit types enable rock-paper-scissors dynamics:
- Infantry screens archers (positional — infantry in front cell, archers behind)
- Cavalry flanks around infantry to hit archers
- Siege breaks fortified positions that infantry can't crack
- Archers thin out advancing infantry before melee contact

Training/retraining: units are trained from generic recruits. Retraining into adjacent types (infantry → archer) is faster than distant types (infantry → siege). Equipment is separate from the unit and must be produced (requires technology).

## Layer 2: Fortifications and Defense in Depth

Workers (or units with a build action) construct structures on hexes:

- **Fort**: 2x defense multiplier for occupants. Provides vision. Acts as supply node. Siege units negate the multiplier.
- **Wall segment**: blocks movement across a specific hex edge. Must be breached by siege. On a hex grid, walls are placed on edges (6 possible per hex), allowing directional defense — a fort with walls on 3 faces creates a strongpoint open to retreat on the other 3.
- **Granary**: stores resources. Can be raided.
- **Road**: reduces movement cost between connected hexes. Increases supply throughput.

A chain of forts along a terrain feature (river, hills) creates defense in depth. Attackers must bring siege, advance slowly, reduce each position. Defenders can counterattack exposed siege columns with cavalry.

Fort zone of control: a fort projects influence into adjacent hexes (all 6 neighbors), making it harder for enemies to pass without engaging. A line of forts with overlapping ZOC creates a front that can't be bypassed.

Hex walls enable Alesia-style scenarios: a ring of wall segments facing inward (circumvallation) surrounding a besieged force, with a second ring facing outward (contravallation) defending against relief. Each wall segment is placed on a hex edge, so the geometry is clean and uniform.

## Layer 3: Supply and Logistics

Replace instant resource teleportation with physical transport:

- Resources are produced at tiles, stored in granaries
- Supply convoys (wagon entities) move resources along roads from granaries to armies
- Armies beyond supply range forage (deplete tile resources) or starve (take attrition)
- Convoys are named entities — raidable by cavalry
- Supply range extends from: general, cities/villages, forts, granaries

This naturally limits force projection. You can't send an army across the map without building supply infrastructure behind it. Knights raiding supply convoys becomes a devastating strategy.

Foraging mechanic: each tile has a forage value that regenerates slowly. Large armies deplete it fast. Scorched earth (retreating while depleting your own tiles) becomes a strategy.

## Layer 4: Technology and Equipment

Historical progression: Agriculture → Bronze Age → Iron Age → (future ages).

Technology is researched by spending resources over time. Each tech unlocks:
- New equipment (bronze swords, iron armor, shields, bows)
- New unit training options
- New structures (walls require masonry, roads require wheel)
- Economic improvements (better farming = more food per tile)

Equipment is separate from units. A "soldier" + "bronze sword" + "shield" = a bronze swordsman. Equipment must be produced (requires resources + smithy + tech). Losing equipped soldiers means losing equipment.

Tech tree is small (8-15 nodes) with meaningful branches. An agent that beelines military tech gets a window of unit superiority. An agent that invests in economic tech builds a stronger base for the long game.

## Layer 5: Population and Economy

Replace simple "10 resources = 1 unit" with a population model:

- Cities/settlements have population that grows based on food supply and safety
- Population is the base resource — people become workers, soldiers, scholars
- Training a soldier removes a worker from the economy (guns vs butter tension)
- Growth happens in cycles (generation pulses, like the current reinforcement wave)
- Population has needs: food, water, safety. Unmet needs slow growth or cause decline.

Workers on tiles extract resources. The economy is physically on the map and vulnerable. A cavalry raid that kills workers hurts the economy for many turns.

Tile degradation: overworking a tile reduces its output. Sustainable farming yields less per turn but maintains the tile. Intensive farming yields more but degrades it. This creates land management decisions.

## Layer 6: Commanders and Delegation

Replace the single omniscient AI controller with a command hierarchy:

- The top-level AI is the "ruler" — sets grand strategy, allocates resources
- Frontline commanders are semi-autonomous agents that control regional forces
- Orders from the ruler take time to reach commanders (proportional to distance)
- Commanders interpret orders with local information — they may execute imperfectly
- Commander quality varies (traits: aggressive, cautious, brilliant, incompetent)

This creates:
- Communication delays (orders take time to traverse space)
- Fog of war at the strategic level (the ruler doesn't see what the commanders see in real time)
- Coordination failures (two commanders may conflict)
- The "brilliant subordinate" dynamic — a good commander compensates for bad orders

The player AI's job shifts from micromanagement to: appointing commanders, setting objectives, allocating resources, and managing the command structure.

## Layer 7: Morale and Social

Units have morale that affects combat performance:

- High morale: fight harder, hold longer
- Low morale: fight worse, may rout (flee)
- Rout cascade: one unit routing debuffs adjacent friendly units
- Leadership: proximity to general/commander improves morale

Morale is affected by: winning/losing combat, being flanked, being outnumbered, supply status, fatigue, general death.

Social factors at the civilization level:
- War weariness: prolonged war reduces population morale and growth
- Loyalty: conquered territories may rebel if not garrisoned
- Culture: different civilizations have different strengths/weaknesses

## Design Principles

These principles guide all future layers:

1. **Simulation first, fun second.** Model the world truthfully. Let interesting gameplay emerge from the simulation. Don't design mechanics for fun and paper over gaps.

2. **AI-readable.** The simulation can be deep internally, but the observation interface must be structured and comprehensible. Layered observations (strategic summary, region detail, entity list) keep per-decision context manageable.

3. **Leverage compute.** AI agents can handle complexity that would overwhelm human players. Detailed supply chains, morale propagation, terrain effects — all viable because agents can evaluate thousands of tiles per second. The agents handle the complexity; spectators see the consequences.

4. **Visible decisions.** Every agent decision should be visible on the map and debatable by spectators. Unit composition, fort placement, tech choices, army movements — these ARE the entertainment. Spectators should be able to say "why did Blue do that?" and form opinions.

5. **Behavioral defaults.** Units have autonomous behaviors for routine decisions (workers farm, soldiers garrison, scouts explore). The strategic AI issues directives, not micromanagement. This keeps the action space tractable and prepares for the commander layer.

6. **Build up from base layers.** Each layer should be playable independently. V2 without unit types is still a game. V2 + unit types without supply is still a game. Each addition enriches without requiring the next.
