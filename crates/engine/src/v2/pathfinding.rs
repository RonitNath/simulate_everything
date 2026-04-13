use super::hex::{Axial, distance, neighbors};
use super::sim::road_bonus;
use super::state::GameState;
use super::{CONVOY_MOVE_COOLDOWN, TERRAIN_MOVE_PENALTY};
use std::collections::{BinaryHeap, HashMap, VecDeque};

/// Returns the next hex step from `from` toward `to`.
///
/// Uses greedy neighbor selection (pick neighbor minimizing distance to target).
/// This is optimal for obstacle-free hex grids. Switch to BFS (see `find_path`)
/// when obstacles are introduced in future layers.
///
/// Returns None if from == to.
pub fn next_step(state: &GameState, from: Axial, to: Axial) -> Option<Axial> {
    if from == to {
        return None;
    }

    let best = neighbors(from)
        .into_iter()
        .filter(|&n| state.in_bounds(n))
        .min_by_key(|&n| distance(n, to));

    best
}

/// Full shortest path from `from` to `to` via BFS.
///
/// Returns path excluding `from`, including `to`.
/// Returns empty vec if from == to or destination is unreachable.
/// Kept for future use when obstacles are introduced.
pub fn find_path(state: &GameState, from: Axial, to: Axial) -> Vec<Axial> {
    if from == to {
        return vec![];
    }
    if !state.in_bounds(to) {
        return vec![];
    }

    let mut queue: VecDeque<Axial> = VecDeque::new();
    let mut came_from: HashMap<Axial, Axial> = HashMap::new();

    queue.push_back(from);
    came_from.insert(from, from);

    while let Some(current) = queue.pop_front() {
        if current == to {
            // Reconstruct path
            let mut path = vec![];
            let mut node = current;
            while node != from {
                path.push(node);
                node = came_from[&node];
            }
            path.reverse();
            return path;
        }
        for neighbor in neighbors(current) {
            if state.in_bounds(neighbor) && !came_from.contains_key(&neighbor) {
                came_from.insert(neighbor, current);
                queue.push_back(neighbor);
            }
        }
    }

    vec![]
}

/// Priority queue entry for A* with reversed ordering for min-heap behavior.
#[derive(PartialEq)]
struct AStarEntry {
    priority: f32,
    pos: Axial,
}

impl Eq for AStarEntry {}

impl PartialOrd for AStarEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AStarEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering: lower priority value = higher heap priority.
        other.priority.total_cmp(&self.priority)
    }
}

/// Returns the edge cost (movement cost) for entering `to` from `from`.
///
/// Mirrors `movement_cooldown()` in sim.rs for convoy movement, so A* weights
/// match the actual time cost convoys pay when traversing each hex.
fn edge_cost(state: &GameState, from: Axial, to: Axial) -> f32 {
    let Some(cell) = state.cell_at(to) else {
        return f32::MAX;
    };
    let from_height = state.cell_at(from).map(|c| c.height).unwrap_or(0.0);
    let slope = (cell.height - from_height).max(0.0);
    let base = CONVOY_MOVE_COOLDOWN as f32;
    let roughness = cell.terrain_value * TERRAIN_MOVE_PENALTY + slope * 2.0;
    (base + roughness) * (1.0 - road_bonus(cell.road_level) * 0.5)
}

