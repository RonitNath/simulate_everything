use super::hex::{axial_to_offset, within_radius};
use super::state::GameState;
use super::VISION_RADIUS;

/// Compute visibility bitmask for a player. Returns row-major Vec<bool> matching grid layout.
/// A cell is visible if any of the player's units is within VISION_RADIUS hex distance.
pub fn visible_cells(state: &GameState, player_id: u8) -> Vec<bool> {
    let mut visible = vec![false; state.width * state.height];

    for unit in state.units.iter().filter(|u| u.owner == player_id) {
        for ax in within_radius(unit.pos, VISION_RADIUS) {
            let (row, col) = axial_to_offset(ax);
            if row >= 0 && col >= 0 {
                let (row, col) = (row as usize, col as usize);
                if row < state.height && col < state.width {
                    visible[row * state.width + col] = true;
                }
            }
        }
    }

    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex::{axial_to_offset, within_radius};
    use crate::v2::mapgen::{generate, MapConfig};
    use crate::v2::VISION_RADIUS;

    fn test_state() -> GameState {
        generate(&MapConfig { width: 20, height: 20, num_players: 2, seed: 42 })
    }

    #[test]
    fn unit_own_cell_is_visible() {
        let state = test_state();
        let vis = visible_cells(&state, 0);
        for unit in state.units.iter().filter(|u| u.owner == 0) {
            let (row, col) = axial_to_offset(unit.pos);
            assert!(
                vis[row as usize * state.width + col as usize],
                "unit's own cell should be visible"
            );
        }
    }

    #[test]
    fn cells_within_radius_visible() {
        let state = test_state();
        let vis = visible_cells(&state, 0);
        // For each owned unit, all cells within radius 3 that are in bounds should be visible
        for unit in state.units.iter().filter(|u| u.owner == 0) {
            for ax in within_radius(unit.pos, VISION_RADIUS) {
                let (row, col) = axial_to_offset(ax);
                if row >= 0
                    && col >= 0
                    && (row as usize) < state.height
                    && (col as usize) < state.width
                {
                    assert!(
                        vis[row as usize * state.width + col as usize],
                        "cell within vision radius should be visible"
                    );
                }
            }
        }
    }

    #[test]
    fn distant_cells_not_visible() {
        let state = test_state();
        let vis = visible_cells(&state, 0);
        // Count invisible cells — on a 20x20 map with ~6 units, many cells should be invisible
        let invisible_count = vis.iter().filter(|&&v| !v).count();
        assert!(invisible_count > 100, "too few invisible cells: {}", invisible_count);
    }

    #[test]
    fn no_units_means_no_vision() {
        let mut state = test_state();
        // Remove all player 0 units
        state.units.retain(|u| u.owner != 0);
        let vis = visible_cells(&state, 0);
        assert!(vis.iter().all(|&v| !v), "no units should mean no vision");
    }
}
