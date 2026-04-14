use slotmap::Key;
use std::collections::HashMap;

use super::hex;
use super::gamelog::EngagementEndReason;
use super::state::{GameState, UnitKey};
use super::{DAMAGE_RATE, DISENGAGE_PENALTY};

fn record_engagement_end(
    state: &mut GameState,
    unit_id: UnitKey,
    enemy_id: UnitKey,
    reason: EngagementEndReason,
) {
    let Some(unit) = state.units.get(unit_id) else {
        return;
    };
    let Some(enemy) = state.units.get(enemy_id) else {
        return;
    };
    let (unit_public_id, unit_owner) = (unit.public_id, unit.owner);
    let (enemy_public_id, enemy_owner) = (enemy.public_id, enemy.owner);
    if let Some(log) = &mut state.game_log {
        log.record_engagement_ended(
            state.tick,
            unit_public_id,
            enemy_public_id,
            unit_owner,
            enemy_owner,
            reason,
        );
    }
}

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

    record_engagement_end(state, unit_id, enemy_id, EngagementEndReason::DisengageEdge);
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

    for eng in &engagements {
        record_engagement_end(state, unit_id, eng.enemy_id, EngagementEndReason::DisengageAll);
    }
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

    let mut stale_pairs = Vec::new();
    for (unit_id, unit) in state.units.iter() {
        for engagement in &unit.engagements {
            if !dead_ids.contains(&engagement.enemy_id) {
                continue;
            }
            let Some(enemy) = state.units.get(engagement.enemy_id) else {
                continue;
            };
            let pair = if unit.public_id <= enemy.public_id {
                (unit_id, engagement.enemy_id)
            } else {
                (engagement.enemy_id, unit_id)
            };
            if !stale_pairs.contains(&pair) {
                stale_pairs.push(pair);
            }
        }
    }
    for (unit_id, enemy_id) in stale_pairs {
        record_engagement_end(state, unit_id, enemy_id, EngagementEndReason::Death);
    }

    for (_, unit) in &mut state.units {
        unit.engagements.retain(|e| !dead_ids.contains(&e.enemy_id));
    }
}

// ---------------------------------------------------------------------------
// Entity-level facing-based combat (A3)
// ---------------------------------------------------------------------------

use super::state::EntityKey;
use super::{
    DAMAGE_PER_TICK, FRONT_MODIFIER, REAR_MODIFIER, SHIELD_ARC_HALF, SIDE_MODIFIER,
};
use std::f32::consts::PI;

/// Normalize an angle difference to [0, PI].
fn angle_diff(a: f32, b: f32) -> f32 {
    let mut d = (a - b).abs() % (2.0 * PI);
    if d > PI {
        d = 2.0 * PI - d;
    }
    d
}

/// Compute the facing modifier for an attack.
/// attack_angle is the direction from attacker to defender (atan2).
/// When defender faces toward attacker, diff ≈ PI (frontal/shield).
/// When defender faces away, diff ≈ 0 (rear).
fn facing_modifier(attack_angle: f32, defender_facing: f32) -> f32 {
    let diff = angle_diff(attack_angle, defender_facing);
    if diff >= PI - SHIELD_ARC_HALF {
        FRONT_MODIFIER // defender's shield faces the attack
    } else if diff >= PI / 2.0 {
        SIDE_MODIFIER
    } else {
        REAR_MODIFIER // defender faces away from attacker
    }
}

/// Snapshot of a combatant entity for damage computation.
struct CombatantSnapshot {
    key: EntityKey,
    pos: super::hex::Axial,
    owner: u8,
    facing: f32,
    combat_skill: f32,
}

