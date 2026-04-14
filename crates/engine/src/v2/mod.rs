pub mod agent;
pub mod agent_layers;
pub mod ascii;
pub mod city_ai;
pub mod combat;
pub mod directive;
pub mod gamelog;
pub mod hex;
pub mod mapgen;
pub mod observation;
pub mod operations;
pub mod pathfinding;
pub mod replay;
pub mod runner;
pub mod sim;
pub mod spatial;
pub mod spectator;
pub mod state;
pub mod vision;

#[cfg(test)]
mod integration_tests;

pub const FOOD_RATE: f32 = 0.2;
pub const MATERIAL_RATE: f32 = 0.1;
pub const UNIT_FOOD_COST: f32 = 8.0;
pub const UNIT_MATERIAL_COST: f32 = 5.0;
pub const UPKEEP_PER_UNIT: f32 = 0.02;
pub const STARVATION_DAMAGE: f32 = 0.5;
pub const FARMER_RATE: f32 = 0.1;
pub const WORKER_RATE: f32 = 0.08;
pub const TRAINING_RATE: f32 = 0.15;
pub const SOLDIER_READY_THRESHOLD: f32 = 1.0;
pub const SOLDIERS_PER_UNIT: u16 = 5;
pub const TRAIN_BATCH_SIZE: u16 = 5;
pub const SOLDIER_EQUIP_COST: f32 = 1.0;
pub const BASE_STORAGE_CAP: f32 = 50.0;
pub const DEPOT_STORAGE_CAP: f32 = 200.0;
pub const DEPOT_BUILD_COST: f32 = 20.0;
pub const ROAD_LEVEL2_COST: f32 = 10.0;
pub const ROAD_LEVEL3_COST: f32 = 20.0;
pub const CONVOY_CAPACITY: f32 = 20.0;
pub const CONVOY_MOVE_COOLDOWN: u8 = 3;
pub const POPULATION_GROWTH_RATE: f32 = 0.02;
/// Population threshold for a hex to count as any settlement (Farm).
pub const FARM_THRESHOLD: u16 = 2;
/// Village tier: 10+ population.
pub const VILLAGE_THRESHOLD: u16 = 10;
/// City tier: 30+ population.
pub const CITY_THRESHOLD: u16 = 30;
/// Territory radius for Farm settlements (own hex only).
pub const FARM_RADIUS: i32 = 0;
/// Territory radius for Village settlements.
pub const VILLAGE_RADIUS: i32 = 1;
/// Territory radius for City settlements.
pub const CITY_RADIUS: i32 = 2;
/// How often (in ticks) the city AI runs.
pub const CITY_AI_INTERVAL: u64 = 5;
/// Vision radius for Farm settlements.
pub const FARM_VISION: i32 = 2;
/// Vision radius for Village settlements.
pub const VILLAGE_VISION: i32 = 3;
/// Vision radius for City settlements.
pub const CITY_VISION: i32 = 4;
/// Population size of a farm settler convoy.
pub const FARM_CONVOY_SIZE: u16 = 4;
/// Legacy alias kept for existing callers.
pub const SETTLEMENT_THRESHOLD: u16 = VILLAGE_THRESHOLD;
pub const SETTLER_CONVOY_SIZE: u16 = 10;
pub const SETTLEMENT_SUPPORT_RADIUS: i32 = VILLAGE_RADIUS;
pub const FRONTIER_DECAY_RATE: f32 = 0.02;
pub const MIGRATION_DIVISOR: u64 = 120;
pub const TIMEOUT_TICKS: u64 = 3000;
pub const INITIAL_STRENGTH: f32 = 100.0;
pub const DAMAGE_RATE: f32 = 0.05;
pub const DISENGAGE_PENALTY: f32 = 0.3;
/// Units below this strength auto-rout (disengage from all combat).
pub const ROUT_THRESHOLD: f32 = 30.0;
pub const BASE_MOVE_COOLDOWN: u8 = 1;
pub const TERRAIN_MOVE_PENALTY: f32 = 0.5;
pub const VISION_RADIUS: i32 = 5;
pub const INITIAL_UNITS: usize = 5;
pub const TICKS_PER_SECOND: u32 = 10;
pub const AGENT_POLL_INTERVAL: u32 = 5;
pub const MAX_RATIONS: f32 = 50.0;
/// Food consumed per person entity per tick.
pub const PERSON_FOOD_RATE: f32 = 0.005;
/// Build progress per builder entity per tick.
pub const BUILD_RATE: f32 = 0.02;
/// Health lost per person entity per tick when starving.
pub const STARVATION_HEALTH_DAMAGE: f32 = 0.01;
/// Base damage multiplier per attacker entity per tick.
pub const DAMAGE_PER_TICK: f32 = 0.02;
/// Half-width of frontal shield arc in radians (~30 degrees).
pub const SHIELD_ARC_HALF: f32 = std::f32::consts::FRAC_PI_6;
/// Damage modifier when attack hits within shield arc (front).
pub const FRONT_MODIFIER: f32 = 0.3;
/// Damage modifier when attack hits from the side.
pub const SIDE_MODIFIER: f32 = 0.7;
/// Damage modifier when attack hits from the rear.
pub const REAR_MODIFIER: f32 = 1.5;
