use super::hex::{distance, neighbors, Axial};
use super::state::GameState;
use std::collections::{HashMap, VecDeque};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex::{distance, offset_to_axial};
    use crate::v2::mapgen::{generate, MapConfig};

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
}