/// Per-tick entity combat resolution. No engagement lock — every combatant
/// attacks hostile entities on the same or adjacent hexes each tick. Facing
/// auto-updates toward nearest enemy (simple heuristic until B4 tactical layer).
pub fn entity_resolve_combat(state: &mut GameState) {
    // Snapshot all combatant entities (person + combatant + pos + owner).
    let snapshots: Vec<CombatantSnapshot> = state
        .entities
        .iter()
        .filter_map(|(key, e)| {
            let person = e.person.as_ref()?;
            let combatant = e.combatant.as_ref()?;
            let pos = state.entity_hex(e)?;
            let owner = e.owner?;
            Some(CombatantSnapshot {
                key,
                pos,
                owner,
                facing: combatant.facing,
                combat_skill: person.combat_skill,
            })
        })
        .collect();

    if snapshots.is_empty() {
        return;
    }

    // Auto-face: each combatant faces nearest enemy.
    for snap in &snapshots {
        let nearest_enemy = snapshots
            .iter()
            .filter(|other| other.owner != snap.owner)
            .filter(|other| hex::distance(snap.pos, other.pos) <= 1)
            .min_by_key(|other| {
                let (ax, ay) = hex::axial_to_pixel(snap.pos);
                let (bx, by) = hex::axial_to_pixel(other.pos);
                let dx = bx - ax;
                let dy = by - ay;
                // Use squared distance for comparison (avoid sqrt).
                ((dx * dx + dy * dy) * 1000.0) as i64
            });
        if let Some(enemy) = nearest_enemy {
            let new_facing = if snap.pos == enemy.pos {
                // Same hex: keep current facing (or could randomize — keep simple)
                snap.facing
            } else {
                let (ax, ay) = hex::axial_to_pixel(snap.pos);
                let (bx, by) = hex::axial_to_pixel(enemy.pos);
                (by - ay).atan2(bx - ax)
            };
            if let Some(entity) = state.entities.get_mut(snap.key) {
                if let Some(combatant) = entity.combatant.as_mut() {
                    combatant.facing = new_facing;
                }
            }
        }
    }

    // Re-snapshot facing after auto-face update.
    let updated_snapshots: Vec<CombatantSnapshot> = state
        .entities
        .iter()
        .filter_map(|(key, e)| {
            let person = e.person.as_ref()?;
            let combatant = e.combatant.as_ref()?;
            let pos = state.entity_hex(e)?;
            let owner = e.owner?;
            Some(CombatantSnapshot {
                key,
                pos,
                owner,
                facing: combatant.facing,
                combat_skill: person.combat_skill,
            })
        })
        .collect();

    // Compute damage for each defender from all attackers.
    let mut damage_map: HashMap<EntityKey, f32> = HashMap::new();

    for attacker in &updated_snapshots {
        if attacker.combat_skill <= 0.0 {
            continue;
        }
        for defender in &updated_snapshots {
            if defender.owner == attacker.owner {
                continue; // no friendly fire
            }
            let dist = hex::distance(attacker.pos, defender.pos);
            if dist > 1 {
                continue; // out of range
            }

            let attack_angle = if attacker.pos == defender.pos {
                // Same hex: attack in attacker's facing direction
                attacker.facing
            } else {
                let (ax, ay) = hex::axial_to_pixel(attacker.pos);
                let (dx, dy) = hex::axial_to_pixel(defender.pos);
                (dy - ay).atan2(dx - ax)
            };

            let modifier = facing_modifier(attack_angle, defender.facing);
            let dmg = attacker.combat_skill * DAMAGE_PER_TICK * modifier;
            *damage_map.entry(defender.key).or_insert(0.0) += dmg;
        }
    }

    // Apply damage to Person.health.
    for (key, dmg) in &damage_map {
        if let Some(entity) = state.entities.get_mut(*key) {
            if let Some(person) = entity.person.as_mut() {
                person.health = (person.health - dmg).max(0.0);
            }
        }
    }
}

