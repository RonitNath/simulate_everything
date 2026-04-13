use std::collections::HashMap;

use super::hex;
use super::state::GameState;
use super::{DAMAGE_RATE, DISENGAGE_PENALTY};

/// Engage unit `attacker_id` with `target_id`. Both must be adjacent and the shared
/// edge must be free on both units. Returns true if engagement was created.
pub fn engage(state: &mut GameState, attacker_id: u32, target_id: u32) -> bool {
    // Find unit indices
    let attacker_idx = match state.units.iter().position(|u| u.id == attacker_id) {
        Some(i) => i,
        None => return false,
    };
    let target_idx = match state.units.iter().position(|u| u.id == target_id) {
        Some(i) => i,
        None => return false,
    };

    // Must belong to different players
    if state.units[attacker_idx].owner == state.units[target_idx].owner {
        return false;
    }

    let attacker_pos = state.units[attacker_idx].pos;
    let target_pos = state.units[target_idx].pos;

    // Must be adjacent — shared_edge returns the edge index from attacker to target
    let edge = match hex::shared_edge(attacker_pos, target_pos) {
        Some(e) => e,
        None => return false,
    };
    let opposite_edge = (edge + 3) % 6;

    // Neither unit may already be engaged on that edge
    let attacker_edge_taken = state.units[attacker_idx]
        .engagements
        .iter()
        .any(|e| e.edge == edge);
    let target_edge_taken = state.units[target_idx]
        .engagements
        .iter()
        .any(|e| e.edge == opposite_edge);

    if attacker_edge_taken || target_edge_taken {
        return false;
    }

    // Create engagements on both sides
    state.units[attacker_idx]
        .engagements
        .push(super::state::Engagement {
            enemy_id: target_id,
            edge,
        });
    state.units[target_idx]
        .engagements
        .push(super::state::Engagement {
            enemy_id: attacker_id,
            edge: opposite_edge,
        });

    // Engaged units cannot move
    state.units[attacker_idx].destination = None;
    state.units[target_idx].destination = None;

    true
}

/// Disengage unit from a specific edge. Costs 50% of current strength.
/// Returns false if unit isn't engaged on that edge, or is surrounded (3+ edges).
pub fn disengage_edge(state: &mut GameState, unit_id: u32, edge: u8) -> bool {
    let unit_idx = match state.units.iter().position(|u| u.id == unit_id) {
        Some(i) => i,
        None => return false,
    };

    // Must be engaged on this specific edge
    let engagement_pos = match state.units[unit_idx]
        .engagements
        .iter()
        .position(|e| e.edge == edge)
    {
        Some(p) => p,
        None => return false,
    };

    // Surrounded (3+ engagements) cannot disengage
    if state.units[unit_idx].engagements.len() >= 3 {
        return false;
    }

    let enemy_id = state.units[unit_idx].engagements[engagement_pos].enemy_id;
    let opposite_edge = (edge + 3) % 6;

    // Apply 50% strength penalty to the disengaging unit
    state.units[unit_idx].strength *= 1.0 - DISENGAGE_PENALTY;

    // Remove this engagement from the unit
    state.units[unit_idx].engagements.remove(engagement_pos);

    // Find the opponent and remove their corresponding engagement
    if let Some(enemy_idx) = state.units.iter().position(|u| u.id == enemy_id) {
        state.units[enemy_idx]
            .engagements
            .retain(|e| !(e.enemy_id == unit_id && e.edge == opposite_edge));
    }

    true
}

/// Disengage unit from ALL edges at once. Costs 50% of current strength total (not per-edge).
/// Returns false if unit is surrounded (3+ edges engaged).
pub fn disengage_all(state: &mut GameState, unit_id: u32) -> bool {
    let unit_idx = match state.units.iter().position(|u| u.id == unit_id) {
        Some(i) => i,
        None => return false,
    };

    if state.units[unit_idx].engagements.is_empty() {
        return false;
    }

    // Surrounded (3+ engagements) cannot disengage
    if state.units[unit_idx].engagements.len() >= 3 {
        return false;
    }

    // Apply 50% strength penalty once
    state.units[unit_idx].strength *= 1.0 - DISENGAGE_PENALTY;

    // Collect all engagements to clean up opponents
    let engagements: Vec<_> = state.units[unit_idx].engagements.clone();

    // Clear all engagements on this unit
    state.units[unit_idx].engagements.clear();

    // Remove corresponding engagements from all opponents
    for eng in &engagements {
        let opposite_edge = (eng.edge + 3) % 6;
        if let Some(enemy_idx) = state.units.iter().position(|u| u.id == eng.enemy_id) {
            state.units[enemy_idx]
                .engagements
                .retain(|e| !(e.enemy_id == unit_id && e.edge == opposite_edge));
        }
    }

    true
}

