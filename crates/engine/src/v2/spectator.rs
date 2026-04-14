use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::hex::Axial;
use super::state::{CargoType, GameState, SettlementType};

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

/// Unified entity for the wire protocol. Flattens the engine's component bags into optional
/// fields so the frontend can render units, convoys, and settlements from one array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorEntity {
    pub id: u32,
    pub owner: Option<u8>,
    pub q: i32,
    pub r: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combat_skill: Option<f32>,
    pub engaged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facing: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_amount: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_progress: Option<f32>,
    pub contains_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorSnapshot {
    pub tick: u64,
    pub full_state: bool,
    pub entities: Vec<SpectatorEntity>,
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
    pub ration_level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorConvoy {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub cargo_type: CargoType,
    /// Remaining waypoints toward destination as (q, r) pairs for frontend visualization.
    pub route: Vec<(i32, i32)>,
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
    let settl = settlements(state);
    let entities = build_entities(state, &settl);

    SpectatorSnapshot {
        tick: state.tick,
        full_state,
        entities,
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
                ration_level: ration_level(u.rations),
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
                route: c.route.iter().map(|a| (a.q, a.r)).collect(),
            })
            .collect(),
        hex_changes: hex_changes(state, full_state),
        settlements: settl,
        players: spectator_players(state),
    }
}

fn build_entities(state: &GameState, settl: &[SpectatorSettlement]) -> Vec<SpectatorEntity> {
    let mut out = Vec::new();

    // Units → entities with health/role/combat_skill
    for u in state.units.values() {
        out.push(SpectatorEntity {
            id: u.public_id,
            owner: Some(u.owner),
            q: u.pos.q,
            r: u.pos.r,
            health: Some(u.strength),
            role: Some("Soldier".to_string()),
            combat_skill: None,
            engaged: !u.engagements.is_empty(),
            facing: None,
            resource_type: None,
            resource_amount: None,
            structure_type: None,
            build_progress: None,
            contains_count: 0,
        });
    }

    // Convoys → entities with resource_type/resource_amount
    for c in state.convoys.values() {
        out.push(SpectatorEntity {
            id: c.public_id,
            owner: Some(c.owner),
            q: c.pos.q,
            r: c.pos.r,
            health: None,
            role: None,
            combat_skill: None,
            engaged: false,
            facing: None,
            resource_type: Some(format!("{:?}", c.cargo_type)),
            resource_amount: Some(c.cargo_amount),
            structure_type: None,
            build_progress: None,
            contains_count: 0,
        });
    }

    // Settlements → entities with structure_type
    for s in settl {
        let stype = settlement_type_at(state, s);
        out.push(SpectatorEntity {
            id: 0, // settlements don't have stable public IDs yet
            owner: Some(s.owner),
            q: s.q,
            r: s.r,
            health: None,
            role: None,
            combat_skill: None,
            engaged: false,
            facing: None,
            resource_type: None,
            resource_amount: None,
            structure_type: Some(format!("{:?}", stype)),
            build_progress: None,
            contains_count: population_at(state, s.owner, Axial { q: s.q, r: s.r }),
        });
    }

    out
}

fn settlement_type_at(state: &GameState, s: &SpectatorSettlement) -> SettlementType {
    let hex = Axial { q: s.q, r: s.r };
    state
        .settlements
        .values()
        .find(|st| st.owner == s.owner && st.hex == hex)
        .map(|st| st.settlement_type)
        .unwrap_or(SettlementType::Village)
}

fn population_at(state: &GameState, owner: u8, hex: Axial) -> usize {
    state
        .population
        .values()
        .filter(|p| p.owner == owner && p.hex == hex)
        .map(|p| p.count as usize)
        .sum()
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

/// Converts a raw rations value to a 0–3 level for display (3=full, 0=empty).
fn ration_level(rations: f32) -> u8 {
    let frac = (rations / super::MAX_RATIONS).clamp(0.0, 1.0);
    if frac >= 0.75 {
        3
    } else if frac >= 0.50 {
        2
    } else if frac >= 0.25 {
        1
    } else {
        0
    }
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
