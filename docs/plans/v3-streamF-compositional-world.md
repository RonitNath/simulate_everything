# Stream F: Compositional World Model

Status: **ready for implementation** (E2 prerequisite for F1, E3 for F2+)
Depends on: Stream E (agent behavior — HTN engine + action queue)
Integrates with: Stream D (terrain ops), Stream A (body model), economy system

## Goal

Replace role-typed entity components (Structure, Resource, workshop-specific
economy logic) with a property-based composition system. A "forge" is not a
special entity type — it's a location where fire source + anvil + tongs + metal
stock happen to be co-located. A "farm" is a location with soil + seeds + water
access. Entity capabilities emerge from physical properties and spatial
affordances, not from hardcoded categories.

This is the foundation for massive proliferation of recipes, strategies, and
activities. New behaviors = new HTN method definitions parameterized by physical
constraints, not new Rust code.

## User stories

**As the simulation engine**, material transformation is resolved by checking
tool properties against material properties — a hammer multiplies force on
shaping actions the same way regardless of whether the material is iron, clay,
or wood. No per-material-type crafting code.

**As an entity**, I know I need to shape iron. I search my environment for
tools with `force_mult >= required_force` and heat sources with
`temp >= shaping_temp`. I find a hammer and a furnace at location A. I don't
know the human label "forge" — I just know that location satisfies my
preconditions.

**As a method author**, I define "ShapeMetal" by specifying physical constraints
(tool force, material temperature, material type), not named entity types.
The same method works for bronze, iron, and steel — the only difference is
the constraint thresholds.

**As the economy system**, production isn't "check if person has Farm task and
farm structure exists." Production is "entity executing Harvest action on
entity with `harvestable` property produces resource with `food` property."
The economy emerges from actions, not from role assignments.

## Current state

Entity components are role-typed:

```rust
pub struct Structure {
    pub structure_type: StructureType,  // Farm, Workshop, Village, City, Depot, Tower, Wall
    pub build_progress: f32,
    pub integrity: f32,
    pub capacity: u32,
    pub material: MaterialType,
}

pub struct Resource {
    pub resource_type: ResourceType,    // Food, Material
    pub amount: f32,
}
```

Economy in `economy.rs` hardcodes production per structure type:
- `TaskAssignment::Farm { site }` + `StructureType::Farm` → food production
- `TaskAssignment::Workshop { site }` + `StructureType::Workshop` → material production

Equipment production checks `StructureType::Workshop` specifically.

## Architecture

### Physical properties

Every entity gets a property set describing its physical characteristics:

```rust
struct PhysicalProperties {
    pub mass: f32,          // kg
    pub hardness: f32,      // 0.0 (cloth) to 1.0 (diamond)
    pub temperature: f32,   // Kelvin (ambient ~293K)
    pub material_type: MaterialComposition,
    pub state: MatterState, // Solid, Liquid, Gas, Powder
    pub tags: PropertyTags, // Bitflags: harvestable, edible, fuel, etc.
}

bitflags! {
    struct PropertyTags: u32 {
        const HARVESTABLE = 0x01;
        const EDIBLE      = 0x02;
        const FUEL         = 0x04;
        const HEAT_SOURCE  = 0x08;
        const TOOL         = 0x10;
        const CONTAINER    = 0x20;
        const SHELTER      = 0x40;
        const WORKABLE     = 0x80;    // can be shaped/cut/mixed
        const STRUCTURAL   = 0x100;   // load-bearing
    }
}
```

Tools have additional effectiveness properties:

```rust
struct ToolProperties {
    pub force_mult: f32,      // striking force multiplier (hammer: 5.0)
    pub precision: f32,       // shaping precision (chisel: 0.9)
    pub cutting_edge: f32,    // cutting ability (saw: 0.8, axe: 0.6)
    pub heat_output: f32,     // thermal energy per tick (furnace: 1000K)
    pub capacity: f32,        // volume for containers (basket: 50L)
    pub durability: f32,      // wear remaining (0.0 = broken)
    pub applicable_actions: PhysicalActionSet,  // which actions this tool enables
}
```

### Affordance queries

HTN preconditions check affordances — "is there a tool nearby that satisfies
constraint X?" — via spatial queries:

