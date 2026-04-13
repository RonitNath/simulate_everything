use slotmap::Key;
use std::collections::HashMap;

use super::hex;
use super::state::{GameState, UnitKey};
use super::{DAMAGE_RATE, DISENGAGE_PENALTY};

/// Engage unit `attacker_id` with `target_id`. Both must be adjacent and the shared
/// edge must be free on both units. Returns true if engagement was created.
pub fn engage(state: &mut GameState, attacker_id: UnitKey, target_id: UnitKey) -> bool {
    let (attacker_owner, attacker_pos) = match state.units.get(attacker_id) {
        Some(unit) => (unit.owner, unit.pos),
        None => return false,
    };
    let target = match state.units.get(target_id) {
        Some(unit) => unit,
        None => return false,
    };
    if attacker_owner == target.owner {
        return false;
    }
    let target_pos = target.pos;

    let edge = match hex::shared_edge(attacker_pos, target_pos) {
        Some(e) => e,
        None => return false,
    };
    let opposite_edge = (edge + 3) % 6;
    let attacker_edge_taken = state.units[attacker_id]
        .engagements
        .iter()
        .any(|e| e.edge == edge);
    let target_edge_taken = target.engagements.iter().any(|e| e.edge == opposite_edge);

    if attacker_edge_taken || target_edge_taken {
        return false;
    }

    // Create engagements on both sides
    state.units[attacker_id]
        .engagements
        .push(super::state::Engagement {
            enemy_id: target_id,
            edge,
        });
    state.units[target_id]
        .engagements
        .push(super::state::Engagement {
            enemy_id: attacker_id,
            edge: opposite_edge,
        });

    state.units[attacker_id].destination = None;
    state.units[target_id].destination = None;

    tracing::debug!(
        tick = state.tick,
        attacker = attacker_id.data().as_ffi(),
        target = target_id.data().as_ffi(),
        edge,
        attacker_owner = state.units[attacker_id].owner,
        target_owner = state.units[target_id].owner,
        "engagement created"
    );

    if let Some(log) = &mut state.game_log {
        log.record(super::gamelog::GameEvent::EngagementCreated {
            tick: state.tick,
            attacker: state.units[attacker_id].public_id,
            target: state.units[target_id].public_id,
            attacker_owner: state.units[attacker_id].owner,
            target_owner: state.units[target_id].owner,
        });
    }

    true
}

/// Disengage unit from a specific edge. Costs 50% of current strength.
/// Returns false if unit isn't engaged on that edge, or is surrounded (3+ edges).
pub fn disengage_edge(state: &mut GameState, unit_id: UnitKey, edge: u8) -> bool {
    let engagement_pos = match state.units[unit_id]
        .engagements
        .iter()
        .position(|e| e.edge == edge)
    {
        Some(p) => p,
        None => return false,
    };

    // Surrounded (3+ engagements) cannot disengage
    if state.units[unit_id].engagements.len() >= 3 {
        return false;
    }

    let enemy_id = state.units[unit_id].engagements[engagement_pos].enemy_id;
    let opposite_edge = (edge + 3) % 6;

    state.units[unit_id].strength *= 1.0 - DISENGAGE_PENALTY;

    state.units[unit_id].engagements.remove(engagement_pos);

    if let Some(enemy) = state.units.get_mut(enemy_id) {
        enemy
            .engagements
            .retain(|e| !(e.enemy_id == unit_id && e.edge == opposite_edge));
    }

    true
}

