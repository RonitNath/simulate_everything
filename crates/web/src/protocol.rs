use simulate_everything_engine::action::Action;
use simulate_everything_engine::agent::Observation;
use simulate_everything_engine::replay::Frame;
use serde::{Deserialize, Serialize};

// === Messages FROM spectators TO server ===

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SpectatorToServer {
    /// Set the tick speed.
    #[serde(rename = "set_speed")]
    SetSpeed { tick_ms: u64 },
}

// === Messages FROM server TO agents ===

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerToAgent {
    /// Agent has joined the lobby, waiting for more players.
    #[serde(rename = "lobby")]
    Lobby {
        slot: u8,
        name: String,
        players_connected: u8,
        players_needed: u8,
    },
    /// Game is starting.
    #[serde(rename = "game_start")]
    GameStart {
        player: u8,
        width: usize,
        height: usize,
        num_players: u8,
    },
    /// Your turn — here's what you can see.
    #[serde(rename = "observation")]
    Observation {
        #[serde(flatten)]
        obs: Observation,
    },
    /// Game over.
    #[serde(rename = "game_end")]
    GameEnd { winner: Option<u8>, turns: u32 },
    /// Error (invalid name, lobby full, etc).
    #[serde(rename = "error")]
    Error { message: String },
}

// === Messages FROM agents TO server ===

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum AgentToServer {
    /// Agent identifies itself on connect.
    #[serde(rename = "join")]
    Join { name: String },
    /// Agent submits actions for this turn.
    #[serde(rename = "actions")]
    Actions { actions: Vec<Action> },
}

// === Messages FROM server TO spectators ===

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerToSpectator {
    /// Lobby state update.
    #[serde(rename = "lobby")]
    Lobby {
        players: Vec<LobbyPlayer>,
        players_needed: u8,
    },
    /// Game started.
    #[serde(rename = "game_start")]
    GameStart {
        width: usize,
        height: usize,
        num_players: u8,
        agent_names: Vec<String>,
    },
    /// A new frame (turn snapshot).
    #[serde(rename = "frame")]
    Frame {
        #[serde(flatten)]
        frame: Frame,
        /// Per-player agent compute time in microseconds.
        compute_us: Vec<u64>,
    },
    /// Game over.
    #[serde(rename = "game_end")]
    GameEnd { winner: Option<u8>, turns: u32 },
    /// Display config update (pushed from REST API).
    #[serde(rename = "config")]
    Config {
        #[serde(skip_serializing_if = "Option::is_none")]
        show_numbers: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tick_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct LobbyPlayer {
    pub slot: u8,
    pub name: String,
}
