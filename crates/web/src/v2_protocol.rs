use serde::Serialize;
use simulate_everything_engine::v2::spectator::{SpectatorInit, SpectatorSnapshot};

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
}
