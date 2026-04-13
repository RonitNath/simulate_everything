use super::SETTLEMENT_THRESHOLD;
use super::hex::{Axial, axial_to_offset};
use super::spatial::SpatialIndex;
use serde::{Deserialize, Serialize};
use slotmap::{SlotMap, new_key_type};

new_key_type! {
    pub struct UnitKey;
    pub struct PopKey;
    pub struct ConvoyKey;
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
    pub is_general: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Idle,
    Farmer,
    Worker,
    Soldier,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: u8,
    pub food: f32,
    pub material: f32,
    pub general_id: UnitKey,
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
    pub regions: Vec<Region>,
    pub tick: u64,
    /// Monotonically increasing counters for frontend/replay-facing IDs.
    pub next_unit_id: u32,
    pub next_pop_id: u32,
    pub next_convoy_id: u32,
    pub scouted: Vec<Vec<bool>>,
    #[serde(skip)]
    pub spatial: SpatialIndex,
}

impl GameState {
    pub fn rebuild_spatial(&mut self) {
        self.spatial.rebuild(&self.units);
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

    pub fn is_settlement(&self, owner: u8, ax: Axial) -> bool {
        self.population_on_hex(owner, ax) >= SETTLEMENT_THRESHOLD
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

    pub fn general_pos(&self, player_id: u8) -> Option<Axial> {
        let general_id = self.players.iter().find(|p| p.id == player_id)?.general_id;
        Some(self.units.get(general_id)?.pos)
    }

    pub fn has_unit_at(&self, ax: Axial) -> bool {
        self.spatial.has_unit_at(ax)
    }
}
