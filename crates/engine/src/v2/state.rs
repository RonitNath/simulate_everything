use super::hex::{Axial, axial_to_offset};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Cell {
    pub terrain_value: f32,
    pub material_value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engagement {
    pub enemy_id: u32,
    pub edge: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unit {
    pub id: u32,
    pub owner: u8,
    pub pos: Axial,
    pub strength: f32,
    pub move_cooldown: u8,
    pub engagements: Vec<Engagement>,
    pub destination: Option<Axial>,
    pub is_general: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub id: u8,
    pub food: f32,
    pub material: f32,
    pub general_id: u32,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub width: usize,
    pub height: usize,
    /// Row-major grid in offset coordinates.
    pub grid: Vec<Cell>,
    pub units: Vec<Unit>,
    pub players: Vec<Player>,
    pub tick: u64,
    /// Monotonically increasing counter for assigning unique unit IDs.
    pub next_unit_id: u32,
}

impl GameState {
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.grid[row * self.width + col]
    }

    pub fn cell_at(&self, ax: Axial) -> Option<&Cell> {
        let (row, col) = axial_to_offset(ax);
        if row < 0 || col < 0 {
            return None;
        }
        let (row, col) = (row as usize, col as usize);
        if row < self.height && col < self.width {
            Some(&self.grid[row * self.width + col])
        } else {
            None
        }
    }

    pub fn in_bounds(&self, ax: Axial) -> bool {
        let (row, col) = axial_to_offset(ax);
        row >= 0 && col >= 0 && (row as usize) < self.height && (col as usize) < self.width
    }
}
