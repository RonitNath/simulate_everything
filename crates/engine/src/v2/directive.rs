use serde::{Deserialize, Serialize};

use super::combat;
use super::hex::{self, Axial};
use super::state::{CargoType, Convoy, GameState, Population, Role, Unit};
use super::{
    CONVOY_CAPACITY, CONVOY_MOVE_COOLDOWN, DEPOT_BUILD_COST, INITIAL_STRENGTH, ROAD_LEVEL2_COST,
    ROAD_LEVEL3_COST, SOLDIER_EQUIP_COST, SOLDIER_READY_THRESHOLD, SOLDIERS_PER_UNIT,
    SETTLEMENT_THRESHOLD, SETTLER_CONVOY_SIZE, TRAIN_BATCH_SIZE, UNIT_FOOD_COST, UNIT_MATERIAL_COST,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Directive {
    Move {
        unit_id: u32,
        q: i32,
        r: i32,
    },
    Engage {
        unit_id: u32,
        target_id: u32,
    },
    DisengageEdge {
        unit_id: u32,
        edge: u8,
    },
    DisengageAll {
        unit_id: u32,
    },
    Produce,
    AssignRole {
        hex_q: i32,
        hex_r: i32,
        role: Role,
        count: u16,
    },
    TrainSoldier {
        hex_q: i32,
        hex_r: i32,
    },
    LoadConvoy {
        hex_q: i32,
        hex_r: i32,
        cargo_type: CargoType,
        amount: f32,
    },
    SendConvoy {
        convoy_id: u32,
        dest_q: i32,
        dest_r: i32,
    },
    BuildDepot {
        hex_q: i32,
        hex_r: i32,
    },
    BuildRoad {
        hex_q: i32,
        hex_r: i32,
        level: u8,
    },
    Pass,
}

pub fn apply_directives(state: &mut GameState, player_id: u8, directives: &[Directive]) {
    for directive in directives {
        apply_one(state, player_id, directive);
    }
}

fn apply_one(state: &mut GameState, player_id: u8, directive: &Directive) {
    match directive {
        Directive::Move { unit_id, q, r } => {
            let dest = Axial::new(*q, *r);
            if !state.in_bounds(dest) {
                return;
            }
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
        Directive::Produce => produce_unit(state, player_id),
        Directive::AssignRole {
            hex_q,
            hex_r,
            role,
            count,
        } => assign_role(state, player_id, Axial::new(*hex_q, *hex_r), *role, *count),
        Directive::TrainSoldier { hex_q, hex_r } => {
            train_soldiers(state, player_id, Axial::new(*hex_q, *hex_r))
        }
        Directive::LoadConvoy {
            hex_q,
            hex_r,
            cargo_type,
            amount,
        } => load_convoy(
            state,
            player_id,
            Axial::new(*hex_q, *hex_r),
            *cargo_type,
            *amount,
        ),
        Directive::SendConvoy {
            convoy_id,
            dest_q,
            dest_r,
        } => send_convoy(state, player_id, *convoy_id, Axial::new(*dest_q, *dest_r)),
        Directive::BuildDepot { hex_q, hex_r } => {
            build_depot(state, player_id, Axial::new(*hex_q, *hex_r))
        }
        Directive::BuildRoad {
            hex_q,
            hex_r,
            level,
        } => build_road(state, player_id, Axial::new(*hex_q, *hex_r), *level),
        Directive::Pass => {}
    }
}

fn owner_controls_hex(state: &GameState, player_id: u8, hex: Axial) -> bool {
    state
        .cell_at(hex)
        .map(|c| c.stockpile_owner == Some(player_id))
        .unwrap_or(false)
        || state
            .units
            .iter()
            .any(|u| u.owner == player_id && u.pos == hex)
}

fn is_settlement_hex(state: &GameState, player_id: u8, hex: Axial) -> bool {
    state.is_settlement(player_id, hex)
}

fn split_population(state: &mut GameState, idx: usize, count: u16, role: Role, training: f32) {
    if state.population[idx].count == count {
        state.population[idx].role = role;
        state.population[idx].training = training;
        return;
    }
    state.population[idx].count -= count;
    let source = state.population[idx].clone();
    state.population.push(Population {
        id: state.next_pop_id,
        hex: source.hex,
        owner: source.owner,
        count,
        role,
        training,
    });
    state.next_pop_id += 1;
}

fn merge_population(state: &mut GameState) {
    let mut merged: Vec<Population> = Vec::new();
    for pop in state.population.drain(..) {
        if pop.count == 0 {
            continue;
        }
        if let Some(existing) = merged.iter_mut().find(|p| {
            p.owner == pop.owner
                && p.hex == pop.hex
                && p.role == pop.role
                && (p.training - pop.training).abs() < 0.001
        }) {
            existing.count = existing.count.saturating_add(pop.count);
        } else {
            merged.push(pop);
        }
    }
    state.population = merged;
}

fn assign_role(state: &mut GameState, player_id: u8, hex: Axial, role: Role, count: u16) {
    if count == 0 || !owner_controls_hex(state, player_id, hex) {
        return;
    }
    let mut remaining = count;
    for idx in 0..state.population.len() {
        if remaining == 0 {
            break;
        }
        if state.population[idx].owner != player_id || state.population[idx].hex != hex {
            continue;
        }
        let current_role = state.population[idx].role;
        if current_role == role || current_role == Role::Soldier {
            continue;
        }
        let take = remaining.min(state.population[idx].count);
        split_population(state, idx, take, role, 0.0);
        remaining -= take;
    }
    merge_population(state);
}

fn train_soldiers(state: &mut GameState, player_id: u8, hex: Axial) {
    if !owner_controls_hex(state, player_id, hex) {
        return;
    }
    let affordable = state
        .cell_at(hex)
        .map(|cell| (cell.material_stockpile / SOLDIER_EQUIP_COST).floor() as u16)
        .unwrap_or(0);
    if affordable == 0 {
        return;
    }
    let batch = affordable.min(TRAIN_BATCH_SIZE);
    let Some(idx) = state
        .population
        .iter()
        .position(|p| p.owner == player_id && p.hex == hex && p.role == Role::Idle && p.count > 0)
    else {
        return;
    };
    let take = batch.min(state.population[idx].count);
    if let Some(cell) = state.cell_at_mut(hex) {
        cell.material_stockpile -= take as f32 * SOLDIER_EQUIP_COST;
    }
    split_population(state, idx, take, Role::Soldier, 0.0);
    merge_population(state);
}

fn produce_unit(state: &mut GameState, player_id: u8) {
    let player = match state.players.iter().find(|p| p.id == player_id && p.alive) {
        Some(p) => p,
        None => return,
    };
    let general_pos = match state.units.iter().find(|u| u.id == player.general_id) {
        Some(g) => g.pos,
        None => return,
    };
    let Some(cell) = state.cell_at(general_pos) else {
        return;
    };
    if cell.stockpile_owner != Some(player_id)
        || cell.food_stockpile < UNIT_FOOD_COST
        || cell.material_stockpile < UNIT_MATERIAL_COST
    {
        return;
    }

    let available_soldiers: u16 = state
        .population
        .iter()
        .filter(|p| {
            p.owner == player_id
                && p.hex == general_pos
                && p.role == Role::Soldier
                && p.training >= SOLDIER_READY_THRESHOLD
        })
        .map(|p| p.count)
        .sum();
    if available_soldiers < SOLDIERS_PER_UNIT {
        return;
    }

    let neighbors = hex::neighbors(general_pos);
    let spawn_pos = neighbors
        .iter()
        .filter(|&&n| state.in_bounds(n))
        .find(|&&n| !state.units.iter().any(|u| u.pos == n))
        .or_else(|| neighbors.iter().find(|&&n| state.in_bounds(n)));
    let Some(&spawn_pos) = spawn_pos else { return };

    let mut remaining = SOLDIERS_PER_UNIT;
    for pop in state.population.iter_mut().filter(|p| {
        p.owner == player_id
            && p.hex == general_pos
            && p.role == Role::Soldier
            && p.training >= SOLDIER_READY_THRESHOLD
    }) {
        if remaining == 0 {
            break;
        }
        let take = remaining.min(pop.count);
        pop.count -= take;
        remaining -= take;
    }
    state.population.retain(|p| p.count > 0);
    if let Some(cell) = state.cell_at_mut(general_pos) {
        cell.food_stockpile -= UNIT_FOOD_COST;
        cell.material_stockpile -= UNIT_MATERIAL_COST;
    }

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
    if let Some(cell) = state.cell_at_mut(spawn_pos) {
        cell.stockpile_owner = Some(player_id);
    }
}

fn general_pos(state: &GameState, player_id: u8) -> Option<Axial> {
    let general_id = state.players.iter().find(|p| p.id == player_id)?.general_id;
    Some(state.units.iter().find(|u| u.id == general_id)?.pos)
}

fn load_convoy(
    state: &mut GameState,
    player_id: u8,
    hex: Axial,
    cargo_type: CargoType,
    amount: f32,
) {
    if !owner_controls_hex(state, player_id, hex) || amount <= 0.0 {
        return;
    }
    if cargo_type == CargoType::Settlers {
        let _ = load_settlers(state, player_id, hex);
        return;
    }
    let Some(destination) = general_pos(state, player_id) else {
        return;
    };
    let Some(cell) = state.cell_at_mut(hex) else {
        return;
    };
    if cell.stockpile_owner != Some(player_id) {
        return;
    }
    let capacity = CONVOY_CAPACITY.min(amount);
    let cargo_amount = match cargo_type {
        CargoType::Food => {
            let amt = cell.food_stockpile.min(capacity);
            cell.food_stockpile -= amt;
            amt
        }
        CargoType::Material => {
            let amt = cell.material_stockpile.min(capacity);
            cell.material_stockpile -= amt;
            amt
        }
        CargoType::Settlers => 0.0,
    };
    if cargo_amount <= 0.0 {
        return;
    }

    state.convoys.push(Convoy {
        id: state.next_convoy_id,
        owner: player_id,
        pos: hex,
        origin: hex,
        destination,
        cargo_type,
        cargo_amount,
        capacity: CONVOY_CAPACITY,
        speed: 1.0,
        move_cooldown: CONVOY_MOVE_COOLDOWN,
        returning: false,
    });
    state.next_convoy_id += 1;
}

fn load_settlers(state: &mut GameState, player_id: u8, hex: Axial) -> bool {
    if !is_settlement_hex(state, player_id, hex) {
        return false;
    }
    let total_pop = state.population_on_hex(player_id, hex);
    if total_pop < SETTLEMENT_THRESHOLD + SETTLER_CONVOY_SIZE {
        return false;
    }
    let available_non_soldiers: u16 = state
        .population
        .iter()
        .filter(|p| p.owner == player_id && p.hex == hex && p.role != Role::Soldier)
        .map(|p| p.count)
        .sum();
    if available_non_soldiers < SETTLER_CONVOY_SIZE {
        return false;
    }
    let mut remaining = SETTLER_CONVOY_SIZE;
    for pop in state.population.iter_mut().filter(|p| {
        p.owner == player_id && p.hex == hex && p.role != Role::Soldier && p.count > 0
    }) {
        if remaining == 0 {
            break;
        }
        let take = remaining.min(pop.count);
        pop.count -= take;
        remaining -= take;
    }
    state.population.retain(|p| p.count > 0);
    if remaining > 0 {
        return false;
    }
    let destination = hex;
    state.convoys.push(Convoy {
        id: state.next_convoy_id,
        owner: player_id,
        pos: hex,
        origin: hex,
        destination,
        cargo_type: CargoType::Settlers,
        cargo_amount: SETTLER_CONVOY_SIZE as f32,
        capacity: SETTLER_CONVOY_SIZE as f32,
        speed: 1.0,
        move_cooldown: CONVOY_MOVE_COOLDOWN,
        returning: false,
    });
    state.next_convoy_id += 1;
    true
}

fn send_convoy(state: &mut GameState, player_id: u8, convoy_id: u32, dest: Axial) {
    if !state.in_bounds(dest) {
        return;
    }
    if let Some(convoy) = state
        .convoys
        .iter_mut()
        .find(|c| c.id == convoy_id && c.owner == player_id)
    {
        convoy.destination = dest;
        convoy.returning = false;
    }
}

fn build_depot(state: &mut GameState, player_id: u8, hex: Axial) {
    if !owner_controls_hex(state, player_id, hex) || !is_settlement_hex(state, player_id, hex) {
        return;
    }
    let Some(cell) = state.cell_at_mut(hex) else {
        return;
    };
    if cell.stockpile_owner != Some(player_id)
        || cell.has_depot
        || cell.material_stockpile < DEPOT_BUILD_COST
    {
        return;
    }
    cell.material_stockpile -= DEPOT_BUILD_COST;
    cell.has_depot = true;
}

fn build_road(state: &mut GameState, player_id: u8, hex: Axial, level: u8) {
    if !owner_controls_hex(state, player_id, hex) || !is_settlement_hex(state, player_id, hex) {
        return;
    }
    let Some(cell) = state.cell_at_mut(hex) else {
        return;
    };
    if cell.stockpile_owner != Some(player_id) || level <= cell.road_level || level > 3 {
        return;
    }
    let cost = match level {
        1 => 0.0,
        2 => ROAD_LEVEL2_COST,
        3 => ROAD_LEVEL3_COST,
        _ => return,
    };
    if cell.material_stockpile < cost {
        return;
    }
    cell.material_stockpile -= cost;
    cell.road_level = level;
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn produce_spawns_unit() {
        let mut state = test_state();
        let general = state
            .units
            .iter()
            .find(|u| u.owner == 0 && u.is_general)
            .unwrap()
            .pos;
        for _ in 0..2 {
            apply_directives(
                &mut state,
                0,
                &[Directive::TrainSoldier {
                    hex_q: general.q,
                    hex_r: general.r,
                }],
            );
        }
        for pop in state
            .population
            .iter_mut()
            .filter(|p| p.owner == 0 && p.role == Role::Soldier)
        {
            pop.training = SOLDIER_READY_THRESHOLD;
        }
        let initial = state.units.iter().filter(|u| u.owner == 0).count();
        apply_directives(&mut state, 0, &[Directive::Produce]);
        assert_eq!(
            state.units.iter().filter(|u| u.owner == 0).count(),
            initial + 1
        );
    }

    #[test]
    fn assign_role_changes_population() {
        let mut state = test_state();
        let general = state
            .units
            .iter()
            .find(|u| u.owner == 0 && u.is_general)
            .unwrap()
            .pos;
        apply_directives(
            &mut state,
            0,
            &[Directive::AssignRole {
                hex_q: general.q,
                hex_r: general.r,
                role: Role::Farmer,
                count: 3,
            }],
        );
        let farmers: u16 = state
            .population
            .iter()
            .filter(|p| p.owner == 0 && p.hex == general && p.role == Role::Farmer)
            .map(|p| p.count)
            .sum();
        assert!(farmers >= 8);
    }

    #[test]
    fn load_convoy_spawns_convoy() {
        let mut state = test_state();
        let general = state
            .units
            .iter()
            .find(|u| u.owner == 0 && u.is_general)
            .unwrap()
            .pos;
        apply_directives(
            &mut state,
            0,
            &[Directive::LoadConvoy {
                hex_q: general.q,
                hex_r: general.r,
                cargo_type: CargoType::Food,
                amount: 10.0,
            }],
        );
        assert_eq!(state.convoys.len(), 1);
    }
}
