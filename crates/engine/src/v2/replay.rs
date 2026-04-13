use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

use super::agent::Agent;
use super::hex::Axial;
use super::mapgen::{MapConfig, generate};
use super::runner;
use super::sim;
use super::spatial::SpatialIndex;
use super::state::{
    Biome, CargoType, Cell, Convoy, Engagement, GameState, Player, Population, Role, Unit,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitSnapshot {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub strength: f32,
    pub engagements: Vec<Engagement>,
    pub move_cooldown: u8,
    pub destination: Option<Axial>,
    pub engaged: bool,
    pub is_general: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopulationSnapshot {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub count: u16,
    pub role: Role,
    pub training: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvoySnapshot {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub origin: Axial,
    pub destination: Axial,
    pub cargo_type: CargoType,
    pub cargo_amount: f32,
    pub capacity: f32,
    pub speed: f32,
    pub move_cooldown: u8,
    pub returning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellSnapshot {
    pub food_stockpile: f32,
    pub material_stockpile: f32,
    pub stockpile_owner: Option<u8>,
    pub road_level: u8,
    pub has_depot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticCellSnapshot {
    pub terrain_value: f32,
    pub material_value: f32,
    pub height: f32,
    pub moisture: f32,
    pub biome: Biome,
    pub is_river: bool,
    pub water_access: f32,
    pub region_id: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub tick: u64,
    pub units: Vec<UnitSnapshot>,
    pub player_food: Vec<f32>,
    pub player_material: Vec<f32>,
    pub alive: Vec<bool>,
    pub cells: Vec<CellSnapshot>,
    pub population: Vec<PopulationSnapshot>,
    pub convoys: Vec<ConvoySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replay {
    pub width: usize,
    pub height: usize,
    pub terrain: Vec<f32>,
    pub material_map: Vec<f32>,
    pub static_cells: Vec<StaticCellSnapshot>,
    pub num_players: usize,
    pub agent_names: Vec<String>,
    pub frames: Vec<Frame>,
    pub winner: Option<u8>,
    pub timed_out: bool,
}

fn snapshot_units(state: &GameState) -> Vec<UnitSnapshot> {
    state
        .units
        .values()
        .map(|u| UnitSnapshot {
            id: u.public_id,
            owner: u.owner,
            q: u.pos.q,
            r: u.pos.r,
            strength: u.strength,
            engagements: u.engagements.clone(),
            move_cooldown: u.move_cooldown,
            destination: u.destination,
            engaged: !u.engagements.is_empty(),
            is_general: u.is_general,
        })
        .collect()
}

fn snapshot_population(state: &GameState) -> Vec<PopulationSnapshot> {
    state
        .population
        .values()
        .map(|p| PopulationSnapshot {
            id: p.public_id,
            owner: p.owner,
            q: p.hex.q,
            r: p.hex.r,
            count: p.count,
            role: p.role,
            training: p.training,
        })
        .collect()
}

fn snapshot_convoys(state: &GameState) -> Vec<ConvoySnapshot> {
    state
        .convoys
        .values()
        .map(|c| ConvoySnapshot {
            id: c.public_id,
            owner: c.owner,
            q: c.pos.q,
            r: c.pos.r,
            origin: c.origin,
            destination: c.destination,
            cargo_type: c.cargo_type,
            cargo_amount: c.cargo_amount,
            capacity: c.capacity,
            speed: c.speed,
            move_cooldown: c.move_cooldown,
            returning: c.returning,
        })
        .collect()
}

fn snapshot_cells(state: &GameState) -> Vec<CellSnapshot> {
    state
        .grid
        .iter()
        .map(|cell| CellSnapshot {
            food_stockpile: cell.food_stockpile,
            material_stockpile: cell.material_stockpile,
            stockpile_owner: cell.stockpile_owner,
            road_level: cell.road_level,
            has_depot: cell.has_depot,
        })
        .collect()
}

fn snapshot_static_cells(state: &GameState) -> Vec<StaticCellSnapshot> {
    state
        .grid
        .iter()
        .map(|cell| StaticCellSnapshot {
            terrain_value: cell.terrain_value,
            material_value: cell.material_value,
            height: cell.height,
            moisture: cell.moisture,
            biome: cell.biome,
            is_river: cell.is_river,
            water_access: cell.water_access,
            region_id: cell.region_id,
        })
        .collect()
}

fn capture_frame(state: &GameState) -> Frame {
    Frame {
        tick: state.tick,
        units: snapshot_units(state),
        player_food: state.players.iter().map(|p| p.food).collect(),
        player_material: state.players.iter().map(|p| p.material).collect(),
        alive: state.players.iter().map(|p| p.alive).collect(),
        cells: snapshot_cells(state),
        population: snapshot_population(state),
        convoys: snapshot_convoys(state),
    }
}

fn build_replay(
    state: &GameState,
    agent_names: Vec<String>,
    frames: Vec<Frame>,
    tick_limit: u64,
) -> Replay {
    Replay {
        width: state.width,
        height: state.height,
        terrain: state.grid.iter().map(|c| c.terrain_value).collect(),
        material_map: state.grid.iter().map(|c| c.material_value).collect(),
        static_cells: snapshot_static_cells(state),
        num_players: state.players.len(),
        agent_names,
        frames,
        winner: sim::winner_at_limit(state, tick_limit),
        timed_out: sim::reached_timeout(state, tick_limit),
    }
}

/// Run a full game and capture periodic snapshots into a Replay.
///
/// `sample_interval` controls how often frames are captured (in ticks).
/// Tick 0 and the final tick are always captured.
pub fn record_game(
    config: &MapConfig,
    agents: &mut [Box<dyn Agent>],
    max_ticks: u64,
    sample_interval: u64,
) -> Replay {
    record_game_with_final_state(config, agents, max_ticks, sample_interval).0
}

pub(crate) fn record_game_with_final_state(
    config: &MapConfig,
    agents: &mut [Box<dyn Agent>],
    max_ticks: u64,
    sample_interval: u64,
) -> (Replay, GameState) {
    let mut state = generate(config);
    let tick_limit = sim::timeout_limit(max_ticks);
    let agent_names: Vec<String> = agents.iter().map(|a| a.name().to_string()).collect();
    let mut frames = vec![capture_frame(&state)];

    runner::run_loop(&mut state, agents, tick_limit, |state| {
        if state.tick % sample_interval == 0 {
            frames.push(capture_frame(state));
        }
    });

    if frames.last().is_none_or(|frame| frame.tick != state.tick) {
        frames.push(capture_frame(&state));
    }

    let replay = build_replay(&state, agent_names, frames, tick_limit);
    (replay, state)
}

/// Reconstruct a GameState from a Replay frame.
pub fn reconstruct_state(replay: &Replay, frame: &Frame) -> GameState {
    let grid: Vec<Cell> = replay
        .static_cells
        .iter()
        .zip(frame.cells.iter())
        .map(|(static_cell, dynamic_cell)| Cell {
            terrain_value: static_cell.terrain_value,
            material_value: static_cell.material_value,
            food_stockpile: dynamic_cell.food_stockpile,
            material_stockpile: dynamic_cell.material_stockpile,
            has_depot: dynamic_cell.has_depot,
            road_level: dynamic_cell.road_level,
            height: static_cell.height,
            moisture: static_cell.moisture,
            biome: static_cell.biome,
            is_river: static_cell.is_river,
            water_access: static_cell.water_access,
            region_id: static_cell.region_id,
            stockpile_owner: dynamic_cell.stockpile_owner,
        })
        .collect();

    let mut units = SlotMap::with_key();
    let mut general_keys = vec![None; replay.num_players];
    let mut next_unit_id = 0;
    for s in &frame.units {
        next_unit_id = next_unit_id.max(s.id + 1);
        let key = units.insert(Unit {
            public_id: s.id,
            owner: s.owner,
            pos: Axial::new(s.q, s.r),
            strength: s.strength,
            move_cooldown: s.move_cooldown,
            engagements: Vec::new(),
            destination: s.destination,
            is_general: s.is_general,
        });
        if s.is_general {
            general_keys[s.owner as usize] = Some(key);
        }
    }

    let players: Vec<Player> = frame
        .alive
        .iter()
        .enumerate()
        .map(|(i, &alive)| Player {
            id: i as u8,
            food: frame.player_food.get(i).copied().unwrap_or(0.0),
            material: frame.player_material.get(i).copied().unwrap_or(0.0),
            general_id: general_keys[i].expect("replay frame missing general"),
            alive,
        })
        .collect();

    let mut population = SlotMap::with_key();
    let mut next_pop_id = 0;
    for snapshot in &frame.population {
        next_pop_id = next_pop_id.max(snapshot.id + 1);
        population.insert(Population {
            public_id: snapshot.id,
            hex: Axial::new(snapshot.q, snapshot.r),
            owner: snapshot.owner,
            count: snapshot.count,
            role: snapshot.role,
            training: snapshot.training,
        });
    }

    let mut convoys = SlotMap::with_key();
    let mut next_convoy_id = 0;
    for snapshot in &frame.convoys {
        next_convoy_id = next_convoy_id.max(snapshot.id + 1);
        convoys.insert(Convoy {
            public_id: snapshot.id,
            owner: snapshot.owner,
            pos: Axial::new(snapshot.q, snapshot.r),
            origin: snapshot.origin,
            destination: snapshot.destination,
            cargo_type: snapshot.cargo_type,
            cargo_amount: snapshot.cargo_amount,
            capacity: snapshot.capacity,
            speed: snapshot.speed,
            move_cooldown: snapshot.move_cooldown,
            returning: snapshot.returning,
        });
    }

    let mut state = GameState {
        width: replay.width,
        height: replay.height,
        grid,
        units,
        players,
        population,
        convoys,
        regions: Vec::new(),
        tick: frame.tick,
        next_unit_id,
        next_pop_id,
        next_convoy_id,
        scouted: vec![vec![true; replay.width * replay.height]; replay.num_players],
        spatial: SpatialIndex::new(replay.width, replay.height),
    };
    state.rebuild_spatial();
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::agent::SpreadAgent;

    fn test_config() -> MapConfig {
        MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        }
    }

    fn test_agents() -> Vec<Box<dyn Agent>> {
        vec![Box::new(SpreadAgent::new()), Box::new(SpreadAgent::new())]
    }

    #[test]
    fn record_game_returns_valid_replay() {
        let config = test_config();
        let mut agents = test_agents();
        let replay = record_game(&config, &mut agents, 500, 10);
        assert_eq!(replay.width, 20);
        assert_eq!(replay.height, 20);
        assert_eq!(replay.num_players, 2);
        assert_eq!(replay.terrain.len(), 20 * 20);
        assert_eq!(replay.material_map.len(), 20 * 20);
        assert_eq!(replay.static_cells.len(), 20 * 20);
    }

    #[test]
    fn record_game_captures_multiple_frames() {
        let config = test_config();
        let mut agents = test_agents();
        let replay = record_game(&config, &mut agents, 500, 10);
        assert!(replay.frames.len() > 1, "should have multiple frames");
    }

    #[test]
    fn record_game_first_frame_is_tick_zero() {
        let config = test_config();
        let mut agents = test_agents();
        let replay = record_game(&config, &mut agents, 500, 10);
        assert_eq!(replay.frames[0].tick, 0);
    }

    #[test]
    fn record_game_ticks_monotonically_increase() {
        let config = test_config();
        let mut agents = test_agents();
        let replay = record_game(&config, &mut agents, 500, 10);
        for window in replay.frames.windows(2) {
            assert!(
                window[1].tick > window[0].tick,
                "ticks not monotonic: {} followed by {}",
                window[0].tick,
                window[1].tick
            );
        }
    }

    #[test]
    fn record_game_agent_names_match() {
        let config = test_config();
        let mut agents = test_agents();
        let replay = record_game(&config, &mut agents, 500, 10);
        assert_eq!(replay.agent_names.len(), 2);
        assert_eq!(replay.agent_names[0], "spread");
        assert_eq!(replay.agent_names[1], "spread");
    }

    #[test]
    fn record_game_sets_winner() {
        let config = MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        };
        let mut agents = test_agents();
        let replay = record_game(&config, &mut agents, 5000, 10);
        assert!(
            replay.frames.len() > 10,
            "game should progress meaningfully"
        );
    }

    #[test]
    fn record_game_respects_timeout_limit() {
        let config = test_config();
        let mut agents = test_agents();
        let replay = record_game(&config, &mut agents, 10_000, 10);
        assert!(replay.frames.last().unwrap().tick <= crate::v2::TIMEOUT_TICKS);
        if replay.frames.last().unwrap().tick == crate::v2::TIMEOUT_TICKS {
            assert!(replay.timed_out);
        }
    }

    #[test]
    fn reconstruct_state_roundtrips() {
        let config = test_config();
        let mut agents = test_agents();
        let (replay, final_state) = record_game_with_final_state(&config, &mut agents, 100, 10);
        let frame = replay.frames.last().unwrap();
        let state = reconstruct_state(&replay, frame);
        assert_eq!(state.width, replay.width);
        assert_eq!(state.height, replay.height);
        assert_eq!(state.tick, frame.tick);
        assert_eq!(state.units.len(), final_state.units.len());
        assert_eq!(state.population.len(), final_state.population.len());
        assert_eq!(state.convoys.len(), final_state.convoys.len());
    }
}
