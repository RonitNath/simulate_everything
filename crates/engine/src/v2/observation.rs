use serde::{Deserialize, Serialize};

use super::hex::axial_to_offset;
use super::state::{CargoType, Engagement, GameState, Role, Unit};
use super::vision;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitInfo {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub strength: f32,
    pub engagements: Vec<Engagement>,
    pub is_general: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopulationInfo {
    pub id: u32,
    pub owner: u8,
    pub q: i32,
    pub r: i32,
    pub count: u16,
    pub role: Role,
    pub training: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvoyInfo {
    pub id: u32,
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
}

fn unit_to_info(u: &Unit) -> UnitInfo {
    UnitInfo {
        id: u.id,
        owner: u.owner,
        q: u.pos.q,
        r: u.pos.r,
        strength: u.strength,
        engagements: u.engagements.clone(),
        is_general: u.is_general,
    }
}

pub fn observe(state: &GameState, player_id: u8) -> Observation {
    let visible = vision::visible_cells(state, player_id);
    let food_stockpiles: Vec<f32> = state
        .grid
        .iter()
        .zip(visible.iter())
        .map(|(cell, &is_visible)| if is_visible { cell.food_stockpile } else { 0.0 })
        .collect();
    let material_stockpiles: Vec<f32> = state
        .grid
        .iter()
        .zip(visible.iter())
        .map(|(cell, &is_visible)| {
            if is_visible {
                cell.material_stockpile
            } else {
                0.0
            }
        })
        .collect();
    let stockpile_owner: Vec<Option<u8>> = state
        .grid
        .iter()
        .zip(visible.iter())
        .map(|(cell, &is_visible)| {
            if is_visible {
                cell.stockpile_owner
            } else {
                None
            }
        })
        .collect();

    let own_units: Vec<UnitInfo> = state
        .units
        .iter()
        .filter(|u| u.owner == player_id)
        .map(unit_to_info)
        .collect();

    let visible_enemies: Vec<UnitInfo> = state
        .units
        .iter()
        .filter(|u| u.owner != player_id)
        .filter(|u| is_visible_cell(state, &visible, u.pos.q, u.pos.r))
        .map(unit_to_info)
        .collect();

    let own_population: Vec<PopulationInfo> = state
        .population
        .iter()
        .filter(|p| p.owner == player_id)
        .map(|p| PopulationInfo {
            id: p.id,
            owner: p.owner,
            q: p.hex.q,
            r: p.hex.r,
            count: p.count,
            role: p.role,
            training: p.training,
        })
        .collect();

    let visible_enemy_population: Vec<PopulationInfo> = state
        .population
        .iter()
        .filter(|p| p.owner != player_id)
        .filter(|p| is_visible_cell(state, &visible, p.hex.q, p.hex.r))
        .map(|p| PopulationInfo {
            id: p.id,
            owner: p.owner,
            q: p.hex.q,
            r: p.hex.r,
            count: p.count,
            role: p.role,
            training: p.training,
        })
        .collect();

    let own_convoys: Vec<ConvoyInfo> = state
        .convoys
        .iter()
        .filter(|c| c.owner == player_id)
        .map(|c| ConvoyInfo {
            id: c.id,
            owner: c.owner,
            q: c.pos.q,
            r: c.pos.r,
            destination_q: c.destination.q,
            destination_r: c.destination.r,
            cargo_type: c.cargo_type,
            cargo_amount: c.cargo_amount,
        })
        .collect();

    let visible_enemy_convoys: Vec<ConvoyInfo> = state
        .convoys
        .iter()
        .filter(|c| c.owner != player_id)
        .filter(|c| is_visible_cell(state, &visible, c.pos.q, c.pos.r))
        .map(|c| ConvoyInfo {
            id: c.id,
            owner: c.owner,
            q: c.pos.q,
            r: c.pos.r,
            destination_q: c.destination.q,
            destination_r: c.destination.r,
            cargo_type: c.cargo_type,
            cargo_amount: c.cargo_amount,
        })
        .collect();

    Observation {
        tick: state.tick,
        player: player_id,
        terrain: state.grid.iter().map(|c| c.terrain_value).collect(),
        material_map: state.grid.iter().map(|c| c.material_value).collect(),
        road_levels: state.grid.iter().map(|c| c.road_level).collect(),
        food_stockpiles,
        material_stockpiles,
        stockpile_owner,
        width: state.width,
        height: state.height,
        total_food: state
            .players
            .iter()
            .find(|p| p.id == player_id)
            .map(|p| p.food)
            .unwrap_or(0.0),
        total_material: state
            .players
            .iter()
            .find(|p| p.id == player_id)
            .map(|p| p.material)
            .unwrap_or(0.0),
        own_units,
        visible_enemies,
        own_population,
        visible_enemy_population,
        own_convoys,
        visible_enemy_convoys,
        visible,
    }
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
    use crate::v2::hex::{Axial, axial_to_offset};
    use crate::v2::mapgen::{MapConfig, generate};

    fn test_state() -> GameState {
        generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        })
    }

    #[test]
    fn observe_includes_all_own_units() {
        let state = test_state();
        let obs = observe(&state, 0);
        let expected = state.units.iter().filter(|u| u.owner == 0).count();
        assert_eq!(obs.own_units.len(), expected);
    }

    #[test]
    fn observe_only_visible_enemies() {
        let state = test_state();
        let obs = observe(&state, 0);
        for enemy in &obs.visible_enemies {
            let ax = Axial::new(enemy.q, enemy.r);
            let (row, col) = axial_to_offset(ax);
            let idx = row as usize * state.width + col as usize;
            assert!(obs.visible[idx]);
        }
    }

    #[test]
    fn observe_contains_population_and_stockpiles() {
        let state = test_state();
        let obs = observe(&state, 0);
        assert!(!obs.own_population.is_empty());
        assert_eq!(obs.food_stockpiles.len(), state.width * state.height);
        assert_eq!(obs.road_levels.len(), state.width * state.height);
    }

    #[test]
    fn observe_hides_stockpiles_outside_vision() {
        let mut state = test_state();
        let visible = vision::visible_cells(&state, 0);
        let hidden_idx = visible
            .iter()
            .position(|cell_visible| !cell_visible)
            .expect("expected at least one hidden cell");

        state.grid[hidden_idx].food_stockpile = 12.0;
        state.grid[hidden_idx].material_stockpile = 7.0;
        state.grid[hidden_idx].stockpile_owner = Some(1);

        let obs = observe(&state, 0);
        assert_eq!(obs.food_stockpiles[hidden_idx], 0.0);
        assert_eq!(obs.material_stockpiles[hidden_idx], 0.0);
        assert_eq!(obs.stockpile_owner[hidden_idx], None);
    }
}
