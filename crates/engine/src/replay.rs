use crate::event::PlayerStats;
use crate::state::{Cell, GameState};
use serde::Serialize;

/// A compact snapshot of the grid at one turn.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub turn: u32,
    pub grid: Vec<Cell>,
    pub stats: Vec<PlayerStats>,
}

/// Full game replay: metadata + per-turn grid snapshots.
#[derive(Debug, Clone, Serialize)]
pub struct Replay {
    pub width: usize,
    pub height: usize,
    pub num_players: u8,
    pub agent_names: Vec<String>,
    pub frames: Vec<Frame>,
    pub winner: Option<u8>,
}

impl Replay {
    pub fn new(state: &GameState, agent_names: Vec<String>) -> Self {
        let mut replay = Self {
            width: state.width,
            height: state.height,
            num_players: state.num_players,
            agent_names,
            frames: Vec::new(),
            winner: None,
        };
        replay.capture(state);
        replay
    }

    pub fn capture(&mut self, state: &GameState) {
        let stats = (0..state.num_players)
            .map(|p| PlayerStats {
                player: p,
                land: state.land_count(p),
                armies: state.army_count(p),
                alive: state.alive[p as usize],
            })
            .collect();
        self.frames.push(Frame {
            turn: state.turn,
            grid: state.grid.clone(),
            stats,
        });
    }

    pub fn finalize(&mut self, state: &GameState) {
        self.winner = state.winner;
    }
}
