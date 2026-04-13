use serde::Serialize;
use simulate_everything_engine::v2::replay::UnitSnapshot;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum V2ServerToSpectator {
    #[serde(rename = "v2_game_start")]
    GameStart {
        width: usize,
        height: usize,
        terrain: Vec<f32>,
        material_map: Vec<f32>,
        num_players: u8,
        agent_names: Vec<String>,
    },
    #[serde(rename = "v2_frame")]
    Frame {
        tick: u64,
        units: Vec<UnitSnapshot>,
        player_food: Vec<f32>,
        player_material: Vec<f32>,
        alive: Vec<bool>,
    },
    #[serde(rename = "v2_game_end")]
    GameEnd { winner: Option<u8>, tick: u64 },
    #[serde(rename = "v2_config")]
    Config {
        #[serde(skip_serializing_if = "Option::is_none")]
        tick_ms: Option<u64>,
    },
}
