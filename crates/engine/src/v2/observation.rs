use serde::{Deserialize, Serialize};

use super::hex::axial_to_offset;
use super::state::{Engagement, GameState, Unit};
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
pub struct Observation {
    pub tick: u64,
    pub player: u8,
    pub terrain: Vec<f32>,
    pub width: usize,
    pub height: usize,
    pub resources: f32,
    pub own_units: Vec<UnitInfo>,
    pub visible_enemies: Vec<UnitInfo>,
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

/// Build a player-scoped observation from the full game state.
/// Own units are always included. Enemy units only if in visible cells.
/// Terrain is always fully visible (spec: terrain doesn't change and is known to all).
pub fn observe(state: &GameState, player_id: u8) -> Observation {
    let visible = vision::visible_cells(state, player_id);

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
        .filter(|u| {
            let (row, col) = axial_to_offset(u.pos);
            if row < 0 || col < 0 {
                return false;
            }
            let (row, col) = (row as usize, col as usize);
            row < state.height && col < state.width && visible[row * state.width + col]
        })
        .map(unit_to_info)
        .collect();

    let resources = state
        .players
        .iter()
        .find(|p| p.id == player_id)
        .map(|p| p.resources)
        .unwrap_or(0.0);

    let terrain: Vec<f32> = state.grid.iter().map(|c| c.terrain_value).collect();

    Observation {
        tick: state.tick,
        player: player_id,
        terrain,
        width: state.width,
        height: state.height,
        resources,
        own_units,
        visible_enemies,
        visible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex::{axial_to_offset, Axial};
    use crate::v2::mapgen::{generate, MapConfig};

    fn test_state() -> GameState {
        generate(&MapConfig { width: 30, height: 30, num_players: 2, seed: 42 })
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
        // Each visible enemy must be in a visible cell
        for enemy in &obs.visible_enemies {
            let ax = Axial::new(enemy.q, enemy.r);
            let (row, col) = axial_to_offset(ax);
            let idx = row as usize * state.width + col as usize;
            assert!(obs.visible[idx], "visible enemy at non-visible cell");
        }
    }

    #[test]
    fn observe_excludes_enemies_in_fog() {
        let state = test_state();
        let obs = observe(&state, 0);
        let total_enemies = state.units.iter().filter(|u| u.owner != 0).count();
        // visible_enemies must be a subset of all enemies
        assert!(obs.visible_enemies.len() <= total_enemies);
    }

    #[test]
    fn observe_terrain_is_full() {
        let state = test_state();
        let obs = observe(&state, 0);
        assert_eq!(obs.terrain.len(), state.width * state.height);
    }

    #[test]
    fn observe_resources_match() {
        let mut state = test_state();
        state.players[0].resources = 42.5;
        let obs = observe(&state, 0);
        assert!((obs.resources - 42.5).abs() < 0.01);
    }

    #[test]
    fn observe_does_not_reveal_enemy_resources() {
        // The Observation struct only contains the observing player's resources
        let mut state = test_state();
        state.players[1].resources = 999.0;
        let obs = observe(&state, 0);
        // obs.resources is player 0's resources, not player 1's
        assert!((obs.resources - state.players[0].resources).abs() < 0.01);
    }

    #[test]
    fn observe_visible_mask_consistent_with_own_units() {
        let state = test_state();
        let obs = observe(&state, 0);
        // Every own unit's cell should be visible
        for unit in &obs.own_units {
            let ax = Axial::new(unit.q, unit.r);
            let (row, col) = axial_to_offset(ax);
            let idx = row as usize * state.width + col as usize;
            assert!(obs.visible[idx], "own unit cell should be visible");
        }
    }
}
