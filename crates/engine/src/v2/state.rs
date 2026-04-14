use super::gamelog::GameLog;
use super::hex::{Axial, axial_to_offset};
use super::spatial::SpatialIndex;
use bitvec::prelude::BitVec;
use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

new_key_type! {
    pub struct UnitKey;
    pub struct PopKey;
    pub struct ConvoyKey;
    pub struct SettlementKey;
    pub struct EntityKey;
}

/// Settlement tier determines radius of territory claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementType {
    /// Small farming community (pop 2–9). Claims only its own hex.
    Farm,
    /// Established village (pop 10–29). Claims radius-1 hex ring.
    Village,
    /// Major city (pop 30+). Claims radius-2 hex ring.
    City,
}

/// A persistent settlement entity that anchors territory for a player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settlement {
    pub public_id: u32,
    pub hex: Axial,
    pub owner: u8,
    pub settlement_type: SettlementType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Biome {
    Desert,
    Steppe,
    Grassland,
    Forest,
    Jungle,
    Tundra,
    Mountain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegionArchetype {
    RiverValley,
    Highland,
    MountainRange,
    CoastalPlain,
    Forest,
    Desert,
    Pass,
    Delta,
    Plateau,
    Steppe,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Cell {
    pub terrain_value: f32,
    pub material_value: f32,
    pub food_stockpile: f32,
    pub material_stockpile: f32,
    pub has_depot: bool,
    pub road_level: u8,
    pub height: f32,
    pub moisture: f32,
    pub biome: Biome,
    pub is_river: bool,
    pub water_access: f32,
    pub region_id: u16,
    /// Derived from territory_cache; written back each tick so the rest of the
    /// codebase (observation, replay) can read it without structural changes.
    pub stockpile_owner: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engagement {
    pub enemy_id: UnitKey,
    pub edge: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unit {
    pub public_id: u32,
    pub owner: u8,
    pub pos: Axial,
    pub strength: f32,
    pub move_cooldown: u8,
    pub engagements: Vec<Engagement>,
    pub destination: Option<Axial>,
    pub rations: f32,
    pub half_rations: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Idle,
    Farmer,
    Worker,
    Soldier,
    Builder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceType {
    Food,
    Material,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructureType {
    Farm,
    Village,
    City,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub health: f32,
    pub combat_skill: f32,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mobile {
    pub speed: f32,
    pub move_cooldown: u8,
    pub destination: Option<Axial>,
    pub route: Vec<Axial>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vision {
    pub radius: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Combatant {
    pub engaged_with: Vec<EntityKey>,
    pub facing: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub resource_type: ResourceType,
    pub amount: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Structure {
    pub structure_type: StructureType,
    pub build_progress: f32,
    pub health: f32,
    pub capacity: usize,
}

/// A unified entity that carries optional component bags for any combination of
/// Person, Mobile, Vision, Combatant, Resource, and Structure behaviours.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: u32,
    pub pos: Option<Axial>,
    pub owner: Option<u8>,
    pub contained_in: Option<EntityKey>,
    pub contains: Vec<EntityKey>,
    pub person: Option<Person>,
    pub mobile: Option<Mobile>,
    pub vision: Option<Vision>,
    pub combatant: Option<Combatant>,
    pub resource: Option<Resource>,
    pub structure: Option<Structure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Population {
    pub public_id: u32,
    pub hex: Axial,
    pub owner: u8,
    pub count: u16,
    pub role: Role,
    pub training: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CargoType {
    Food,
    Material,
    Settlers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Convoy {
    pub public_id: u32,
    pub owner: u8,
    pub pos: Axial,
    pub origin: Axial,
    pub destination: Axial,
    pub cargo_type: CargoType,
    pub cargo_amount: f32,
    pub capacity: f32,
    pub speed: f32,
    pub move_cooldown: u8,
    pub returning: bool,
    /// Remaining waypoints toward destination, next step first. Empty means at destination or route not yet computed.
    pub route: Vec<Axial>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: u8,
    pub food: f32,
    pub material: f32,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Region {
    pub id: u16,
    pub name: String,
    pub archetype: RegionArchetype,
    pub hexes: Vec<Axial>,
    pub avg_fertility: f32,
    pub avg_minerals: f32,
    pub defensibility: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub width: usize,
    pub height: usize,
    /// Row-major grid in offset coordinates.
    pub grid: Vec<Cell>,
    pub units: SlotMap<UnitKey, Unit>,
    pub players: Vec<Player>,
    pub population: SlotMap<PopKey, Population>,
    pub convoys: SlotMap<ConvoyKey, Convoy>,
    pub settlements: SlotMap<SettlementKey, Settlement>,
    pub regions: Vec<Region>,
    pub tick: u64,
    /// Monotonically increasing counters for frontend/replay-facing IDs.
    pub next_unit_id: u32,
    pub next_pop_id: u32,
    pub next_convoy_id: u32,
    pub next_settlement_id: u32,
    pub entities: SlotMap<EntityKey, Entity>,
    pub next_entity_id: u32,
    pub scouted: Vec<Vec<bool>>,
    #[serde(skip)]
    pub spatial: SpatialIndex,
    #[serde(skip)]
    pub dirty_hexes: BitVec,
    #[serde(skip)]
    pub hex_revisions: Vec<u64>,
    #[serde(skip)]
    pub next_hex_revision: u64,
    /// Rebuilt each tick from settlement radii + unit presence.
    /// Index matches grid (row-major). None means unclaimed.
    #[serde(skip)]
    pub territory_cache: Vec<Option<u8>>,
    #[cfg(debug_assertions)]
    #[serde(skip)]
    pub tick_accumulator: Option<TickAccumulator>,
    #[serde(skip)]
    pub game_log: Option<GameLog>,
}

impl GameState {
    pub fn rebuild_spatial(&mut self) {
        self.spatial.rebuild(&self.units);
        self.spatial.rebuild_entities(&self.entities);
    }

    /// Entities that can act as military units (have person + mobile + combatant).
    pub fn entity_units(&self) -> impl Iterator<Item = (EntityKey, &Entity)> {
        self.entities
            .iter()
            .filter(|(_, e)| e.person.is_some() && e.mobile.is_some() && e.combatant.is_some())
    }

    /// Entities with Structure component.
    pub fn entity_structures(&self) -> impl Iterator<Item = (EntityKey, &Entity)> {
        self.entities.iter().filter(|(_, e)| e.structure.is_some())
    }

    /// Resource entities at a specific hex.
    pub fn resources_at(&self, hex: Axial) -> impl Iterator<Item = (EntityKey, &Entity)> {
        self.entities
            .iter()
            .filter(move |(_, e)| e.resource.is_some() && e.pos == Some(hex))
    }

    /// All entities at a specific hex.
    pub fn entities_at(&self, hex: Axial) -> impl Iterator<Item = (EntityKey, &Entity)> {
        self.entities
            .iter()
            .filter(move |(_, e)| e.pos == Some(hex))
    }

    /// Entities contained in another entity.
    pub fn contained_in(&self, key: EntityKey) -> impl Iterator<Item = (EntityKey, &Entity)> {
        self.entities
            .iter()
            .filter(move |(_, e)| e.contained_in == Some(key))
    }

    pub fn spawn_entity(&mut self, mut entity: Entity) -> EntityKey {
        entity.id = self.next_entity_id;
        self.next_entity_id += 1;
        self.entities.insert(entity)
    }

    pub fn index(&self, row: usize, col: usize) -> usize {
        row * self.width + col
    }

    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.grid[self.index(row, col)]
    }

    pub fn cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
        let idx = self.index(row, col);
        &mut self.grid[idx]
    }

    pub fn mark_dirty_index(&mut self, idx: usize) {
        if idx < self.dirty_hexes.len() {
            self.dirty_hexes.set(idx, true);
            self.next_hex_revision += 1;
            self.hex_revisions[idx] = self.next_hex_revision;
        }
    }

    pub fn mark_dirty_axial(&mut self, ax: Axial) {
        let (row, col) = axial_to_offset(ax);
        if row < 0 || col < 0 {
            return;
        }
        let (row, col) = (row as usize, col as usize);
        if row < self.height && col < self.width {
            self.mark_dirty_index(self.index(row, col));
        }
    }

    pub fn clear_dirty_hexes(&mut self) {
        self.dirty_hexes.fill(false);
    }

    pub fn cell_at(&self, ax: Axial) -> Option<&Cell> {
        let (row, col) = axial_to_offset(ax);
        if row < 0 || col < 0 {
            return None;
        }
        let (row, col) = (row as usize, col as usize);
        if row < self.height && col < self.width {
            Some(&self.grid[self.index(row, col)])
        } else {
            None
        }
    }

    pub fn cell_at_mut(&mut self, ax: Axial) -> Option<&mut Cell> {
        let (row, col) = axial_to_offset(ax);
        if row < 0 || col < 0 {
            return None;
        }
        let (row, col) = (row as usize, col as usize);
        if row < self.height && col < self.width {
            let idx = self.index(row, col);
            Some(&mut self.grid[idx])
        } else {
            None
        }
    }

    pub fn in_bounds(&self, ax: Axial) -> bool {
        let (row, col) = axial_to_offset(ax);
        row >= 0 && col >= 0 && (row as usize) < self.height && (col as usize) < self.width
    }

    pub fn population_on_hex(&self, owner: u8, ax: Axial) -> u16 {
        self.population
            .values()
            .filter(|p| p.owner == owner && p.hex == ax)
            .map(|p| p.count)
            .sum()
    }

    /// Whether a player has a Settlement entity on the given hex.
    pub fn is_settlement(&self, owner: u8, ax: Axial) -> bool {
        self.settlements
            .values()
            .any(|s| s.owner == owner && s.hex == ax)
    }

    pub fn settlement_on_hex(&self, ax: Axial) -> Option<&Settlement> {
        self.settlements.values().find(|s| s.hex == ax)
    }

    pub fn unit_key_by_public_id(&self, public_id: u32) -> Option<UnitKey> {
        self.units
            .iter()
            .find_map(|(key, unit)| (unit.public_id == public_id).then_some(key))
    }

    pub fn unit_by_public_id(&self, public_id: u32) -> Option<&Unit> {
        let key = self.unit_key_by_public_id(public_id)?;
        self.units.get(key)
    }

    pub fn unit_by_public_id_mut(&mut self, public_id: u32) -> Option<&mut Unit> {
        let key = self.unit_key_by_public_id(public_id)?;
        self.units.get_mut(key)
    }

    pub fn pop_key_by_public_id(&self, public_id: u32) -> Option<PopKey> {
        self.population
            .iter()
            .find_map(|(key, pop)| (pop.public_id == public_id).then_some(key))
    }

    pub fn convoy_key_by_public_id(&self, public_id: u32) -> Option<ConvoyKey> {
        self.convoys
            .iter()
            .find_map(|(key, convoy)| (convoy.public_id == public_id).then_some(key))
    }

    /// Resolve an entity's effective hex position: own pos, or container's pos if contained.
    pub fn entity_hex(&self, entity: &Entity) -> Option<Axial> {
        if let Some(pos) = entity.pos {
            return Some(pos);
        }
        if let Some(container_key) = entity.contained_in
            && let Some(container) = self.entities.get(container_key)
        {
            return container.pos;
        }
        None
    }

    pub fn has_unit_at(&self, ax: Axial) -> bool {
        self.spatial.has_unit_at(ax)
    }

    #[cfg(debug_assertions)]
    pub fn record_food_produced(&mut self, amount: f32) {
        if let Some(acc) = self.tick_accumulator.as_mut() {
            acc.food_produced += amount;
        }
    }

    #[cfg(debug_assertions)]
    pub fn record_material_produced(&mut self, amount: f32) {
        if let Some(acc) = self.tick_accumulator.as_mut() {
            acc.material_produced += amount;
        }
    }

    #[cfg(debug_assertions)]
    pub fn record_food_consumed(&mut self, amount: f32) {
        if let Some(acc) = self.tick_accumulator.as_mut() {
            acc.food_consumed += amount;
        }
    }

    #[cfg(debug_assertions)]
    pub fn record_material_consumed(&mut self, amount: f32) {
        if let Some(acc) = self.tick_accumulator.as_mut() {
            acc.material_consumed += amount;
        }
    }

    #[cfg(debug_assertions)]
    pub fn record_food_destroyed(&mut self, amount: f32) {
        if let Some(acc) = self.tick_accumulator.as_mut() {
            acc.food_destroyed += amount;
        }
    }

    #[cfg(debug_assertions)]
    pub fn record_material_destroyed(&mut self, amount: f32) {
        if let Some(acc) = self.tick_accumulator.as_mut() {
            acc.material_destroyed += amount;
        }
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TickAccumulator {
    pub food_produced: f32,
    pub food_consumed: f32,
    pub food_destroyed: f32,
    pub material_produced: f32,
    pub material_consumed: f32,
    pub material_destroyed: f32,
}

#[cfg(test)]
mod entity_tests {
    use super::super::mapgen::{MapConfig, generate};
    use super::*;

    fn make_empty_state() -> GameState {
        GameState {
            width: 10,
            height: 10,
            grid: vec![],
            units: SlotMap::with_key(),
            players: vec![],
            population: SlotMap::with_key(),
            convoys: SlotMap::with_key(),
            settlements: SlotMap::with_key(),
            regions: vec![],
            tick: 0,
            next_unit_id: 0,
            next_pop_id: 0,
            next_convoy_id: 0,
            next_settlement_id: 0,
            entities: SlotMap::with_key(),
            next_entity_id: 0,
            scouted: vec![],
            spatial: super::super::spatial::SpatialIndex::new(10, 10),
            dirty_hexes: bitvec::prelude::BitVec::new(),
            hex_revisions: vec![],
            next_hex_revision: 0,
            territory_cache: vec![],
            #[cfg(debug_assertions)]
            tick_accumulator: None,
            game_log: None,
        }
    }

    #[test]
    fn entity_creation_with_components() {
        let mut state = make_empty_state();
        let entity = Entity {
            id: 0,
            pos: Some(super::super::hex::Axial { q: 0, r: 0 }),
            owner: Some(0),
            contained_in: None,
            contains: vec![],
            person: Some(Person {
                health: 1.0,
                combat_skill: 0.6,
                role: Role::Soldier,
            }),
            mobile: Some(Mobile {
                speed: 1.0,
                move_cooldown: 0,
                destination: None,
                route: vec![],
            }),
            vision: Some(Vision { radius: 5 }),
            combatant: Some(Combatant {
                engaged_with: vec![],
                facing: 0.0,
            }),
            resource: None,
            structure: None,
        };
        let key = state.spawn_entity(entity);
        let e = state.entities.get(key).unwrap();
        assert!(e.person.is_some());
        assert!(e.mobile.is_some());
        assert!(e.combatant.is_some());
        assert!(e.structure.is_none());
        assert_eq!(e.id, 0);
        assert_eq!(state.next_entity_id, 1);
    }

    #[test]
    fn containment_works() {
        let mut state = make_empty_state();
        let settlement_key = state.spawn_entity(Entity {
            id: 0,
            pos: Some(super::super::hex::Axial { q: 1, r: 1 }),
            owner: Some(0),
            contained_in: None,
            contains: vec![],
            person: None,
            mobile: None,
            vision: None,
            combatant: None,
            resource: None,
            structure: Some(Structure {
                structure_type: StructureType::Village,
                build_progress: 1.0,
                health: 1.0,
                capacity: 100,
            }),
        });
        let person_key = state.spawn_entity(Entity {
            id: 0,
            pos: None,
            owner: Some(0),
            contained_in: Some(settlement_key),
            contains: vec![],
            person: Some(Person {
                health: 1.0,
                combat_skill: 0.1,
                role: Role::Idle,
            }),
            mobile: Some(Mobile {
                speed: 1.0,
                move_cooldown: 0,
                destination: None,
                route: vec![],
            }),
            vision: Some(Vision { radius: 3 }),
            combatant: None,
            resource: None,
            structure: None,
        });
        state
            .entities
            .get_mut(settlement_key)
            .unwrap()
            .contains
            .push(person_key);

        let settlement = state.entities.get(settlement_key).unwrap();
        assert_eq!(settlement.contains.len(), 1);
        assert_eq!(settlement.contains[0], person_key);

        let person = state.entities.get(person_key).unwrap();
        assert_eq!(person.contained_in, Some(settlement_key));
    }

    #[test]
    fn query_methods_return_correct_subsets() {
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 7,
        });

        let units: Vec<_> = state.entity_units().collect();
        let structures: Vec<_> = state.entity_structures().collect();

        // All entity_units have all three components
        for (_, e) in &units {
            assert!(e.person.is_some());
            assert!(e.mobile.is_some());
            assert!(e.combatant.is_some());
        }
        // All entity_structures have structure component
        for (_, e) in &structures {
            assert!(e.structure.is_some());
        }
        // Structures are not units
        for (k, _) in &structures {
            assert!(!units.iter().any(|(uk, _)| uk == k));
        }
    }

    #[test]
    fn mapgen_produces_valid_entities() {
        use super::super::INITIAL_UNITS;
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });

        let settlement_count = state.entity_structures().count();
        assert_eq!(settlement_count, 2, "expected 2 settlement entities");

        let soldier_count = state.entity_units().count();
        // (INITIAL_UNITS + 1) soldiers per player
        assert_eq!(
            soldier_count,
            (INITIAL_UNITS + 1) * 2,
            "expected {} soldiers",
            (INITIAL_UNITS + 1) * 2
        );

        // population persons (28 per player): Idle 20 + Farmer 5 + Worker 3
        let person_only: Vec<_> = state
            .entities
            .iter()
            .filter(|(_, e)| e.person.is_some() && e.combatant.is_none())
            .collect();
        assert_eq!(
            person_only.len(),
            28 * 2,
            "expected {} population persons",
            28 * 2
        );
    }

    #[test]
    fn mapgen_containment_valid() {
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });

        for (skey, settlement) in state.entity_structures() {
            // Every key in contains must point back to this settlement
            for &child_key in &settlement.contains {
                let child = state
                    .entities
                    .get(child_key)
                    .expect("child entity must exist");
                assert_eq!(
                    child.contained_in,
                    Some(skey),
                    "child entity does not point back to settlement"
                );
            }
        }

        // Every population person must be contained in some settlement
        for (_, entity) in state
            .entities
            .iter()
            .filter(|(_, e)| e.person.is_some() && e.combatant.is_none())
        {
            assert!(
                entity.contained_in.is_some(),
                "population person has no container"
            );
            let container_key = entity.contained_in.unwrap();
            let container = state
                .entities
                .get(container_key)
                .expect("container must exist");
            assert!(
                container.structure.is_some(),
                "container must be a structure"
            );
        }
    }
}