/// Disengage unit from ALL edges at once. Costs 50% of current strength total (not per-edge).
/// Returns false if unit is surrounded (3+ edges engaged).
pub fn disengage_all(state: &mut GameState, unit_id: UnitKey) -> bool {
    if !state.units.contains_key(unit_id) || state.units[unit_id].engagements.is_empty() {
        return false;
    }

    if state.units[unit_id].engagements.len() >= 3 {
        return false;
    }

    state.units[unit_id].strength *= 1.0 - DISENGAGE_PENALTY;

    let engagements: Vec<_> = state.units[unit_id].engagements.clone();

    state.units[unit_id].engagements.clear();

    for eng in &engagements {
        let opposite_edge = (eng.edge + 3) % 6;
        if let Some(enemy) = state.units.get_mut(eng.enemy_id) {
            enemy
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
    let unit_info: HashMap<UnitKey, (f32, usize, super::hex::Axial)> = state
        .units
        .iter()
        .map(|(id, u)| (id, (u.strength, u.engagements.len(), u.pos)))
        .collect();

    // Compute damage received by each unit
    // damage_received[U] = sum over U's engagements of (E.strength * DAMAGE_RATE * E_effectiveness)
    // where E_effectiveness = 1/sqrt(E.engagements.len())
    let mut damage: HashMap<UnitKey, f32> = HashMap::new();

    for (unit_id, unit) in &state.units {
        for eng in &unit.engagements {
            if let Some(&(enemy_str, enemy_n, enemy_pos)) = unit_info.get(&eng.enemy_id)
                && enemy_n > 0
            {
                let unit_height = state.cell_at(unit.pos).map(|c| c.height).unwrap_or(0.0);
                let enemy_height = state.cell_at(enemy_pos).map(|c| c.height).unwrap_or(0.0);
                let enemy_eff = 1.0 / (enemy_n as f32).sqrt();
                let uphill_penalty = (unit_height - enemy_height).max(0.0) * 0.2;
                *damage.entry(unit_id).or_insert(0.0) +=
                    enemy_str * DAMAGE_RATE * enemy_eff * (1.0 - uphill_penalty).clamp(0.5, 1.0);
            }
        }
    }

    for (unit_id, unit) in &mut state.units {
        if let Some(&dmg) = damage.get(&unit_id) {
            unit.strength -= dmg;
        }
    }
}

/// Clear engagements that reference dead units (strength <= 0) or non-existent units.
pub fn cleanup_engagements(state: &mut GameState) {
    let dead_ids: Vec<UnitKey> = state
        .units
        .iter()
        .filter(|(_, u)| u.strength <= 0.0)
        .map(|(id, _)| id)
        .collect();

    for (_, unit) in &mut state.units {
        unit.engagements.retain(|e| !dead_ids.contains(&e.enemy_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::INITIAL_STRENGTH;
    use crate::v2::hex::Axial;
    use crate::v2::spatial::SpatialIndex;
    use crate::v2::state::*;
    use bitvec::vec::BitVec;
    use slotmap::SlotMap;

    fn combat_state(units: Vec<Unit>) -> GameState {
        let width = 10;
        let height = 10;
        let grid = vec![
            Cell {
                terrain_value: 1.0,
                material_value: 1.0,
                food_stockpile: 0.0,
                material_stockpile: 0.0,
                has_depot: false,
                road_level: 0,
                height: 0.5,
                moisture: 0.5,
                biome: Biome::Grassland,
                is_river: false,
                water_access: 0.0,
                region_id: 0,
                stockpile_owner: None,
            };
            width * height
        ];
        let mut unit_map = SlotMap::with_key();
        for unit in units {
            unit_map.insert(unit);
        }
        let players = vec![
            Player {
                id: 0,
                food: 0.0,
                material: 0.0,
                alive: true,
            },
            Player {
                id: 1,
                food: 0.0,
                material: 0.0,
                alive: true,
            },
        ];
        let total_cells = width * height;
        let mut state = GameState {
            width,
            height,
            grid,
            units: unit_map,
            players,
            population: SlotMap::with_key(),
            convoys: SlotMap::with_key(),
            settlements: SlotMap::with_key(),
            regions: Vec::new(),
            tick: 0,
            next_unit_id: 300,
            next_pop_id: 0,
            next_convoy_id: 0,
            next_settlement_id: 0,
            scouted: vec![vec![true; total_cells]; 2],
            spatial: SpatialIndex::new(width, height),
            dirty_hexes: BitVec::repeat(false, total_cells),
            hex_revisions: vec![0; total_cells],
            next_hex_revision: 0,
            territory_cache: vec![None; total_cells],
            #[cfg(debug_assertions)]
            tick_accumulator: Some(TickAccumulator::default()),
            game_log: None,
        };
        state.rebuild_spatial();
        state
    }

    fn make_unit(id: u32, owner: u8, pos: Axial) -> Unit {
        Unit {
            public_id: id,
            owner,
            pos,
            strength: INITIAL_STRENGTH,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
        }
    }

    fn unit_key(state: &GameState, public_id: u32) -> UnitKey {
        state.unit_key_by_public_id(public_id).unwrap()
    }

    fn unit_ref(state: &GameState, public_id: u32) -> &Unit {
        state.unit_by_public_id(public_id).unwrap()
    }

    fn unit_mut(state: &mut GameState, public_id: u32) -> &mut Unit {
        state.unit_by_public_id_mut(public_id).unwrap()
    }

    #[test]
    fn engage_adjacent_units() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2); // E neighbor of (2,2)
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);

        assert!(engage(&mut state, a, b));
        assert_eq!(unit_ref(&state, 1).engagements.len(), 1);
        assert_eq!(unit_ref(&state, 2).engagements.len(), 1);
        // Edges should be opposite
        let e_a = unit_ref(&state, 1).engagements[0].edge;
        let e_b = unit_ref(&state, 2).engagements[0].edge;
        assert_eq!((e_a + 3) % 6, e_b);
    }

    #[test]
    fn engage_non_adjacent_fails() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(5, 5);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        assert!(!engage(&mut state, a, b));
    }

    #[test]
    fn engage_same_player_fails() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![
            make_unit(1, 0, a_pos),
            make_unit(2, 0, b_pos), // same player
        ]);
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        assert!(!engage(&mut state, a, b));
    }

    #[test]
    fn engage_clears_destination() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        unit_mut(&mut state, 1).destination = Some(Axial::new(5, 5));
        unit_mut(&mut state, 2).destination = Some(Axial::new(0, 0));
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);

        engage(&mut state, a, b);

        assert!(unit_ref(&state, 1).destination.is_none());
        assert!(unit_ref(&state, 2).destination.is_none());
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
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        let c = unit_key(&state, 3);
        assert!(engage(&mut state, a, b));
        // A's edge toward b_pos is now occupied; C is at same pos so same edge
        assert!(!engage(&mut state, a, c));
    }

    #[test]
    fn engaged_units_dont_move() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        unit_mut(&mut state, 1).destination = Some(Axial::new(8, 8));
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);

        engage(&mut state, a, b);
        assert!(unit_ref(&state, 1).destination.is_none());
    }

    #[test]
    fn resolve_1v1_damage() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        engage(&mut state, a, b);

        resolve_combat(&mut state);

        // Both at 100 strength, DAMAGE_RATE=0.05, 1 engagement each (eff=1.0)
        // Each takes 100 * 0.05 * 1.0 = 5.0 damage
        let a = unit_ref(&state, 1);
        let b = unit_ref(&state, 2);
        assert!(
            (a.strength - 95.0).abs() < 0.01,
            "a.strength = {}",
            a.strength
        );
        assert!(
            (b.strength - 95.0).abs() < 0.01,
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
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        let x = unit_key(&state, 3);
        engage(&mut state, a, x); // A engages X
        engage(&mut state, b, x); // B engages X

        // X has 2 engagements, eff_X = 1/sqrt(2) ≈ 0.707
        // A has 1 engagement, eff_A = 1.0
        // B has 1 engagement, eff_B = 1.0
        //
        // damage to X = A.str * DR * eff_A + B.str * DR * eff_B = 5.0 + 5.0 = 10.0
        // damage to A from X = X.str * DR * eff_X = 100 * 0.05 * 0.707 ≈ 3.536
        // damage to B from X = X.str * DR * eff_X ≈ 3.536

        resolve_combat(&mut state);

        let a = unit_ref(&state, 1);
        let b = unit_ref(&state, 2);
        let x = unit_ref(&state, 3);

        assert!(
            (x.strength - 90.0).abs() < 0.01,
            "x.strength = {}",
            x.strength
        );
        assert!(
            (a.strength - 96.464).abs() < 0.02,
            "a.strength = {}",
            a.strength
        );
        assert!(
            (b.strength - 96.464).abs() < 0.02,
            "b.strength = {}",
            b.strength
        );
    }

    #[test]
    fn disengage_edge_costs_50_percent() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        engage(&mut state, a, b);
        let edge = unit_ref(&state, 1).engagements[0].edge;

        disengage_edge(&mut state, a, edge);

        let a = unit_ref(&state, 1);
        assert!(
            (a.strength - 70.0).abs() < 0.01,
            "a.strength = {}",
            a.strength
        );
        assert!(a.engagements.is_empty());

        let b = unit_ref(&state, 2);
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
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        let x = unit_key(&state, 3);
        engage(&mut state, a, x);
        engage(&mut state, b, x);
        assert_eq!(unit_ref(&state, 3).engagements.len(), 2);

        disengage_all(&mut state, x);

        let x = unit_ref(&state, 3);
        assert!(
            (x.strength - 70.0).abs() < 0.01,
            "x.strength = {} (should be 70)",
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
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        let c = unit_key(&state, 3);
        let x = unit_key(&state, 4);
        engage(&mut state, a, x);
        engage(&mut state, b, x);
        engage(&mut state, c, x);
        assert_eq!(unit_ref(&state, 4).engagements.len(), 3);

        let edge = unit_ref(&state, 4).engagements[0].edge;
        assert!(!disengage_edge(&mut state, x, edge));
        assert!(!disengage_all(&mut state, x));
        // Strength unchanged
        assert!((unit_ref(&state, 4).strength - 100.0).abs() < 0.01);
    }

    #[test]
    fn death_clears_engagements() {
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        let mut state = combat_state(vec![make_unit(1, 0, a_pos), make_unit(2, 1, b_pos)]);
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        engage(&mut state, a, b);

        // Kill unit 1
        unit_mut(&mut state, 1).strength = 0.0;

        // cleanup_engagements should clear unit 2's engagement
        cleanup_engagements(&mut state);

        let b = unit_ref(&state, 2);
        assert!(b.engagements.is_empty());
    }
}
