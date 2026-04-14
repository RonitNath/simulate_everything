use serde::Serialize;
use simulate_everything_engine::v2::spectator::{SpectatorInit, SpectatorSnapshot};

use crate::v2_rr_review::ReviewBundleSummary;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum V2ServerToSpectator {
    #[serde(rename = "v2_init")]
    Init {
        #[serde(flatten)]
        init: SpectatorInit,
        game_number: u64,
    },
    #[serde(rename = "v2_snapshot")]
    Snapshot {
        #[serde(flatten)]
        snapshot: SpectatorSnapshot,
    },
    #[serde(rename = "v2_game_end")]
    GameEnd {
        winner: Option<u8>,
        tick: u64,
        timed_out: bool,
    },
    #[serde(rename = "v2_config")]
    Config {
        #[serde(skip_serializing_if = "Option::is_none")]
        tick_ms: Option<u64>,
    },
    #[serde(rename = "v2_rr_status")]
    RrStatus {
        game_number: u64,
        current_tick: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        capturable_start_tick: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        capturable_end_tick: Option<u64>,
        paused: bool,
        tick_ms: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        active_capture: Option<ReviewBundleSummary>,
    },
}