```rust
fn find_affordance(
    state: &GameState,
    near: Vec3,
    radius: f32,
    constraint: AffordanceConstraint,
) -> Option<EntityKey> {
    // Query fine_index for nearby entities
    // Filter by constraint (property check)
    // Return closest match
}

enum AffordanceConstraint {
    ToolWith { min_force: f32 },
    ToolWith { min_precision: f32 },
    HeatSource { min_temp: f32 },
    Material { material_type: MaterialComposition, tags: PropertyTags },
    Container { min_capacity: f32 },
    Any { tags: PropertyTags },
}
```

The affordance query is the bridge between HTN preconditions and the physical
world. When a method says "requires tool with force_mult >= 5.0," the
decomposition engine calls `find_affordance` to locate a suitable entity.

### Material transformation

Crafting/production resolves through physical interaction, not per-recipe code:

```
ApplyTool { tool: hammer, target: heated_iron, action: Shape }
→ check: tool.force_mult >= iron.hardness_at_temp(iron.temperature)
→ if pass: iron.shape changes, tool.durability -= wear
→ if fail: no effect (iron too hard at current temperature)
```

"Recipes" are HTN methods that specify the required sequence of transformations:

```
Method: ForgeIronSword
  preconditions:
    - nearby(entity WHERE tags.WORKABLE && material == Iron)
    - nearby(entity WHERE tags.HEAT_SOURCE && heat_output >= 1200K)
    - agent.holding(entity WHERE tags.TOOL && force_mult >= 5.0)
  subtasks:
    - AcquireTool { constraint: force_mult >= 5.0 }
    - Heat { target: iron, temp: 1200K }
    - ApplyTool { action: Shape, repetitions: 20 }
    - ApplyTool { action: Shape, tool: file, repetitions: 5 }  // edge work
    - Cool { target: shaped_iron }
  effects:
    - iron.shape = Sword
    - iron.tags |= TOOL  // now usable as a weapon/tool
    - agent.stamina -= effort
```

The same `ApplyTool { action: Shape }` primitive works for pottery, woodworking,
stonecutting — the material properties determine what happens.

### Transition strategy

The existing typed components (`Structure`, `Resource`) remain as convenience
wrappers during the transition. They're not deleted in V3 — they're augmented
with physical properties:

```rust
// Phase 1: Add properties alongside existing types
pub struct Entity {
    // Existing (kept for backward compat during transition)
    pub structure: Option<Structure>,
    pub resource: Option<Resource>,
    // New
    pub physical: Option<PhysicalProperties>,
    pub tool_props: Option<ToolProperties>,
}
```

HTN methods can query EITHER the typed components (for existing behavior) or
physical properties (for new compositional behavior). Over time, more methods
migrate to property-based preconditions and the typed components become
vestigial.

Full decomposition (removing Structure/Resource entirely) happens post-V3
when all methods have been migrated.

## Waves

### F1: Physical properties on entities

**Files created:**
- `crates/engine/src/v3/physical.rs` — `PhysicalProperties`, `ToolProperties`,
  `PropertyTags` bitflags, `MaterialComposition` enum, `MatterState` enum

**Files modified:**
- `crates/engine/src/v3/state.rs` — Add `physical: Option<PhysicalProperties>`
  and `tool_props: Option<ToolProperties>` to Entity
- `crates/engine/src/v3/mod.rs` — Declare module
- `crates/engine/src/v3/mapgen.rs` — Initialize physical properties on
  generated entities (structures get mass/hardness/material, resources get
  tags, weapons/armor get existing properties mapped to new format)
- `crates/engine/src/v3/lifecycle.rs` — Set physical properties on spawn

**Deliverables:**
- Physical property system on all entities
- Property initialization for existing entity types (structures, resources,
  weapons, armor all get physical properties derived from their typed data)
- Property tags for affordance queries

**Tests:**
- Farm structure has HARVESTABLE tag on contained crop entities
- Workshop structure contains entities with TOOL tag
- Weapon entities have physical properties matching WeaponProperties
- Resource entities have appropriate tags (food → EDIBLE, material → WORKABLE)

### F2: Affordance query system

