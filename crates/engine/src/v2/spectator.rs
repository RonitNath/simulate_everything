use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::hex::Axial;
use super::state::{CargoType, GameState};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorInit {
    pub width: usize,
    pub height: usize,
    pub terrain: Vec<f32>,
    pub material_map: Vec<f32>,
    pub height_map: Vec<f32>,
    pub region_ids: Vec<u16>,
    pub player_count: u8,
    pub agent_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorSnapshot {
    pub tick: u64,
    pub full_state: bool,
    pub units: Vec<SpectatorUnit>,
    pub engagements: Vec<(u32, u32)>,
    pub convoys: Vec<SpectatorConvoy>,
    pub hex_changes: Vec<SpectatorHexDelta>,
    pub settlements: Vec<SpectatorSettlement>,
    pub players: Vec<SpectatorPlayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorUnit {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub strength: f32,
    pub engaged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorConvoy {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub cargo_type: CargoType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorHexDelta {
    pub index: usize,
    pub owner: Option<u8>,
    pub road_level: u8,
    pub has_settlement: bool,
    pub settlement_owner: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorSettlement {
    pub q: i32,
    pub r: i32,
    pub owner: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorPlayer {
    pub id: u8,
    pub alive: bool,
    pub population: u16,
    pub territory: u16,
    pub food_level: u8,
    pub material_level: u8,
}

pub fn spectator_init(state: &GameState, agent_names: Vec<String>) -> SpectatorInit {
    SpectatorInit {
        width: state.width,
        height: state.height,
        terrain: state.grid.iter().map(|c| c.terrain_value).collect(),
        material_map: state.grid.iter().map(|c| c.material_value).collect(),
        height_map: state.grid.iter().map(|c| c.height).collect(),
        region_ids: state.grid.iter().map(|c| c.region_id).collect(),
        player_count: state.players.len() as u8,
        agent_names,
    }
}

pub fn snapshot(state: &GameState) -> SpectatorSnapshot {
    build_snapshot(state, true)
}

pub fn snapshot_delta(state: &GameState) -> SpectatorSnapshot {
    build_snapshot(state, false)
}

fn build_snapshot(state: &GameState, full_state: bool) -> SpectatorSnapshot {
    SpectatorSnapshot {
        tick: state.tick,
        full_state,
        units: state
            .units
            .values()
            .map(|u| SpectatorUnit {
                id: u.public_id,
                owner: u.owner,
                q: u.pos.q,
                r: u.pos.r,
                strength: u.strength,
                engaged: !u.engagements.is_empty(),
            })
            .collect(),
        engagements: engagement_pairs(state),
        convoys: state
            .convoys
            .values()
            .map(|c| SpectatorConvoy {
                id: c.public_id,
                owner: c.owner,
                q: c.pos.q,
                r: c.pos.r,
                cargo_type: c.cargo_type,
            })
            .collect(),
        hex_changes: hex_changes(state, full_state),
        settlements: settlements(state),
        players: spectator_players(state),
    }
}

fn settlements(state: &GameState) -> Vec<SpectatorSettlement> {
    let mut out = Vec::new();
    for player in &state.players {
        let mut seen = BTreeSet::new();
        for pop in state.population.values().filter(|p| p.owner == player.id) {
            if state.is_settlement(player.id, pop.hex) && seen.insert((pop.hex.q, pop.hex.r)) {
                out.push(SpectatorSettlement {
                    q: pop.hex.q,
                    r: pop.hex.r,
                    owner: player.id,
                });
            }
        }
    }
    out
}

fn settlement_owner_at(settlements: &[SpectatorSettlement], ax: Axial) -> Option<u8> {
    settlements
        .iter()
        .find(|s| s.q == ax.q && s.r == ax.r)
        .map(|s| s.owner)
}

fn hex_changes(state: &GameState, full_state: bool) -> Vec<SpectatorHexDelta> {
    let settlements = settlements(state);
    let indices: Vec<usize> = if full_state {
        (0..state.grid.len()).collect()
    } else {
        state.dirty_hexes.iter_ones().collect()
    };
    indices
        .into_iter()
        .map(|index| {
            let row = index / state.width;
            let col = index % state.width;
            let ax = super::hex::offset_to_axial(row as i32, col as i32);
            SpectatorHexDelta {
                index,
                owner: state.grid[index].stockpile_owner,
                road_level: state.grid[index].road_level,
                has_settlement: settlement_owner_at(&settlements, ax).is_some(),
                settlement_owner: settlement_owner_at(&settlements, ax),
            }
        })
        .collect()
}

fn engagement_pairs(state: &GameState) -> Vec<(u32, u32)> {
    let mut pairs = BTreeSet::new();
    for unit in state.units.values() {
        for engagement in &unit.engagements {
            if let Some(enemy) = state.units.get(engagement.enemy_id) {
                let a = unit.public_id.min(enemy.public_id);
                let b = unit.public_id.max(enemy.public_id);
                pairs.insert((a, b));
            }
        }
    }
    pairs.into_iter().collect()
}

fn resource_bucket(value: f32) -> u8 {
    match value {
        v if v < 10.0 => 0,
        v if v < 40.0 => 1,
        v if v < 100.0 => 2,
        _ => 3,
    }
}

fn spectator_players(state: &GameState) -> Vec<SpectatorPlayer> {
    state
        .players
        .iter()
        .map(|player| SpectatorPlayer {
            id: player.id,
            alive: player.alive,
            population: state
                .population
                .values()
                .filter(|p| p.owner == player.id)
                .map(|p| p.count)
                .sum(),
            territory: state
                .grid
                .iter()
                .filter(|c| c.stockpile_owner == Some(player.id))
                .count() as u16,
            food_level: resource_bucket(player.food),
            material_level: resource_bucket(player.material),
        })
        .collect()
}
