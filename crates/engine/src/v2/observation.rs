use serde::{Deserialize, Serialize};

use super::hex::axial_to_offset;
use super::state::{CargoType, ConvoyKey, Engagement, GameState, PopKey, Role, Unit, UnitKey};
use super::vision;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitInfo {
    pub id: UnitKey,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub strength: f32,
    pub engagements: Vec<Engagement>,
    pub rations: f32,
    pub half_rations: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopulationInfo {
    pub id: PopKey,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub count: u16,
    pub role: Role,
    pub training: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvoyInfo {
    pub id: ConvoyKey,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub destination_q: i32,
    pub destination_r: i32,
    pub cargo_type: CargoType,
    pub cargo_amount: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub tick: u64,
    pub player: u8,
    pub terrain: Vec<f32>,
    pub material_map: Vec<f32>,
    pub road_levels: Vec<u8>,
    pub height_map: Vec<f32>,
    pub food_stockpiles: Vec<f32>,
    pub material_stockpiles: Vec<f32>,
    pub stockpile_owner: Vec<Option<u8>>,
    pub width: usize,
    pub height: usize,
    pub total_food: f32,
    pub total_material: f32,
    pub own_units: Vec<UnitInfo>,
    pub visible_enemies: Vec<UnitInfo>,
    pub own_population: Vec<PopulationInfo>,
    pub visible_enemy_population: Vec<PopulationInfo>,
    pub own_convoys: Vec<ConvoyInfo>,
    pub visible_enemy_convoys: Vec<ConvoyInfo>,
    pub visible: Vec<bool>,
    pub scouted: Vec<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialObservation {
    pub width: usize,
    pub height: usize,
    pub player: u8,
    pub terrain: Vec<f32>,
    pub material_map: Vec<f32>,
    pub height_map: Vec<f32>,
    pub scouted: Vec<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewScoutedHex {
    pub index: usize,
    pub terrain: f32,
    pub material: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexDelta {
    pub index: usize,
    pub food_stockpile: f32,
    pub material_stockpile: f32,
    pub stockpile_owner: Option<u8>,
    pub road_level: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationDelta {
    pub tick: u64,
    pub player: u8,
    pub newly_scouted: Vec<NewScoutedHex>,
    pub hex_changes: Vec<HexDelta>,
    pub own_units: Vec<UnitInfo>,
    pub visible_enemies: Vec<UnitInfo>,
    pub own_population: Vec<PopulationInfo>,
    pub visible_enemy_population: Vec<PopulationInfo>,
    pub own_convoys: Vec<ConvoyInfo>,
    pub visible_enemy_convoys: Vec<ConvoyInfo>,
    pub visible: Vec<bool>,
    pub total_food: f32,
    pub total_material: f32,
}

#[derive(Debug, Clone)]
pub struct PlayerObservationSession {
    initialized: bool,
    previous_visible: Vec<bool>,
    previous_scouted: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct ObservationSession {
    players: Vec<PlayerObservationSession>,
}

impl ObservationSession {
    pub fn new(player_count: usize, cells: usize) -> Self {
        Self {
            players: (0..player_count)
                .map(|_| PlayerObservationSession {
                    initialized: false,
                    previous_visible: vec![false; cells],
                    previous_scouted: vec![false; cells],
                })
                .collect(),
        }
    }

    pub fn ensure_player(&mut self, player_id: usize, cells: usize) {
        while self.players.len() <= player_id {
            self.players.push(PlayerObservationSession {
                initialized: false,
                previous_visible: vec![false; cells],
                previous_scouted: vec![false; cells],
            });
        }
    }

    pub fn reset(&mut self) {
        for player in &mut self.players {
            player.initialized = false;
            player.previous_visible.fill(false);
            player.previous_scouted.fill(false);
        }
    }
}

fn unit_to_info(id: UnitKey, u: &Unit) -> UnitInfo {
    UnitInfo {
        id,
        owner: u.owner,
        q: u.pos.q,
        r: u.pos.r,
        strength: u.strength,
        engagements: u.engagements.clone(),
        rations: u.rations,
        half_rations: u.half_rations,
    }
}

fn player_totals(state: &GameState, player_id: u8) -> (f32, f32) {
    state
        .players
        .iter()
        .find(|p| p.id == player_id)
        .map(|p| (p.food, p.material))
        .unwrap_or((0.0, 0.0))
}

fn collect_entities(
    state: &GameState,
    player_id: u8,
    visible: &[bool],
) -> (
    Vec<UnitInfo>,
    Vec<UnitInfo>,
    Vec<PopulationInfo>,
    Vec<PopulationInfo>,
    Vec<ConvoyInfo>,
    Vec<ConvoyInfo>,
) {
    let own_units = state
        .units
        .iter()
        .filter(|(_, u)| u.owner == player_id)
        .map(|(id, u)| unit_to_info(id, u))
        .collect();
    let visible_enemies = state
        .units
        .iter()
        .filter(|(_, u)| u.owner != player_id)
        .filter(|(_, u)| is_visible_cell(state, visible, u.pos.q, u.pos.r))
        .map(|(id, u)| unit_to_info(id, u))
        .collect();
    let own_population = state
        .population
        .iter()
        .filter(|(_, p)| p.owner == player_id)
        .map(|(id, p)| PopulationInfo {
            id,
            owner: p.owner,
            q: p.hex.q,
            r: p.hex.r,
            count: p.count,
            role: p.role,
            training: p.training,
        })
        .collect();
    let visible_enemy_population = state
        .population
        .iter()
        .filter(|(_, p)| p.owner != player_id)
        .filter(|(_, p)| is_visible_cell(state, visible, p.hex.q, p.hex.r))
        .map(|(id, p)| PopulationInfo {
            id,
            owner: p.owner,
            q: p.hex.q,
            r: p.hex.r,
            count: p.count,
            role: p.role,
            training: p.training,
        })
        .collect();
    let own_convoys = state
        .convoys
        .iter()
        .filter(|(_, c)| c.owner == player_id)
        .map(|(id, c)| ConvoyInfo {
            id,
            owner: c.owner,
            q: c.pos.q,
            r: c.pos.r,
            destination_q: c.destination.q,
            destination_r: c.destination.r,
            cargo_type: c.cargo_type,
            cargo_amount: c.cargo_amount,
        })
        .collect();
    let visible_enemy_convoys = state
        .convoys
        .iter()
        .filter(|(_, c)| c.owner != player_id)
        .filter(|(_, c)| is_visible_cell(state, visible, c.pos.q, c.pos.r))
        .map(|(id, c)| ConvoyInfo {
            id,
            owner: c.owner,
            q: c.pos.q,
            r: c.pos.r,
            destination_q: c.destination.q,
            destination_r: c.destination.r,
            cargo_type: c.cargo_type,
            cargo_amount: c.cargo_amount,
        })
        .collect();

    (
        own_units,
        visible_enemies,
        own_population,
        visible_enemy_population,
        own_convoys,
        visible_enemy_convoys,
    )
}

pub fn initial_observation(state: &GameState, player_id: u8) -> InitialObservation {
    let scouted = state.scouted[player_id as usize].clone();
    InitialObservation {
        width: state.width,
        height: state.height,
        player: player_id,
        terrain: state
            .grid
            .iter()
            .zip(scouted.iter())
            .map(|(cell, &is_scouted)| if is_scouted { cell.terrain_value } else { 0.0 })
            .collect(),
        material_map: state
            .grid
            .iter()
            .zip(scouted.iter())
            .map(|(cell, &is_scouted)| if is_scouted { cell.material_value } else { 0.0 })
            .collect(),
        height_map: state
            .grid
            .iter()
            .zip(scouted.iter())
            .map(|(cell, &is_scouted)| if is_scouted { cell.height } else { 0.0 })
            .collect(),
        scouted,
    }
}

pub fn observe_delta(
    state: &mut GameState,
    player_id: u8,
    session: &mut ObservationSession,
) -> ObservationDelta {
    let cells = state.width * state.height;
    let player_idx = player_id as usize;
    session.ensure_player(player_idx, cells);
    let player_session = &mut session.players[player_idx];
    if !player_session.initialized {
        player_session.previous_scouted = state.scouted[player_idx].clone();
        player_session.initialized = true;
    }

    let previous_visible = player_session.previous_visible.clone();
    let previous_scouted = player_session.previous_scouted.clone();
    let visible = vision::visible_cells(state, player_id);
    if let Some(player_scouted) = state.scouted.get_mut(player_idx) {
        for (s, v) in player_scouted.iter_mut().zip(visible.iter()) {
            *s |= *v;
        }
    }
    let scouted = state.scouted[player_idx].clone();

    let newly_scouted = scouted
        .iter()
        .enumerate()
        .filter(|(idx, is_scouted)| **is_scouted && !previous_scouted[*idx])
        .map(|(idx, _)| NewScoutedHex {
            index: idx,
            terrain: state.grid[idx].terrain_value,
            material: state.grid[idx].material_value,
            height: state.grid[idx].height,
        })
        .collect();

    let mut changed_indices = Vec::new();
    for idx in state.dirty_hexes.iter_ones() {
        if visible[idx] {
            changed_indices.push(idx);
        }
    }
    for (idx, (&was_visible, &is_visible)) in
        previous_visible.iter().zip(visible.iter()).enumerate()
    {
        if was_visible != is_visible || (is_visible && !previous_scouted[idx] && scouted[idx]) {
            changed_indices.push(idx);
        }
    }
    changed_indices.sort_unstable();
    changed_indices.dedup();

    let hex_changes = changed_indices
        .into_iter()
        .filter(|&idx| visible[idx])
        .map(|idx| HexDelta {
            index: idx,
            food_stockpile: state.grid[idx].food_stockpile,
            material_stockpile: state.grid[idx].material_stockpile,
            stockpile_owner: state.grid[idx].stockpile_owner,
            road_level: if scouted[idx] {
                state.grid[idx].road_level
            } else {
                0
            },
        })
        .collect();

    let (
        own_units,
        visible_enemies,
        own_population,
        visible_enemy_population,
        own_convoys,
        visible_enemy_convoys,
    ) = collect_entities(state, player_id, &visible);
    let (total_food, total_material) = player_totals(state, player_id);

    player_session.previous_visible = visible.clone();
    player_session.previous_scouted = scouted;

    ObservationDelta {
        tick: state.tick,
        player: player_id,
        newly_scouted,
        hex_changes,
        own_units,
        visible_enemies,
        own_population,
        visible_enemy_population,
        own_convoys,
        visible_enemy_convoys,
        visible,
        total_food,
        total_material,
    }
}

pub fn materialize_observation(init: &InitialObservation, delta: &ObservationDelta) -> Observation {
    let mut obs = Observation {
        tick: delta.tick,
        player: delta.player,
        terrain: init.terrain.clone(),
        material_map: init.material_map.clone(),
        road_levels: vec![0; init.width * init.height],
        height_map: init.height_map.clone(),
        food_stockpiles: vec![0.0; init.width * init.height],
        material_stockpiles: vec![0.0; init.width * init.height],
        stockpile_owner: vec![None; init.width * init.height],
        width: init.width,
        height: init.height,
        total_food: delta.total_food,
        total_material: delta.total_material,
        own_units: delta.own_units.clone(),
        visible_enemies: delta.visible_enemies.clone(),
        own_population: delta.own_population.clone(),
        visible_enemy_population: delta.visible_enemy_population.clone(),
        own_convoys: delta.own_convoys.clone(),
        visible_enemy_convoys: delta.visible_enemy_convoys.clone(),
        visible: delta.visible.clone(),
        scouted: init.scouted.clone(),
    };
    apply_delta_to_observation(&mut obs, delta);
    obs
}

pub fn apply_delta_to_observation(obs: &mut Observation, delta: &ObservationDelta) {
    obs.tick = delta.tick;
    obs.player = delta.player;
    obs.total_food = delta.total_food;
    obs.total_material = delta.total_material;
    obs.own_units = delta.own_units.clone();
    obs.visible_enemies = delta.visible_enemies.clone();
    obs.own_population = delta.own_population.clone();
    obs.visible_enemy_population = delta.visible_enemy_population.clone();
    obs.own_convoys = delta.own_convoys.clone();
    obs.visible_enemy_convoys = delta.visible_enemy_convoys.clone();
    obs.visible = delta.visible.clone();

    for idx in 0..obs.visible.len() {
        if !obs.visible[idx] {
            obs.food_stockpiles[idx] = 0.0;
            obs.material_stockpiles[idx] = 0.0;
            obs.stockpile_owner[idx] = None;
        }
    }

    for scouted in &delta.newly_scouted {
        obs.scouted[scouted.index] = true;
        obs.terrain[scouted.index] = scouted.terrain;
        obs.material_map[scouted.index] = scouted.material;
        obs.height_map[scouted.index] = scouted.height;
    }

    for cell in &delta.hex_changes {
        obs.food_stockpiles[cell.index] = cell.food_stockpile;
        obs.material_stockpiles[cell.index] = cell.material_stockpile;
        obs.stockpile_owner[cell.index] = cell.stockpile_owner;
        if obs.scouted[cell.index] {
            obs.road_levels[cell.index] = cell.road_level;
        }
    }
}

pub fn observe(state: &mut GameState, player_id: u8) -> Observation {
    let init = initial_observation(state, player_id);
    let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
    let delta = observe_delta(state, player_id, &mut session);
    materialize_observation(&init, &delta)
}

fn is_visible_cell(state: &GameState, visible: &[bool], q: i32, r: i32) -> bool {
    let (row, col) = axial_to_offset(super::hex::Axial::new(q, r));
    if row < 0 || col < 0 {
        return false;
    }
    let (row, col) = (row as usize, col as usize);
    row < state.height && col < state.width && visible[row * state.width + col]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex::offset_to_axial;
    use crate::v2::mapgen::{MapConfig, generate};

    #[test]
    fn observe_masks_hidden_dynamic_cells() {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 1,
        });
        let hidden = offset_to_axial(19, 19);
        let idx = state.index(19, 19);
        let cell = state.cell_at_mut(hidden).unwrap();
        cell.food_stockpile = 9.0;
        cell.material_stockpile = 4.0;
        cell.stockpile_owner = Some(1);
        state.mark_dirty_axial(hidden);

        let obs = observe(&mut state, 0);
        assert_eq!(obs.food_stockpiles[idx], 0.0);
        assert_eq!(obs.material_stockpiles[idx], 0.0);
        assert_eq!(obs.stockpile_owner[idx], None);
    }

    #[test]
    fn delta_roundtrip_matches_full_observe() {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        });
        let init = initial_observation(&state, 0);
        let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
        let delta = observe_delta(&mut state, 0, &mut session);
        let materialized = materialize_observation(&init, &delta);
        let full = observe(&mut state, 0);
        assert_eq!(materialized.visible, full.visible);
        assert_eq!(materialized.scouted, full.scouted);
        assert_eq!(materialized.terrain, full.terrain);
        assert_eq!(materialized.material_map, full.material_map);
        assert_eq!(materialized.height_map, full.height_map);
    }
}
