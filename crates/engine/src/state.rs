use serde::{Deserialize, Serialize};

/// What terrain occupies a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tile {
    Empty,
    Mountain,
    City,
    General,
}

/// A single board cell.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Cell {
    pub tile: Tile,
    /// Which player owns this cell, if any.
    pub owner: Option<u8>,
    /// Army count on this cell. Cities/generals start with a garrison.
    pub armies: i32,
}

impl Cell {
    pub fn empty() -> Self {
        Self {
            tile: Tile::Empty,
            owner: None,
            armies: 0,
        }
    }

    pub fn mountain() -> Self {
        Self {
            tile: Tile::Mountain,
            owner: None,
            armies: 0,
        }
    }

    pub fn city(armies: i32) -> Self {
        Self {
            tile: Tile::City,
            owner: None,
            armies,
        }
    }

    pub fn general(player: u8) -> Self {
        Self {
            tile: Tile::General,
            owner: Some(player),
            armies: 1,
        }
    }
}

/// Full game state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub width: usize,
    pub height: usize,
    /// Row-major grid: index = row * width + col.
    pub grid: Vec<Cell>,
    /// Number of players.
    pub num_players: u8,
    /// Position of each player's general: (row, col). Index = player id.
    pub general_positions: Vec<(usize, usize)>,
    /// Which players are still alive.
    pub alive: Vec<bool>,
    /// Current turn number (0-indexed).
    pub turn: u32,
    /// Winner, if the game is over.
    pub winner: Option<u8>,
}

impl GameState {
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.grid[row * self.width + col]
    }

    pub fn cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
        &mut self.grid[row * self.width + col]
    }

    pub fn in_bounds(&self, row: i32, col: i32) -> bool {
        row >= 0 && col >= 0 && (row as usize) < self.height && (col as usize) < self.width
    }

    /// Count cells owned by a player.
    pub fn land_count(&self, player: u8) -> usize {
        self.grid.iter().filter(|c| c.owner == Some(player)).count()
    }

    /// Sum armies owned by a player.
    pub fn army_count(&self, player: u8) -> i32 {
        self.grid
            .iter()
            .filter(|c| c.owner == Some(player))
            .map(|c| c.armies)
            .sum()
    }

    /// Number of players still alive.
    pub fn alive_count(&self) -> usize {
        self.alive.iter().filter(|&&a| a).count()
    }
}
