use crate::action::Action;
use serde::Serialize;

/// Structured events emitted during a game, designed for AI agent consumption.
/// Each event is one JSONL line.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    /// Game started.
    #[serde(rename = "game_start")]
    GameStart {
        width: usize,
        height: usize,
        num_players: u8,
        turn: u32,
    },
    /// A turn was executed.
    #[serde(rename = "turn")]
    Turn {
        turn: u32,
        actions: Vec<PlayerAction>,
        /// Per-player stats after the turn.
        stats: Vec<PlayerStats>,
    },
    /// A player captured another player's general.
    #[serde(rename = "elimination")]
    Elimination { turn: u32, eliminated: u8, by: u8 },
    /// Game over.
    #[serde(rename = "game_end")]
    GameEnd {
        turn: u32,
        winner: Option<u8>,
        stats: Vec<PlayerStats>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerAction {
    pub player: u8,
    pub actions: Vec<Action>,
    /// How many of the actions were valid and executed.
    pub executed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerStats {
    pub player: u8,
    pub land: usize,
    pub armies: i32,
    pub alive: bool,
}
