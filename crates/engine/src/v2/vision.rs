use super::hex::{Axial, axial_to_offset, within_radius};
use super::state::GameState;
use super::{
    CITY_THRESHOLD, CITY_VISION, FARM_THRESHOLD, FARM_VISION, VILLAGE_THRESHOLD, VILLAGE_VISION,
    VISION_RADIUS,
};

/// Compute visibility bitmask for a player. Returns row-major Vec<bool> matching grid layout.
/// A cell is visible if any of the player's units or settlements is within vision range.
pub fn visible_cells(state: &GameState, player_id: u8) -> Vec<bool> {
    let mut visible = vec![false; state.width * state.height];

    for unit in state.units.values().filter(|u| u.owner == player_id) {
        let vision_bonus = state
            .cell_at(unit.pos)
            .map(|c| if c.height > 0.7 { 1 } else { 0 })
            .unwrap_or(0);
        for ax in within_radius(unit.pos, VISION_RADIUS + vision_bonus) {
            let (row, col) = axial_to_offset(ax);
            if row >= 0 && col >= 0 {
                let (row, col) = (row as usize, col as usize);
                if row < state.height && col < state.width {
                    visible[row * state.width + col] = true;
                }
            }
        }
    }

    // Settlement vision: settlements provide vision based on population tier.
    let mut seen_hexes: Vec<Axial> = Vec::new();
    for pop in state.population.values().filter(|p| p.owner == player_id) {
        if seen_hexes.contains(&pop.hex) {
            continue;
        }
        let pop_count = state.population_on_hex(player_id, pop.hex);
        let vision_radius = if pop_count >= CITY_THRESHOLD {
            CITY_VISION
        } else if pop_count >= VILLAGE_THRESHOLD {
            VILLAGE_VISION
        } else if pop_count >= FARM_THRESHOLD {
            FARM_VISION
        } else {
            continue;
        };
        seen_hexes.push(pop.hex);
        for ax in within_radius(pop.hex, vision_radius) {
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
    use crate::v2::VISION_RADIUS;
    use crate::v2::hex::{axial_to_offset, within_radius};
    use crate::v2::mapgen::{MapConfig, generate};

    fn test_state() -> GameState {
        generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        })
    }

    #[test]
    fn unit_own_cell_is_visible() {
        let state = test_state();
        let vis = visible_cells(&state, 0);
        for unit in state.units.values().filter(|u| u.owner == 0) {
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
        for unit in state.units.values().filter(|u| u.owner == 0) {
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
        assert!(
            invisible_count > 100,
            "too few invisible cells: {}",
            invisible_count
        );
    }

    #[test]
    fn no_vision_sources_means_no_vision() {
        let mut state = test_state();
        // Remove all player 0 units and population (both provide vision).
        state.units.retain(|_, u| u.owner != 0);
        state.population.retain(|_, p| p.owner != 0);
        let vis = visible_cells(&state, 0);
        assert!(
            vis.iter().all(|&v| !v),
            "no units or settlements should mean no vision"
        );
    }
}
