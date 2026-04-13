use serde::{Deserialize, Serialize};

use super::combat;
use super::hex::{self, Axial};
use super::state::{GameState, Unit};
use super::{INITIAL_STRENGTH, UNIT_FOOD_COST, UNIT_MATERIAL_COST};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Directive {
    Move { unit_id: u32, q: i32, r: i32 },
    Engage { unit_id: u32, target_id: u32 },
    DisengageEdge { unit_id: u32, edge: u8 },
    DisengageAll { unit_id: u32 },
    Produce,
    Pass,
}

/// Apply a list of directives for a player. Invalid directives are silently ignored.
pub fn apply_directives(state: &mut GameState, player_id: u8, directives: &[Directive]) {
    for directive in directives {
        apply_one(state, player_id, directive);
    }
}

fn apply_one(state: &mut GameState, player_id: u8, directive: &Directive) {
    match directive {
        Directive::Move { unit_id, q, r } => {
            let dest = Axial::new(*q, *r);
            // Check bounds before taking mutable borrow of units
            if !state.in_bounds(dest) {
                return;
            }
            // Unit must exist, belong to player, and not be engaged
            if let Some(unit) = state
                .units
                .iter_mut()
                .find(|u| u.id == *unit_id && u.owner == player_id)
            {
                if unit.engagements.is_empty() {
                    unit.destination = Some(dest);
                }
            }
        }
        Directive::Engage { unit_id, target_id } => {
            // Verify unit belongs to player before delegating
            if state
                .units
                .iter()
                .any(|u| u.id == *unit_id && u.owner == player_id)
            {
                combat::engage(state, *unit_id, *target_id);
            }
        }
        Directive::DisengageEdge { unit_id, edge } => {
            if state
                .units
                .iter()
                .any(|u| u.id == *unit_id && u.owner == player_id)
            {
                combat::disengage_edge(state, *unit_id, *edge);
            }
        }
        Directive::DisengageAll { unit_id } => {
            if state
                .units
                .iter()
                .any(|u| u.id == *unit_id && u.owner == player_id)
            {
                combat::disengage_all(state, *unit_id);
            }
        }
        Directive::Produce => {
            produce_unit(state, player_id);
        }
        Directive::Pass => {}
    }
}