/// Resolve one tick of combat damage for all engaged units.
///
/// Each unit's outgoing effectiveness = 1/sqrt(N) where N = number of engaged edges.
/// Damage received by unit U from opponent E = E.strength * DAMAGE_RATE * E_effectiveness.
pub fn resolve_combat(state: &mut GameState) {
    // Snapshot strength and engagement count at tick start to avoid mid-tick mutations affecting damage
    let unit_info: HashMap<u32, (f32, usize)> = state
        .units
        .iter()
        .map(|u| (u.id, (u.strength, u.engagements.len())))
        .collect();

    // Compute damage received by each unit
    // damage_received[U] = sum over U's engagements of (E.strength * DAMAGE_RATE * E_effectiveness)
    // where E_effectiveness = 1/sqrt(E.engagements.len())
    let mut damage: HashMap<u32, f32> = HashMap::new();

    for unit in &state.units {
        for eng in &unit.engagements {
            if let Some(&(enemy_str, enemy_n)) = unit_info.get(&eng.enemy_id)
                && enemy_n > 0
            {
                let enemy_eff = 1.0 / (enemy_n as f32).sqrt();
                *damage.entry(unit.id).or_insert(0.0) += enemy_str * DAMAGE_RATE * enemy_eff;
            }
        }
    }

    // Apply accumulated damage
    for unit in &mut state.units {
        if let Some(&dmg) = damage.get(&unit.id) {
            unit.strength -= dmg;
        }
    }
}

