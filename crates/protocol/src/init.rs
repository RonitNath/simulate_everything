use serde::{Deserialize, Serialize};

/// Sent once on spectator connect — map dimensions, full heightmap, entity list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Init {
    pub width: u32,
    pub height: u32,
    pub terrain: Vec<f32>,
    pub height_map: Vec<f32>,
    pub material_map: Vec<f32>,
    pub region_ids: Vec<u16>,
    pub player_count: u8,
    pub agent_names: Vec<String>,
    pub agent_versions: Vec<String>,
    pub game_number: u64,
}
