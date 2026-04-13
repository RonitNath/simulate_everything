use serde::{Deserialize, Serialize};

use super::agent::Agent;
use super::hex::Axial;
use super::mapgen::{MapConfig, generate};
use super::observation;
use super::sim;
use super::state::{Biome, Cell, GameState, Player, Unit};
use super::{AGENT_POLL_INTERVAL, directive};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitSnapshot {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub strength: f32,
    pub engaged: bool,
    pub is_general: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub tick: u64,
    pub units: Vec<UnitSnapshot>,
    pub player_food: Vec<f32>,
    pub player_material: Vec<f32>,
    pub alive: Vec<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Replay {
    pub width: usize,
    pub height: usize,
    pub terrain: Vec<f32>,
    pub material_map: Vec<f32>,
    pub num_players: usize,
    pub agent_names: Vec<String>,
    pub frames: Vec<Frame>,
    pub winner: Option<u8>,
}

fn snapshot_units(state: &GameState) -> Vec<UnitSnapshot> {
    state
        .units
        .iter()
        .map(|u| UnitSnapshot {
            id: u.id,
            owner: u.owner,
            q: u.pos.q,
            r: u.pos.r,
            strength: u.strength,
            engaged: !u.engagements.is_empty(),
            is_general: u.is_general,
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
    let mut state = generate(config);
    let agent_names: Vec<String> = agents.iter().map(|a| a.name().to_string()).collect();
    let terrain: Vec<f32> = state.grid.iter().map(|c| c.terrain_value).collect();
    let material_map: Vec<f32> = state.grid.iter().map(|c| c.material_value).collect();

    let mut frames = vec![capture_frame(&state)];

    while state.tick < max_ticks && !sim::is_over(&state) {
        if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
            for (player_id, agent) in agents.iter_mut().enumerate() {
                let pid = player_id as u8;
                if !state.players.iter().any(|p| p.id == pid && p.alive) {
                    continue;
                }
                let obs = observation::observe(&state, pid);
                let directives = agent.act(&obs);
                directive::apply_directives(&mut state, pid, &directives);
            }
        }

        sim::tick(&mut state);

        if state.tick % sample_interval == 0 {
            frames.push(capture_frame(&state));
        }
    }

    // Always capture the final state
    if frames.last().map_or(true, |f| f.tick != state.tick) {
        frames.push(capture_frame(&state));
    }

    Replay {
        width: state.width,
        height: state.height,
        terrain,
        material_map,
        num_players: config.num_players as usize,
        agent_names,
        frames,
        winner: sim::winner(&state),
    }
}

/// Reconstruct a GameState from a Replay frame (for rendering).
///
/// Engagement details, cooldowns, and destinations are not preserved in snapshots,
/// so the reconstructed state is only suitable for display (e.g. ASCII rendering).
pub fn reconstruct_state(replay: &Replay, frame: &Frame) -> GameState {
    let grid: Vec<Cell> = replay
        .terrain
        .iter()
        .zip(replay.material_map.iter())
        .map(|(&terrain_value, &material_value)| Cell {
            terrain_value,
            material_value,
            food_stockpile: 0.0,
            material_stockpile: 0.0,
            has_depot: false,
            road_level: 0,
            height: 0.5,
            moisture: 0.5,
            biome: Biome::Grassland,
            is_river: false,
            water_access: 0.0,
            region_id: 0,
            stockpile_owner: None,
        })
        .collect();

    let units: Vec<Unit> = frame
        .units
        .iter()
        .map(|s| Unit {
            id: s.id,
            owner: s.owner,
            pos: Axial::new(s.q, s.r),
            strength: s.strength,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: s.is_general,
        })
        .collect();

    let players: Vec<Player> = frame
        .alive
        .iter()
        .enumerate()
        .map(|(i, &alive)| Player {
            id: i as u8,
            food: frame.player_food.get(i).copied().unwrap_or(0.0),
            material: frame.player_material.get(i).copied().unwrap_or(0.0),
            general_id: units
                .iter()
                .find(|u| u.owner == i as u8 && u.is_general)
                .map(|u| u.id)
                .unwrap_or(0),
            alive,
        })
        .collect();

    GameState {
        width: replay.width,
        height: replay.height,
        grid,
        units,
        players,
        population: Vec::new(),
        convoys: Vec::new(),
        regions: Vec::new(),
        tick: frame.tick,
        next_unit_id: 0,
        next_pop_id: 0,
        next_convoy_id: 0,
    }
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
    fn reconstruct_state_roundtrips() {
        let config = test_config();
        let mut agents = test_agents();
        let replay = record_game(&config, &mut agents, 100, 10);
        let frame = &replay.frames[replay.frames.len() / 2];
        let state = reconstruct_state(&replay, frame);
        assert_eq!(state.width, replay.width);
        assert_eq!(state.height, replay.height);
        assert_eq!(state.tick, frame.tick);
        assert_eq!(state.units.len(), frame.units.len());
    }
}
