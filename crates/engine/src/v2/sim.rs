use super::combat;
use super::hex::{self, Axial};
use super::pathfinding;
use super::state::GameState;
use super::{BASE_MOVE_COOLDOWN, RESOURCE_RATE, TERRAIN_MOVE_PENALTY};
use std::collections::HashMap;

/// Advance the game state by one tick.
///
/// Order: resource generation → combat resolution → movement → cooldown decrement → cleanup → tick increment.
pub fn tick(state: &mut GameState) {
    generate_resources(state);
    combat::resolve_combat(state);
    move_units(state);
    decrement_cooldowns(state);
    cleanup(state);
    check_stale_engagements(state);
    state.tick += 1;
}

/// Check if the game is over (0 or 1 players alive).
pub fn is_over(state: &GameState) -> bool {
    state.players.iter().filter(|p| p.alive).count() <= 1
}

/// Get the winner's player id, if the game is decided.
pub fn winner(state: &GameState) -> Option<u8> {
    let alive: Vec<_> = state.players.iter().filter(|p| p.alive).collect();
    if alive.len() == 1 {
        Some(alive[0].id)
    } else {
        None
    }
}

fn generate_resources(state: &mut GameState) {
    let incomes: Vec<(u8, f32)> = state
        .units
        .iter()
        .filter(|u| u.destination.is_none() && u.engagements.is_empty())
        .filter_map(|u| {
            state
                .cell_at(u.pos)
                .map(|cell| (u.owner, cell.terrain_value * RESOURCE_RATE))
        })
        .collect();

    for (owner, income) in incomes {
        if let Some(player) = state.players.iter_mut().find(|p| p.id == owner) {
            player.resources += income;
        }
    }
}

fn move_units(state: &mut GameState) {
    // Compute movement decisions immutably first to avoid borrow conflicts.
    // Each entry: (unit_index, new_pos, new_cooldown, clear_destination)
    let moves: Vec<(usize, Axial, u8, bool)> = state
        .units
        .iter()
        .enumerate()
        .filter_map(|(i, u)| {
            // Skip units with no destination, non-zero cooldown, or active engagements
            let dest = u.destination?;
            if u.move_cooldown > 0 || !u.engagements.is_empty() {
                return None;
            }

            match pathfinding::next_step(state, u.pos, dest) {
                Some(next_pos) => {
                    // Cooldown is based on terrain of the destination cell
                    let terrain_penalty = state
                        .cell_at(next_pos)
                        .map(|c| (c.terrain_value * TERRAIN_MOVE_PENALTY) as u8)
                        .unwrap_or(0);
                    let cooldown = BASE_MOVE_COOLDOWN + terrain_penalty;
                    let arrived = next_pos == dest;
                    Some((i, next_pos, cooldown, arrived))
                }
                // next_step returns None only when from == to (already at destination)
                None => Some((i, u.pos, 0, true)),
            }
        })
        .collect();

    for (i, new_pos, cooldown, clear_dest) in moves {
        state.units[i].pos = new_pos;
        state.units[i].move_cooldown = cooldown;
        if clear_dest {
            state.units[i].destination = None;
        }
    }
}

fn decrement_cooldowns(state: &mut GameState) {
    for unit in &mut state.units {
        if unit.move_cooldown > 0 {
            unit.move_cooldown -= 1;
        }
    }
}

fn cleanup(state: &mut GameState) {
    // Clear engagements referencing dead units before removing them
    combat::cleanup_engagements(state);

    // Log deaths
    for u in state.units.iter().filter(|u| u.strength <= 0.0) {
        tracing::debug!(
            tick = state.tick,
            unit_id = u.id,
            owner = u.owner,
            is_general = u.is_general,
            "unit killed"
        );
    }

    // Remove dead units
    state.units.retain(|u| u.strength > 0.0);

    // Collect which players lost their general this tick
    let eliminated: Vec<u8> = state
        .players
        .iter()
        .filter(|p| p.alive)
        .filter(|p| !state.units.iter().any(|u| u.id == p.general_id))
        .map(|p| p.id)
        .collect();

    for pid in eliminated {
        tracing::info!(tick = state.tick, player = pid, "player eliminated");
        if let Some(player) = state.players.iter_mut().find(|p| p.id == pid) {
            player.alive = false;
        }
        // Collect IDs of units being removed so we can clean up engagement refs
        let removed_ids: Vec<u32> = state
            .units
            .iter()
            .filter(|u| u.owner == pid)
            .map(|u| u.id)
            .collect();
        state.units.retain(|u| u.owner != pid);
        // Clean up stale engagement refs on surviving units
        for unit in &mut state.units {
            unit.engagements
                .retain(|e| !removed_ids.contains(&e.enemy_id));
        }
    }
}