**Files created:**
- `crates/engine/src/v3/affordance.rs` — `AffordanceConstraint`,
  `find_affordance()`, `find_all_affordances()`, spatial + property
  combined queries

**Files modified:**
- `crates/engine/src/v3/htn.rs` (from E3) — Integrate affordance queries
  into HTN precondition evaluation. Method preconditions can now reference
  `AffordanceConstraint` instead of specific entity keys.

**Deliverables:**
- Affordance query: find nearest entity satisfying property constraints
  within radius
- Integration with fine_index for spatial filtering
- Integration with HTN precondition system
- Caching: affordance results cached per decision cycle (invalidated on
  entity spawn/despawn/move)

**Tests:**
- Find hammer near anvil: returns entity with force_mult >= threshold
- Find heat source near workbench: returns furnace entity
- No match: returns None, HTN method fails precondition
- Radius constraint: entity outside radius not found
- Cache invalidation: entity moves, subsequent query finds new nearest

### F3: Material transformation through actions

**Files modified:**
- `crates/engine/src/v3/action_queue.rs` (from E4) — Implement `ApplyTool`
  action resolution for non-combat targets. Tool properties modify action
  effectiveness. Material properties determine outcome.
- `crates/engine/src/v3/economy.rs` — Add action-based production path
  alongside existing task-based path. `ApplyTool { action: Harvest }` on
  harvestable entity → produce food. `ApplyTool { action: Shape }` on
  workable material → transform shape.
- `crates/engine/src/v3/physical.rs` — Add temperature dynamics (heating,
  cooling), durability wear calculations

**Deliverables:**
- `ApplyTool` resolves against physical properties for non-combat targets
- Tool force vs material hardness check determines success
- Temperature requirements for metal working
- Tool durability wear on use
- Material transformation: shape changes, tag changes, new entity creation
- Existing economy still works via task-based path (gradual migration)

**Tests:**
- ApplyTool(hammer, heated_iron, Shape): iron shape changes
- ApplyTool(hammer, cold_iron, Shape): fails (too hard at low temp)
- ApplyTool(sickle, wheat, Harvest): produces food entity
- ApplyTool(axe, tree, Cut): produces wood entity
- Tool durability decreases on use
- Tool breaks at durability 0 → action fails

### F4: Domain methods migration

**Files modified:**
- `crates/engine/src/v3/domains/subsistence.rs` (from E5) — Migrate food
  acquisition methods from type-checking (`StructureType::Farm`) to
  property-checking (`tags.HARVESTABLE`)
- `crates/engine/src/v3/domains/material_work.rs` (from E5) — Migrate
  crafting methods from workshop-specific to affordance-based
- `crates/engine/src/v3/domains/construction.rs` (from E5) — Migrate
  building methods to use physical properties for material requirements
- `crates/engine/src/v3/economy.rs` — Mark task-based production as
  deprecated fallback. Action-based production is primary path.

**Deliverables:**
- All 6 domain areas use affordance queries instead of type checks
- Economy production driven by action execution, not task assignments
- Task-based path retained as fallback for entities not yet on action system
- Method definitions parameterized by physical constraints

**Tests:**
- Entity farms via action queue (not task assignment) → food produced
- Entity crafts sword via action queue → sword entity created
- Entity builds wall via action queue → structure placed + terrain modified
- New material type (e.g., clay) works with existing Shape method without code changes
- Remove workshop structure → entity still crafts if tools + materials present elsewhere

## Verification criteria (full stream)

- [ ] Every entity has physical properties initialized from its typed data
- [ ] Affordance queries return correct entities for physical constraints
- [ ] ApplyTool resolves against physical properties (force, temperature, hardness)
- [ ] Material transformation produces correct output entities
- [ ] Tool durability decreases on use, tools break at zero
- [ ] Temperature dynamics: heating raises temp, cooling decays toward ambient
- [ ] Domain methods use affordance queries, not type checks
- [ ] Economy production works through action execution
- [ ] Existing typed components still function during transition
- [ ] New material types work without code changes (data-only method definitions)
- [ ] Performance: affordance query < 100μs (spatial filter + property check)
- [ ] Performance: no regression in economy tick with dual production path
- [ ] "Forge" emerges from co-located tools + heat source, not from StructureType
