use serde::{Deserialize, Serialize};

use crate::entity::*;
use crate::init::V3Init;

// ---------------------------------------------------------------------------
// Snapshot — full state per tick
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Snapshot {
    pub tick: u64,
    pub dt: f32,
    pub full_state: bool,
    pub entities: Vec<SpectatorEntityInfo>,
    pub projectiles: Vec<ProjectileInfo>,
    pub stacks: Vec<StackInfo>,
    pub hex_ownership: Vec<Option<u8>>,
    pub hex_roads: Vec<u8>,
    pub hex_structures: Vec<Option<u32>>,
    pub players: Vec<PlayerInfo>,
}

// ---------------------------------------------------------------------------
// SnapshotDelta — per-tick update
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3SnapshotDelta {
    pub tick: u64,
    pub dt: f32,
    pub full_state: bool,
    pub entities_appeared: Vec<SpectatorEntityInfo>,
    pub entities_updated: Vec<EntityUpdate>,
    pub entities_removed: Vec<u32>,
    pub projectiles_spawned: Vec<ProjectileInfo>,
    pub projectiles_removed: Vec<u32>,
    pub stacks_created: Vec<StackInfo>,
    pub stacks_updated: Vec<StackUpdate>,
    pub stacks_dissolved: Vec<u32>,
    pub hex_changes: Vec<HexDelta>,
    pub players: Vec<PlayerInfo>,
}

// ---------------------------------------------------------------------------
// RR status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3RrStatus {
    pub game_number: u64,
    pub current_tick: u64,
    pub dt: f32,
    pub mode: TimeMode,
    pub paused: bool,
    pub tick_ms: u64,
    pub autoplay: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capturable_start_tick: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capturable_end_tick: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_capture: Option<String>,
}

// ---------------------------------------------------------------------------
// Server-to-spectator envelope
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum V3ServerToSpectator {
    #[serde(rename = "v3_init")]
    Init {
        #[serde(flatten)]
        init: V3Init,
        game_number: u64,
    },
    #[serde(rename = "v3_snapshot")]
    Snapshot {
        #[serde(flatten)]
        snapshot: V3Snapshot,
    },
    #[serde(rename = "v3_snapshot_delta")]
    SnapshotDelta {
        #[serde(flatten)]
        delta: V3SnapshotDelta,
    },
    #[serde(rename = "v3_game_end")]
    GameEnd {
        winner: Option<u8>,
        tick: u64,
        timed_out: bool,
        scores: Vec<u32>,
    },
    #[serde(rename = "v3_config")]
    Config {
        #[serde(skip_serializing_if = "Option::is_none")]
        tick_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<TimeMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        autoplay: Option<bool>,
    },
    #[serde(rename = "v3_rr_status")]
    RrStatus(V3RrStatus),
}