fn produce_unit(state: &mut GameState, player_id: u8) {
    // Check player has enough resources and is alive
    let player = match state.players.iter().find(|p| p.id == player_id && p.alive) {
        Some(p) => p,
        None => return,
    };
    if player.food < UNIT_FOOD_COST || player.material < UNIT_MATERIAL_COST {
        return;
    }
    let general_id = player.general_id;

    // Find the general unit
    let general_pos = match state.units.iter().find(|u| u.id == general_id) {
        Some(g) => g.pos,
        None => return,
    };

    // Find an adjacent in-bounds hex to spawn on.
    // Prefer hexes without existing units, but stacking is allowed.
    let neighbors = hex::neighbors(general_pos);
    let spawn_pos = neighbors
        .iter()
        .filter(|&&n| state.in_bounds(n))
        .find(|&&n| !state.units.iter().any(|u| u.pos == n))
        .or_else(|| neighbors.iter().find(|&&n| state.in_bounds(n)));

    let spawn_pos = match spawn_pos {
        Some(&pos) => pos,
        None => return, // no valid spawn position (shouldn't happen on reasonable maps)
    };

    // Deduct resources
    let player = state
        .players
        .iter_mut()
        .find(|p| p.id == player_id)
        .unwrap();
    player.food -= UNIT_FOOD_COST;
    player.material -= UNIT_MATERIAL_COST;

    // Spawn unit
    let id = state.next_unit_id;
    state.next_unit_id += 1;
    state.units.push(Unit {
        id,
        owner: player_id,
        pos: spawn_pos,
        strength: INITIAL_STRENGTH,
        move_cooldown: 0,
        engagements: Vec::new(),
        destination: None,
        is_general: false,
    });

    tracing::trace!(
        tick = state.tick,
        player = player_id,
        unit_id = id,
        pos = ?spawn_pos,
        "unit produced"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex::Axial;
    use crate::v2::mapgen::{MapConfig, generate};

    fn test_state_with_resources() -> GameState {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        });
        // Give player 0 enough resources to produce
        state.players[0].food = 50.0;
        state.players[0].material = 50.0;
        state
    }

    #[test]
    fn produce_spawns_unit() {
        let mut state = test_state_with_resources();
        let initial_units = state.units.iter().filter(|u| u.owner == 0).count();
        apply_directives(&mut state, 0, &[Directive::Produce]);
        let new_units = state.units.iter().filter(|u| u.owner == 0).count();
        assert_eq!(new_units, initial_units + 1);
    }

    #[test]
    fn produce_deducts_resources() {
        let mut state = test_state_with_resources();
        apply_directives(&mut state, 0, &[Directive::Produce]);
        assert!((state.players[0].food - 42.0).abs() < 0.01);
        assert!((state.players[0].material - 45.0).abs() < 0.01);
    }

    #[test]
    fn produce_insufficient_resources_ignored() {
        let mut state = test_state_with_resources();
        state.players[0].food = 5.0; // not enough
        let initial_units = state.units.iter().filter(|u| u.owner == 0).count();
        apply_directives(&mut state, 0, &[Directive::Produce]);
        assert_eq!(
            state.units.iter().filter(|u| u.owner == 0).count(),
            initial_units
        );
        assert!((state.players[0].food - 5.0).abs() < 0.01); // not deducted
        assert!((state.players[0].material - 50.0).abs() < 0.01);
    }

    #[test]
    fn produce_dead_general_ignored() {
        let mut state = test_state_with_resources();
        // Kill the general
        let gen_id = state.players[0].general_id;
        state.units.retain(|u| u.id != gen_id);
        let initial_units = state.units.iter().filter(|u| u.owner == 0).count();
        apply_directives(&mut state, 0, &[Directive::Produce]);
        assert_eq!(
            state.units.iter().filter(|u| u.owner == 0).count(),
            initial_units
        );
    }

    #[test]
    fn produce_spawns_near_general() {
        let mut state = test_state_with_resources();
        let gen_id = state.players[0].general_id;
        let gen_pos = state.units.iter().find(|u| u.id == gen_id).unwrap().pos;
        apply_directives(&mut state, 0, &[Directive::Produce]);
        // The newest unit should be adjacent to general
        let newest = state.units.last().unwrap();
        assert_eq!(hex::distance(newest.pos, gen_pos), 1);
    }

    #[test]
    fn produce_unique_ids() {
        let mut state = test_state_with_resources();
        state.players[0].food = 100.0;
        state.players[0].material = 100.0;
        apply_directives(
            &mut state,
            0,
            &[Directive::Produce, Directive::Produce, Directive::Produce],
        );
        let ids: Vec<u32> = state.units.iter().map(|u| u.id).collect();
        let unique: std::collections::HashSet<u32> = ids.iter().copied().collect();
        assert_eq!(ids.len(), unique.len(), "duplicate unit IDs found");
    }

    #[test]
    fn move_directive_sets_destination() {
        let mut state = test_state_with_resources();
        let unit = state
            .units
            .iter()
            .find(|u| u.owner == 0 && !u.is_general)
            .unwrap();
        let uid = unit.id;
        apply_directives(
            &mut state,
            0,
            &[Directive::Move {
                unit_id: uid,
                q: 5,
                r: 5,
            }],
        );
        let unit = state.units.iter().find(|u| u.id == uid).unwrap();
        assert_eq!(unit.destination, Some(Axial::new(5, 5)));
    }

    #[test]
    fn move_wrong_player_ignored() {
        let mut state = test_state_with_resources();
        let unit = state
            .units
            .iter()
            .find(|u| u.owner == 0 && !u.is_general)
            .unwrap();
        let uid = unit.id;
        // Player 1 tries to move player 0's unit
        apply_directives(
            &mut state,
            1,
            &[Directive::Move {
                unit_id: uid,
                q: 5,
                r: 5,
            }],
        );
        let unit = state.units.iter().find(|u| u.id == uid).unwrap();
        assert!(unit.destination.is_none());
    }

    #[test]
    fn multiple_produces_if_resources_allow() {
        let mut state = test_state_with_resources();
        state.players[0].food = 25.0; // enough for 3 food-wise
        state.players[0].material = 12.0; // enough for 2 material-wise
        let initial = state.units.iter().filter(|u| u.owner == 0).count();
        apply_directives(
            &mut state,
            0,
            &[Directive::Produce, Directive::Produce, Directive::Produce],
        );
        let final_count = state.units.iter().filter(|u| u.owner == 0).count();
        assert_eq!(final_count, initial + 2); // only 2 produced
        assert!(state.players[0].material < UNIT_MATERIAL_COST); // can't afford a third
    }
}