/// Weighted A* path from `from` to `to`, preferring roads and low-cost terrain.
///
/// Returns path excluding `from`, including `to`.
/// Returns empty vec if `from == to`, destination is unreachable, or out of bounds.
pub fn find_path_weighted(state: &GameState, from: Axial, to: Axial) -> Vec<Axial> {
    if from == to {
        return vec![];
    }
    if !state.in_bounds(to) {
        return vec![];
    }

    let mut heap: BinaryHeap<AStarEntry> = BinaryHeap::new();
    let mut g_score: HashMap<Axial, f32> = HashMap::new();
    let mut came_from: HashMap<Axial, Axial> = HashMap::new();

    g_score.insert(from, 0.0);
    heap.push(AStarEntry {
        priority: 0.0,
        pos: from,
    });

    while let Some(AStarEntry { pos: current, .. }) = heap.pop() {
        if current == to {
            // Reconstruct path from came_from map.
            let mut path = vec![];
            let mut node = current;
            while node != from {
                path.push(node);
                node = came_from[&node];
            }
            path.reverse();
            return path;
        }

        let current_g = g_score[&current];

        for neighbor in neighbors(current) {
            if !state.in_bounds(neighbor) {
                continue;
            }
            let cost = edge_cost(state, current, neighbor);
            if cost == f32::MAX {
                continue;
            }
            let tentative_g = current_g + cost;
            if tentative_g < *g_score.get(&neighbor).unwrap_or(&f32::MAX) {
                g_score.insert(neighbor, tentative_g);
                came_from.insert(neighbor, current);
                // Heuristic: minimum possible edge cost times remaining hex distance.
                // 1.5 is the minimum edge cost achievable with max road bonus and flat terrain.
                let h = distance(neighbor, to) as f32 * 1.5;
                heap.push(AStarEntry {
                    priority: tentative_g + h,
                    pos: neighbor,
                });
            }
        }
    }

    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex::{distance, offset_to_axial};
    use crate::v2::mapgen::{MapConfig, generate};
    use crate::v2::state::{Biome, Cell};

    fn test_state() -> GameState {
        generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        })
    }

    #[test]
    fn next_step_same_pos_returns_none() {
        let state = test_state();
        let pos = offset_to_axial(5, 5);
        assert!(next_step(&state, pos, pos).is_none());
    }

    #[test]
    fn next_step_returns_valid_neighbor() {
        let state = test_state();
        let from = offset_to_axial(5, 5);
        let to = offset_to_axial(10, 10);
        let step = next_step(&state, from, to).unwrap();
        assert_eq!(distance(from, step), 1, "step should be adjacent to source");
    }

    #[test]
    fn next_step_moves_closer() {
        let state = test_state();
        let from = offset_to_axial(5, 5);
        let to = offset_to_axial(10, 10);
        let step = next_step(&state, from, to).unwrap();
        assert!(
            distance(step, to) < distance(from, to),
            "step should be closer to dest: from_dist={}, step_dist={}",
            distance(from, to),
            distance(step, to)
        );
    }

    #[test]
    fn find_path_same_pos_is_empty() {
        let state = test_state();
        let pos = offset_to_axial(5, 5);
        assert!(find_path(&state, pos, pos).is_empty());
    }

    #[test]
    fn find_path_length_equals_hex_distance() {
        let state = test_state();
        let from = offset_to_axial(3, 3);
        let to = offset_to_axial(8, 8);
        let path = find_path(&state, from, to);
        let expected = distance(from, to) as usize;
        assert_eq!(
            path.len(),
            expected,
            "path length {} should equal hex distance {}",
            path.len(),
            expected
        );
    }

    #[test]
    fn find_path_ends_at_destination() {
        let state = test_state();
        let from = offset_to_axial(2, 2);
        let to = offset_to_axial(7, 7);
        let path = find_path(&state, from, to);
        assert!(!path.is_empty());
        assert_eq!(*path.last().unwrap(), to);
    }

    #[test]
    fn find_path_each_step_adjacent() {
        let state = test_state();
        let from = offset_to_axial(2, 2);
        let to = offset_to_axial(7, 7);
        let path = find_path(&state, from, to);
        let mut prev = from;
        for step in &path {
            assert_eq!(
                distance(prev, *step),
                1,
                "non-adjacent step in path: {:?} -> {:?}",
                prev,
                step
            );
            prev = *step;
        }
    }

    // ---------------------------------------------------------------------------
    // find_path_weighted tests
    // ---------------------------------------------------------------------------

    #[test]
    fn find_path_weighted_same_pos_is_empty() {
        let state = test_state();
        let pos = offset_to_axial(5, 5);
        assert!(find_path_weighted(&state, pos, pos).is_empty());
    }

    #[test]
    fn find_path_weighted_ends_at_destination() {
        let state = test_state();
        let from = offset_to_axial(3, 3);
        let to = offset_to_axial(8, 8);
        let path = find_path_weighted(&state, from, to);
        assert!(!path.is_empty());
        assert_eq!(*path.last().unwrap(), to);
    }

    #[test]
    fn find_path_weighted_each_step_adjacent() {
        let state = test_state();
        let from = offset_to_axial(2, 2);
        let to = offset_to_axial(7, 7);
        let path = find_path_weighted(&state, from, to);
        assert!(!path.is_empty());
        let mut prev = from;
        for step in &path {
            assert_eq!(
                distance(prev, *step),
                1,
                "non-adjacent step in path: {:?} -> {:?}",
                prev,
                step
            );
            prev = *step;
        }
    }

    #[test]
    fn find_path_weighted_out_of_bounds_destination_returns_empty() {
        let state = test_state();
        let from = offset_to_axial(5, 5);
        // A position far outside the 20x20 grid
        let out_of_bounds = Axial::new(9999, 9999);
        assert!(find_path_weighted(&state, from, out_of_bounds).is_empty());
    }

    /// Build a small flat GameState with a road corridor on one side.
    ///
    /// Grid layout (offset coords, 10x5):
    ///   Road path: row 1, cols 1..=8 — all road_level 3
    ///   Direct path: row 3, cols 1..=8 — no roads
    /// from = (row=1, col=1), to = (row=1, col=8) via road corridor
    /// but we measure whether the weighted path prefers the road row even
    /// when an alternative equally-long route exists without roads.
    fn road_state() -> (crate::v2::state::GameState, Axial, Axial, Vec<Axial>) {
        use crate::v2::spatial::SpatialIndex;
        use bitvec::vec::BitVec;
        use slotmap::SlotMap;

        let width = 12usize;
        let height = 6usize;
        let total = width * height;

        let flat_cell = Cell {
            terrain_value: 1.0,
            material_value: 0.0,
            food_stockpile: 0.0,
            material_stockpile: 0.0,
            has_depot: false,
            road_level: 0,
            height: 0.5,
            moisture: 0.5,
            biome: Biome::Grassland,
            is_river: false,
            water_access: 0.5,
            region_id: 0,
            stockpile_owner: None,
        };

        let mut grid = vec![flat_cell; total];

        // Place road_level 3 on row 1, cols 1..=8
        let road_hexes: Vec<Axial> = (1usize..=8)
            .map(|col| offset_to_axial(1, col as i32))
            .collect();
        for ax in &road_hexes {
            let (row, col) = crate::v2::hex::axial_to_offset(*ax);
            let idx = row as usize * width + col as usize;
            grid[idx].road_level = 3;
        }

        let state = crate::v2::state::GameState {
            width,
            height,
            grid,
            units: SlotMap::with_key(),
            players: Vec::new(),
            population: SlotMap::with_key(),
            convoys: SlotMap::with_key(),
            settlements: SlotMap::with_key(),
            regions: Vec::new(),
            tick: 0,
            next_unit_id: 0,
            next_pop_id: 0,
            next_convoy_id: 0,
            next_settlement_id: 0,
            scouted: vec![vec![true; total]; 0],
            spatial: SpatialIndex::new(width, height),
            dirty_hexes: BitVec::repeat(false, total),
            hex_revisions: vec![0; total],
            next_hex_revision: 0,
            territory_cache: vec![None; total],
            #[cfg(debug_assertions)]
            tick_accumulator: None,
            game_log: None,
        };

        let from = offset_to_axial(1, 1);
        let to = offset_to_axial(1, 8);
        (state, from, to, road_hexes)
    }

    #[test]
    fn find_path_weighted_prefers_roads() {
        let (state, from, to, road_hexes) = road_state();
        let path = find_path_weighted(&state, from, to);

        assert!(!path.is_empty(), "should find a path");
        assert_eq!(*path.last().unwrap(), to);

        // Count how many path hexes (excluding `from`) lie on the road corridor.
        // For a road-aware planner the entire path should follow the road strip.
        let road_hex_count = path.iter().filter(|&&ax| road_hexes.contains(&ax)).count();
        // The direct road path has 7 hexes (cols 2..=8). We expect all to be on road.
        assert!(
            road_hex_count >= path.len() - 1,
            "road-aware path should stay on roads: {road_hex_count}/{} road hexes",
            path.len()
        );
    }
}
