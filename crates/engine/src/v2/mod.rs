pub mod ascii;
pub mod hex;
pub mod mapgen;
pub mod pathfinding;
pub mod sim;
pub mod state;

pub const RESOURCE_RATE: f32 = 0.1;
pub const UNIT_COST: f32 = 10.0;
pub const INITIAL_STRENGTH: f32 = 100.0;
pub const DAMAGE_RATE: f32 = 0.01;
pub const DISENGAGE_PENALTY: f32 = 0.5;
pub const BASE_MOVE_COOLDOWN: u8 = 3;
pub const TERRAIN_MOVE_PENALTY: f32 = 0.5;
pub const VISION_RADIUS: i32 = 3;
pub const INITIAL_UNITS: usize = 5;
pub const TICKS_PER_SECOND: u32 = 10;
pub const AGENT_POLL_INTERVAL: u32 = 5;