/// Clear engagements that reference dead units (strength <= 0) or non-existent units.
pub fn cleanup_engagements(state: &mut GameState) {
    let dead_ids: Vec<u32> = state
        .units
        .iter()
        .filter(|u| u.strength <= 0.0)
        .map(|u| u.id)
        .collect();

    for unit in &mut state.units {
        unit.engagements.retain(|e| !dead_ids.contains(&e.enemy_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::INITIAL_STRENGTH;
    use crate::v2::hex::Axial;
    use crate::v2::state::*;

    fn combat_state(units: Vec<Unit>) -> GameState {
        let width = 10;
        let height = 10;
        let grid = vec![Cell { terrain_value: 1.0 }; width * height];
        let players = vec![
            Player {
                id: 0,
                resources: 0.0,
                general_id: 100,
                alive: true,
            },
            Player {
                id: 1,
                resources: 0.0,
                general_id: 200,
                alive: true,
            },
        ];
        GameState {
            width,
            height,
            grid,
            units,
            players,
            tick: 0,
        }
    }

    fn make_unit(id: u32, owner: u8, pos: Axial) -> Unit {
        Unit {
            id,
            owner,
            pos,
            strength: INITIAL_STRENGTH,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: false,
        }
    }

    #[test]
    fn engage_adjacent_units() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2); // E neighbor of (2,2)
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);

        assert!(engage(&mut state, 1, 2));
        assert_eq!(state.units[0].engagements.len(), 1);
        assert_eq!(state.units[1].engagements.len(), 1);
        // Edges should be opposite
        let e_a = state.units[0].engagements[0].edge;
        let e_b = state.units[1].engagements[0].edge;
        assert_eq!((e_a + 3) % 6, e_b);
    }

    #[test]
    fn engage_non_adjacent_fails() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(5, 5);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        assert!(!engage(&mut state, 1, 2));
    }

    #[test]
    fn engage_same_player_fails() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![
            make_unit(1, 0, a_pos),
            make_unit(2, 0, b_pos), // same player
        ]);
        assert!(!engage(&mut state, 1, 2));
    }

    #[test]
    fn engage_clears_destination() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        state.units[0].destination = Some(Axial::new(5, 5));
        state.units[1].destination = Some(Axial::new(0, 0));

        engage(&mut state, 1, 2);

        assert!(state.units[0].destination.is_none());
        assert!(state.units[1].destination.is_none());
    }

    #[test]
    fn engage_occupied_edge_fails() {
        // A engages B on some edge. C is at the same position as B, so shared_edge(A, C)
        // returns the same edge. That edge is already occupied on A's side.
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let c_pos = Axial::new(3, 2); // same position as B
        let mut state = combat_state(vec![
            make_unit(1, 0, a_pos),
            make_unit(2, 1, b_pos),
            make_unit(3, 1, c_pos),
        ]);
        assert!(engage(&mut state, 1, 2));
        // A's edge toward b_pos is now occupied; C is at same pos so same edge
        assert!(!engage(&mut state, 1, 3));
    }

    #[test]
    fn engaged_units_dont_move() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        state.units[0].destination = Some(Axial::new(8, 8));

        engage(&mut state, 1, 2);
        assert!(state.units[0].destination.is_none());
    }

    #[test]
    fn resolve_1v1_damage() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        engage(&mut state, 1, 2);

        resolve_combat(&mut state);

        // Both at 100 strength, DAMAGE_RATE=0.01, 1 engagement each (eff=1.0)
        // Each takes 100 * 0.01 * 1.0 = 1.0 damage
        let a = state.units.iter().find(|u| u.id == 1).unwrap();
        let b = state.units.iter().find(|u| u.id == 2).unwrap();
        assert!(
            (a.strength - 99.0).abs() < 0.01,
            "a.strength = {}",
            a.strength
        );
        assert!(
            (b.strength - 99.0).abs() < 0.01,
            "b.strength = {}",
            b.strength
        );
    }

    #[test]
    fn flanking_damage_advantage() {
        // A and B (player 0) both engage X (player 1).
        // neighbors of (3,2) in axial: NE(4,1), E(4,2), SE(3,3), SW(2,3), W(2,2), NW(3,1)
        let a_pos = Axial::new(2, 2); // W of X
        let x_pos = Axial::new(3, 2);
        let b_pos = Axial::new(3, 1); // NW of X
        let mut state = combat_state(vec![
            make_unit(1, 0, a_pos),
            make_unit(2, 0, b_pos),
            make_unit(3, 1, x_pos),
        ]);
        engage(&mut state, 1, 3); // A engages X
        engage(&mut state, 2, 3); // B engages X

        // X has 2 engagements, eff_X = 1/sqrt(2) ≈ 0.707
        // A has 1 engagement, eff_A = 1.0
        // B has 1 engagement, eff_B = 1.0
        //
        // damage to X = A.str * DR * eff_A + B.str * DR * eff_B = 1.0 + 1.0 = 2.0
        // damage to A from X = X.str * DR * eff_X ≈ 0.707
        // damage to B from X = X.str * DR * eff_X ≈ 0.707

        resolve_combat(&mut state);

        let a = state.units.iter().find(|u| u.id == 1).unwrap();
        let b = state.units.iter().find(|u| u.id == 2).unwrap();
        let x = state.units.iter().find(|u| u.id == 3).unwrap();

        assert!(
            (x.strength - 98.0).abs() < 0.01,
            "x.strength = {}",
            x.strength
        );
        assert!(
            (a.strength - 99.293).abs() < 0.02,
            "a.strength = {}",
            a.strength
        );
        assert!(
            (b.strength - 99.293).abs() < 0.02,
            "b.strength = {}",
            b.strength
        );
    }

    #[test]
    fn disengage_edge_costs_50_percent() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        engage(&mut state, 1, 2);
        let edge = state.units[0].engagements[0].edge;

        disengage_edge(&mut state, 1, edge);

        let a = state.units.iter().find(|u| u.id == 1).unwrap();
        assert!((a.strength - 50.0).abs() < 0.01);
        assert!(a.engagements.is_empty());

        let b = state.units.iter().find(|u| u.id == 2).unwrap();
        assert!(b.engagements.is_empty()); // opponent freed too
        assert!((b.strength - 100.0).abs() < 0.01); // opponent not penalized
    }

    #[test]
    fn disengage_all_costs_50_percent_once() {
        // Unit engaged on 2 edges, disengage all should cost 50% total not per-edge
        let x_pos = Axial::new(3, 2);
        let a_pos = Axial::new(2, 2); // W of X
        let b_pos = Axial::new(3, 1); // NW of X
        let mut state = combat_state(vec![
            make_unit(1, 0, a_pos),
            make_unit(2, 0, b_pos),
            make_unit(3, 1, x_pos),
        ]);
        engage(&mut state, 1, 3);
        engage(&mut state, 2, 3);
        assert_eq!(
            state
                .units
                .iter()
                .find(|u| u.id == 3)
                .unwrap()
                .engagements
                .len(),
            2
        );

        disengage_all(&mut state, 3);

        let x = state.units.iter().find(|u| u.id == 3).unwrap();
        assert!(
            (x.strength - 50.0).abs() < 0.01,
            "x.strength = {} (should be 50)",
            x.strength
        );
        assert!(x.engagements.is_empty());
    }

    #[test]
    fn surrounded_cannot_disengage() {
        // Unit engaged on 3+ edges cannot disengage
        // neighbors of (3,3): NE(4,2), E(4,3), SE(3,4), SW(2,4), W(2,3), NW(3,2)
        let x_pos = Axial::new(3, 3);
        let a_pos = Axial::new(4, 2); // NE
        let b_pos = Axial::new(4, 3); // E
        let c_pos = Axial::new(3, 4); // SE
        let mut state = combat_state(vec![
            make_unit(1, 0, a_pos),
            make_unit(2, 0, b_pos),
            make_unit(3, 0, c_pos),
            make_unit(4, 1, x_pos),
        ]);
        engage(&mut state, 1, 4);
        engage(&mut state, 2, 4);
        engage(&mut state, 3, 4);
        assert_eq!(
            state
                .units
                .iter()
                .find(|u| u.id == 4)
                .unwrap()
                .engagements
                .len(),
            3
        );

        let edge = state.units.iter().find(|u| u.id == 4).unwrap().engagements[0].edge;
        assert!(!disengage_edge(&mut state, 4, edge));
        assert!(!disengage_all(&mut state, 4));
        // Strength unchanged
        assert!((state.units.iter().find(|u| u.id == 4).unwrap().strength - 100.0).abs() < 0.01);
    }

    #[test]
    fn death_clears_engagements() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        engage(&mut state, 1, 2);

        // Kill unit 1
        state.units.iter_mut().find(|u| u.id == 1).unwrap().strength = 0.0;

        // cleanup_engagements should clear unit 2's engagement
        cleanup_engagements(&mut state);

        let b = state.units.iter().find(|u| u.id == 2).unwrap();
        assert!(b.engagements.is_empty());
    }
}