/// Remove dead entities (Person.health <= 0) and clean up containment links.
pub fn entity_cleanup_dead(state: &mut GameState) {
    let dead_keys: Vec<EntityKey> = state
        .entities
        .iter()
        .filter(|(_, e)| e.person.as_ref().is_some_and(|p| p.health <= 0.0))
        .map(|(key, _)| key)
        .collect();

    if dead_keys.is_empty() {
        return;
    }

    // Remove dead keys from container.contains lists.
    for &dead_key in &dead_keys {
        let container_key = state
            .entities
            .get(dead_key)
            .and_then(|e| e.contained_in);
        if let Some(ck) = container_key {
            if let Some(container) = state.entities.get_mut(ck) {
                container.contains.retain(|&k| k != dead_key);
            }
        }
    }

    // Remove dead entities.
    for &dead_key in &dead_keys {
        state.entities.remove(dead_key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::gamelog::{EngagementEndReason, GameEvent, GameLog};
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
            entities: SlotMap::with_key(),
            next_entity_id: 0,
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
            rations: crate::v2::MAX_RATIONS,
            half_rations: false,
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
        state.game_log = Some(GameLog::new());
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
        assert!(state.game_log.as_ref().is_some_and(|log| {
            log.events.iter().any(|event| {
                matches!(
                    event,
                    GameEvent::EngagementEnded {
                        unit_a,
                        unit_b,
                        reason: EngagementEndReason::DisengageEdge,
                        ..
                    } if (*unit_a, *unit_b) == (1, 2)
                )
            })
        }));
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
        state.game_log = Some(GameLog::new());
        let a = unit_key(&state, 1);
        let b = unit_key(&state, 2);
        engage(&mut state, a, b);

        // Kill unit 1
        unit_mut(&mut state, 1).strength = 0.0;

        // cleanup_engagements should clear unit 2's engagement
        cleanup_engagements(&mut state);

        let b = unit_ref(&state, 2);
        assert!(b.engagements.is_empty());
        assert!(state.game_log.as_ref().is_some_and(|log| {
            log.events.iter().any(|event| {
                matches!(
                    event,
                    GameEvent::EngagementEnded {
                        unit_a,
                        unit_b,
                        reason: EngagementEndReason::Death,
                        ..
                    } if (*unit_a, *unit_b) == (1, 2)
                )
            })
        }));
    }

    // -----------------------------------------------------------------------
    // Entity facing-based combat tests (A3)
    // -----------------------------------------------------------------------

    /// Helper: create a GameState with entity combatants but no legacy units.
    fn entity_combat_state() -> GameState {
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
        let total_cells = width * height;
        let mut state = GameState {
            width,
            height,
            grid,
            units: SlotMap::with_key(),
            players: vec![
                Player { id: 0, food: 0.0, material: 0.0, alive: true },
                Player { id: 1, food: 0.0, material: 0.0, alive: true },
            ],
            population: SlotMap::with_key(),
            convoys: SlotMap::with_key(),
            settlements: SlotMap::with_key(),
            regions: Vec::new(),
            tick: 0,
            next_unit_id: 0,
            next_pop_id: 0,
            next_convoy_id: 0,
            next_settlement_id: 0,
            entities: SlotMap::with_key(),
            next_entity_id: 0,
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

    /// Spawn a combatant entity at pos with given owner, facing, and combat_skill.
    fn spawn_combatant(
        state: &mut GameState,
        pos: Axial,
        owner: u8,
        facing: f32,
        combat_skill: f32,
    ) -> EntityKey {
        state.spawn_entity(Entity {
            id: 0,
            pos: Some(pos),
            owner: Some(owner),
            contained_in: None,
            contains: vec![],
            person: Some(Person {
                health: 1.0,
                combat_skill,
                role: Role::Soldier,
            }),
            mobile: Some(Mobile {
                speed: 1.0,
                move_cooldown: 0,
                destination: None,
                route: vec![],
            }),
            vision: Some(Vision { radius: 5 }),
            combatant: Some(Combatant {
                engaged_with: vec![],
                facing,
            }),
            resource: None,
            structure: None,
        })
    }

    #[test]
    fn entity_head_on_symmetric_damage() {
        let mut state = entity_combat_state();
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2); // E neighbor
        // Face each other
        let a_to_b = {
            let (ax, ay) = hex::axial_to_pixel(a_pos);
            let (bx, by) = hex::axial_to_pixel(b_pos);
            (by - ay).atan2(bx - ax)
        };
        let b_to_a = {
            let (ax, ay) = hex::axial_to_pixel(a_pos);
            let (bx, by) = hex::axial_to_pixel(b_pos);
            (ay - by).atan2(ax - bx)
        };
        let ka = spawn_combatant(&mut state, a_pos, 0, a_to_b, 1.0);
        let kb = spawn_combatant(&mut state, b_pos, 1, b_to_a, 1.0);
        state.rebuild_spatial();

        entity_resolve_combat(&mut state);

        let a_health = state.entities[ka].person.as_ref().unwrap().health;
        let b_health = state.entities[kb].person.as_ref().unwrap().health;
        // Both face each other -> front attack -> FRONT_MODIFIER
        let expected_dmg = 1.0 * crate::v2::DAMAGE_PER_TICK * crate::v2::FRONT_MODIFIER;
        assert!(
            (a_health - (1.0 - expected_dmg)).abs() < 0.001,
            "a_health = {a_health}, expected {}",
            1.0 - expected_dmg
        );
        // Symmetric
        assert!(
            (a_health - b_health).abs() < 0.001,
            "damage should be symmetric: a={a_health}, b={b_health}"
        );
    }

    #[test]
    fn entity_rear_attack_1_5x() {
        // Attack angle = direction from attacker to defender = east (0).
        // Defender also faces east (0) = faces AWAY from attacker.
        // diff = |0 - 0| = 0 → REAR (1.5x).
        let attack_angle = 0.0;
        let defender_facing_away = 0.0;
        let modifier = facing_modifier(attack_angle, defender_facing_away);
        assert!(
            (modifier - crate::v2::REAR_MODIFIER).abs() < 0.001,
            "rear modifier = {modifier}, expected {}",
            crate::v2::REAR_MODIFIER
        );
    }

    #[test]
    fn entity_shield_arc_0_3x() {
        // Attack direction = east (0). Defender faces west (PI) = faces TOWARD attacker.
        // diff = |0 - PI| = PI → within shield arc (PI >= PI - PI/6) → FRONT (0.3x).
        let attack_angle = 0.0;
        let defender_facing_toward = std::f32::consts::PI;
        let modifier = facing_modifier(attack_angle, defender_facing_toward);
        assert!(
            (modifier - crate::v2::FRONT_MODIFIER).abs() < 0.001,
            "front modifier = {modifier}, expected {}",
            crate::v2::FRONT_MODIFIER
        );
    }

    #[test]
    fn entity_side_attack_0_7x() {
        // Attack direction = east (0). Defender faces south (PI/2).
        // diff = |0 - PI/2| = PI/2 → SIDE (>= PI/2 but < PI - PI/6) → 0.7x.
        let attack_angle = 0.0;
        let defender_facing_south = std::f32::consts::FRAC_PI_2;
        let modifier = facing_modifier(attack_angle, defender_facing_south);
        assert!(
            (modifier - crate::v2::SIDE_MODIFIER).abs() < 0.001,
            "side modifier = {modifier}, expected {}",
            crate::v2::SIDE_MODIFIER
        );
    }

    #[test]
    fn entity_flanking_more_total_damage() {
        let mut state = entity_combat_state();
        // Defender at (3,2), two attackers from opposite sides
        let def_pos = Axial::new(3, 2);
        let atk1_pos = Axial::new(2, 2); // W
        let atk2_pos = Axial::new(4, 2); // E

        // Defender faces west (toward atk1). atk2 attacks from behind.
        let facing_west = std::f32::consts::PI;
        let facing_east = 0.0;
        let _ka1 = spawn_combatant(&mut state, atk1_pos, 0, facing_east, 1.0);
        let _ka2 = spawn_combatant(&mut state, atk2_pos, 0, facing_west, 1.0);
        let kd = spawn_combatant(&mut state, def_pos, 1, facing_west, 1.0);
        state.rebuild_spatial();

        entity_resolve_combat(&mut state);

        let def_health = state.entities[kd].person.as_ref().unwrap().health;
        // After auto-face, defender faces nearest enemy (could be either).
        // One attacker hits front (0.3x), other hits rear (1.5x).
        // Total damage >= 1.0 * 0.02 * (0.3 + 1.5) = 0.036
        // This is more than 2x frontal: 2 * 0.02 * 0.3 = 0.012
        let two_frontal = 2.0 * crate::v2::DAMAGE_PER_TICK * crate::v2::FRONT_MODIFIER;
        let actual_damage = 1.0 - def_health;
        assert!(
            actual_damage > two_frontal,
            "flanking damage {actual_damage} should exceed 2x frontal {two_frontal}"
        );
    }

    #[test]
    fn entity_death_removes_entity() {
        let mut state = entity_combat_state();
        let pos = Axial::new(3, 3);
        let key = spawn_combatant(&mut state, pos, 0, 0.0, 1.0);
        // Set health to 0
        state.entities[key].person.as_mut().unwrap().health = 0.0;

        entity_cleanup_dead(&mut state);

        assert!(
            state.entities.get(key).is_none(),
            "dead entity should be removed"
        );
    }

    #[test]
    fn entity_death_cleans_containment() {
        let mut state = entity_combat_state();
        let struct_pos = Axial::new(3, 3);
        let struct_key = state.spawn_entity(Entity {
            id: 0,
            pos: Some(struct_pos),
            owner: Some(0),
            contained_in: None,
            contains: vec![],
            person: None,
            mobile: None,
            vision: None,
            combatant: None,
            resource: None,
            structure: Some(Structure {
                structure_type: StructureType::Village,
                build_progress: 1.0,
                health: 1.0,
                capacity: 100,
            }),
        });
        let person_key = spawn_combatant(&mut state, struct_pos, 0, 0.0, 1.0);
        // Set up containment
        state.entities[person_key].contained_in = Some(struct_key);
        state.entities[person_key].pos = None;
        state.entities[struct_key].contains.push(person_key);
        // Kill the person
        state.entities[person_key].person.as_mut().unwrap().health = 0.0;

        entity_cleanup_dead(&mut state);

        assert!(state.entities.get(person_key).is_none());
        let structure = &state.entities[struct_key];
        assert!(
            structure.contains.is_empty(),
            "dead entity should be removed from container.contains"
        );
    }

    #[test]
    fn entity_no_friendly_fire() {
        let mut state = entity_combat_state();
        let a_pos = Axial::new(2, 2);
        let b_pos = Axial::new(3, 2);
        // Same owner
        let ka = spawn_combatant(&mut state, a_pos, 0, 0.0, 1.0);
        let kb = spawn_combatant(&mut state, b_pos, 0, std::f32::consts::PI, 1.0);
        state.rebuild_spatial();

        entity_resolve_combat(&mut state);

        let a_health = state.entities[ka].person.as_ref().unwrap().health;
        let b_health = state.entities[kb].person.as_ref().unwrap().health;
        assert!(
            (a_health - 1.0).abs() < 0.001,
            "no friendly fire: a_health = {a_health}"
        );
        assert!(
            (b_health - 1.0).abs() < 0.001,
            "no friendly fire: b_health = {b_health}"
        );
    }

    #[test]
    fn entity_same_hex_combat() {
        let mut state = entity_combat_state();
        let pos = Axial::new(3, 3);
        // Both on same hex, different owners
        let ka = spawn_combatant(&mut state, pos, 0, 0.0, 1.0);
        let kb = spawn_combatant(&mut state, pos, 1, std::f32::consts::PI, 1.0);
        state.rebuild_spatial();

        entity_resolve_combat(&mut state);

        let a_health = state.entities[ka].person.as_ref().unwrap().health;
        let b_health = state.entities[kb].person.as_ref().unwrap().health;
        // Both should take damage (same hex uses attacker facing as attack angle)
        assert!(a_health < 1.0, "a should take damage: {a_health}");
        assert!(b_health < 1.0, "b should take damage: {b_health}");
    }
}