/// Log stale engagements — units marked as engaged but with no valid adjacent enemy.
/// This is a diagnostic to detect engagement state bugs.
fn check_stale_engagements(state: &GameState) {
    let unit_positions: HashMap<u32, Axial> = state
        .units
        .iter()
        .map(|u| (u.id, u.pos))
        .collect();

    for u in &state.units {
        for eng in &u.engagements {
            match unit_positions.get(&eng.enemy_id) {
                None => {
                    tracing::warn!(
                        tick = state.tick,
                        unit_id = u.id,
                        enemy_id = eng.enemy_id,
                        edge = eng.edge,
                        "stale engagement: enemy does not exist"
                    );
                }
                Some(&enemy_pos) => {
                    if hex::shared_edge(u.pos, enemy_pos).is_none() {
                        tracing::warn!(
                            tick = state.tick,
                            unit_id = u.id,
                            enemy_id = eng.enemy_id,
                            unit_pos = ?u.pos,
                            enemy_pos = ?enemy_pos,
                            "stale engagement: units not adjacent"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex::{distance, neighbors, offset_to_axial};
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
    fn tick_increments() {
        let mut state = test_state();
        assert_eq!(state.tick, 0);
        tick(&mut state);
        assert_eq!(state.tick, 1);
    }

    #[test]
    fn stationary_units_generate_resources() {
        let mut state = test_state();
        let initial_resources: Vec<f32> = state.players.iter().map(|p| p.resources).collect();
        tick(&mut state);
        for (i, player) in state.players.iter().enumerate() {
            assert!(
                player.resources > initial_resources[i],
                "player {} resources didn't increase",
                player.id
            );
        }
    }

    #[test]
    fn unit_moves_toward_destination() {
        let mut state = test_state();
        let unit_idx = state.units.iter().position(|u| !u.is_general).unwrap();
        let start = state.units[unit_idx].pos;
        let dest = offset_to_axial(10, 10);
        state.units[unit_idx].destination = Some(dest);
        state.units[unit_idx].move_cooldown = 0;

        let initial_dist = distance(start, dest);
        tick(&mut state);

        let new_pos = state.units[unit_idx].pos;
        let new_dist = distance(new_pos, dest);
        assert!(
            new_dist < initial_dist,
            "unit didn't move closer: was {} now {}",
            initial_dist,
            new_dist
        );
    }

    #[test]
    fn unit_respects_cooldown() {
        let mut state = test_state();
        let unit_idx = state.units.iter().position(|u| !u.is_general).unwrap();
        let dest = offset_to_axial(10, 10);
        state.units[unit_idx].destination = Some(dest);
        state.units[unit_idx].move_cooldown = 5;

        let start = state.units[unit_idx].pos;
        tick(&mut state);

        assert_eq!(state.units[unit_idx].pos, start);
        assert_eq!(state.units[unit_idx].move_cooldown, 4);
    }

    #[test]
    fn unit_gets_cooldown_after_move() {
        let mut state = test_state();
        let unit_idx = state.units.iter().position(|u| !u.is_general).unwrap();
        let dest = offset_to_axial(10, 10);
        state.units[unit_idx].destination = Some(dest);
        state.units[unit_idx].move_cooldown = 0;

        tick(&mut state);

        // cooldown = BASE_MOVE_COOLDOWN + terrain_penalty, then decremented by 1
        // BASE_MOVE_COOLDOWN=2, terrain_penalty in [0,1], so result is 1 or 2
        assert!(
            state.units[unit_idx].move_cooldown >= 1,
            "cooldown {} too low after move",
            state.units[unit_idx].move_cooldown
        );
    }

    #[test]
    fn unit_clears_destination_on_arrival() {
        let mut state = test_state();
        let unit_idx = state.units.iter().position(|u| !u.is_general).unwrap();
        let start = state.units[unit_idx].pos;
        let dest = *neighbors(start)
            .iter()
            .find(|&&n| state.in_bounds(n))
            .unwrap();
        state.units[unit_idx].destination = Some(dest);
        state.units[unit_idx].move_cooldown = 0;

        tick(&mut state);

        assert_eq!(state.units[unit_idx].pos, dest);
        assert!(
            state.units[unit_idx].destination.is_none(),
            "destination should be cleared on arrival"
        );
    }

    #[test]
    fn cleanup_removes_dead_units() {
        let mut state = test_state();
        let unit_idx = state.units.iter().position(|u| !u.is_general).unwrap();
        state.units[unit_idx].strength = 0.0;
        let initial_count = state.units.len();

        tick(&mut state);

        assert_eq!(state.units.len(), initial_count - 1);
    }

    #[test]
    fn general_death_eliminates_player() {
        let mut state = test_state();
        let general_id = state.players[0].general_id;
        let gen_idx = state.units.iter().position(|u| u.id == general_id).unwrap();
        let player_0_units_before = state.units.iter().filter(|u| u.owner == 0).count();
        assert!(player_0_units_before > 1);

        state.units[gen_idx].strength = 0.0;
        tick(&mut state);

        assert!(!state.players[0].alive);
        assert_eq!(state.units.iter().filter(|u| u.owner == 0).count(), 0);
    }

    #[test]
    fn is_over_after_elimination() {
        let mut state = test_state();
        let general_id = state.players[0].general_id;
        let gen_idx = state.units.iter().position(|u| u.id == general_id).unwrap();
        state.units[gen_idx].strength = 0.0;
        tick(&mut state);

        assert!(is_over(&state));
        assert_eq!(winner(&state), Some(1));
    }

    #[test]
    fn multi_tick_pathfinding() {
        let mut state = test_state();
        let unit_idx = state.units.iter().position(|u| !u.is_general).unwrap();
        let dest = offset_to_axial(10, 10);
        state.units[unit_idx].destination = Some(dest);
        state.units[unit_idx].move_cooldown = 0;

        for _ in 0..500 {
            tick(&mut state);
            if state.units[unit_idx].pos == dest {
                break;
            }
        }

        assert_eq!(
            state.units[unit_idx].pos, dest,
            "unit didn't reach destination in 500 ticks"
        );
        assert!(state.units[unit_idx].destination.is_none());
    }
}
