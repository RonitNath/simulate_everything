use super::combat;
use super::hex::{self, Axial};
use super::pathfinding;
use super::state::{CargoType, GameState, Population, Role};
use super::{
    BASE_MOVE_COOLDOWN, BASE_STORAGE_CAP, CONVOY_MOVE_COOLDOWN, DEPOT_STORAGE_CAP, FARMER_RATE,
    FOOD_RATE, MATERIAL_RATE, POPULATION_GROWTH_RATE, SOLDIER_READY_THRESHOLD, STARVATION_DAMAGE,
    TERRAIN_MOVE_PENALTY, TRAINING_RATE, UPKEEP_PER_UNIT, WORKER_RATE,
};
use std::collections::HashMap;

pub fn tick(state: &mut GameState) {
    generate_resources(state);
    grow_population(state);
    consume_upkeep(state);
    combat::resolve_combat(state);
    move_convoys(state);
    move_units(state);
    decrement_cooldowns(state);
    cleanup(state);
    check_stale_engagements(state);
    refresh_player_totals(state);
    state.tick += 1;
}

pub fn is_over(state: &GameState) -> bool {
    state.players.iter().filter(|p| p.alive).count() <= 1
}

pub fn winner(state: &GameState) -> Option<u8> {
    let alive: Vec<_> = state.players.iter().filter(|p| p.alive).collect();
    if alive.len() == 1 {
        Some(alive[0].id)
    } else {
        None
    }
}

fn storage_cap(has_depot: bool) -> f32 {
    if has_depot {
        DEPOT_STORAGE_CAP
    } else {
        BASE_STORAGE_CAP
    }
}

fn add_stockpile(state: &mut GameState, ax: Axial, owner: u8, food: f32, material: f32) {
    if let Some(cell) = state.cell_at_mut(ax) {
        if cell.stockpile_owner.is_none() || cell.stockpile_owner == Some(owner) {
            cell.stockpile_owner = Some(owner);
        }
        let cap = storage_cap(cell.has_depot);
        if cell.stockpile_owner == Some(owner) {
            cell.food_stockpile = (cell.food_stockpile + food).clamp(0.0, cap);
            cell.material_stockpile = (cell.material_stockpile + material).clamp(0.0, cap);
        }
    }
}

fn capture_hex(state: &mut GameState, ax: Axial, owner: u8) {
    if let Some(cell) = state.cell_at_mut(ax) {
        cell.stockpile_owner = Some(owner);
    }
}

fn generate_resources(state: &mut GameState) {
    let unit_income: Vec<(Axial, u8, f32, f32)> = state
        .units
        .iter()
        .filter(|u| u.destination.is_none() && u.engagements.is_empty())
        .filter_map(|u| {
            state.cell_at(u.pos).map(|cell| {
                (
                    u.pos,
                    u.owner,
                    cell.terrain_value * FOOD_RATE,
                    cell.material_value * MATERIAL_RATE,
                )
            })
        })
        .collect();

    for (hex, owner, food, material) in unit_income {
        add_stockpile(state, hex, owner, food, material);
    }

    // Collect alive player IDs to avoid borrowing state.players while iterating population
    let alive_ids: Vec<u8> = state
        .players
        .iter()
        .filter(|p| p.alive)
        .map(|p| p.id)
        .collect();

    // Collect pop income and training updates separately to avoid borrow conflicts
    let mut pop_income: Vec<(Axial, u8, f32, f32)> = Vec::new();
    let mut training_updates: Vec<(usize, f32)> = Vec::new();

    for (i, pop) in state.population.iter().enumerate() {
        if !alive_ids.contains(&pop.owner) {
            continue;
        }
        if let Some(cell) = state.cell_at(pop.hex) {
            match pop.role {
                Role::Farmer => pop_income.push((
                    pop.hex,
                    pop.owner,
                    pop.count as f32 * cell.terrain_value * FARMER_RATE,
                    0.0,
                )),
                Role::Worker => pop_income.push((
                    pop.hex,
                    pop.owner,
                    0.0,
                    pop.count as f32 * cell.material_value * WORKER_RATE,
                )),
                Role::Soldier => {
                    let new_training = (pop.training + TRAINING_RATE).min(SOLDIER_READY_THRESHOLD);
                    training_updates.push((i, new_training));
                }
                Role::Idle => {}
            }
        }
    }

    for (i, training) in training_updates {
        state.population[i].training = training;
    }

    for (hex, owner, food, material) in pop_income {
        add_stockpile(state, hex, owner, food, material);
    }
}

fn grow_population(state: &mut GameState) {
    if state.tick % 10 != 0 {
        return;
    }

    let mut growth_targets: Vec<(Axial, u8, u16)> = Vec::new();
    let mut by_hex: HashMap<(i32, i32, u8), Vec<&Population>> = HashMap::new();
    for pop in &state.population {
        by_hex
            .entry((pop.hex.q, pop.hex.r, pop.owner))
            .or_default()
            .push(pop);
    }

    for ((q, r, owner), cohorts) in by_hex {
        let hex = Axial::new(q, r);
        let farmers: u16 = cohorts
            .iter()
            .filter(|p| p.role == Role::Farmer)
            .map(|p| p.count)
            .sum();
        if farmers == 0 {
            continue;
        }
        let total_pop: u16 = cohorts.iter().map(|p| p.count).sum();
        let Some(cell) = state.cell_at(hex) else {
            continue;
        };
        if cell.stockpile_owner != Some(owner) || cell.food_stockpile < 2.0 {
            continue;
        }
        let carrying_capacity = 10.0 + cell.terrain_value * 12.0 + cell.water_access * 8.0;
        let headroom = (1.0 - total_pop as f32 / carrying_capacity).max(0.0);
        let growth = (farmers as f32 * POPULATION_GROWTH_RATE * headroom).floor() as u16;
        if growth > 0 {
            growth_targets.push((hex, owner, growth.max(1)));
        }
    }

    for (hex, owner, growth) in growth_targets {
        if let Some(idle) = state
            .population
            .iter_mut()
            .find(|p| p.owner == owner && p.hex == hex && p.role == Role::Idle)
        {
            idle.count = idle.count.saturating_add(growth);
        } else {
            state.population.push(Population {
                id: state.next_pop_id,
                hex,
                owner,
                count: growth,
                role: Role::Idle,
                training: 0.0,
            });
            state.next_pop_id += 1;
        }
    }
}

fn consume_upkeep(state: &mut GameState) {
    // Collect unit positions/owners first to avoid simultaneous borrow of units and grid
    let unit_info: Vec<(usize, Axial, u8)> = state
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| (i, u.pos, u.owner))
        .collect();

    let mut starved_units: Vec<usize> = Vec::new();

    for (i, pos, owner) in &unit_info {
        // Capture hex ownership
        if let Some(cell) = state.cell_at_mut(*pos) {
            if cell.stockpile_owner.is_none() || cell.stockpile_owner == Some(*owner) {
                cell.stockpile_owner = Some(*owner);
            }
        }

        let mut fed = false;
        if let Some(cell) = state.cell_at_mut(*pos) {
            if cell.stockpile_owner == Some(*owner) && cell.food_stockpile >= UPKEEP_PER_UNIT {
                cell.food_stockpile -= UPKEEP_PER_UNIT;
                fed = true;
            } else if cell.stockpile_owner == Some(*owner) && cell.food_stockpile > 0.0 {
                cell.food_stockpile = 0.0;
            }
        }
        if !fed {
            starved_units.push(*i);
        }
    }

    for i in starved_units {
        state.units[i].strength -= STARVATION_DAMAGE;
    }

    let convoy_info: Vec<(Axial, u8)> = state.convoys.iter().map(|c| (c.pos, c.owner)).collect();

    for (pos, owner) in convoy_info {
        if let Some(cell) = state.cell_at_mut(pos) {
            if cell.stockpile_owner == Some(owner) && cell.food_stockpile >= UPKEEP_PER_UNIT * 0.5 {
                cell.food_stockpile -= UPKEEP_PER_UNIT * 0.5;
            }
        }
    }
}

fn road_bonus(level: u8) -> f32 {
    match level {
        1 => 0.3,
        2 => 0.6,
        3 => 1.0,
        _ => 0.0,
    }
}

fn movement_cooldown(state: &GameState, from: Axial, to: Axial, convoy: bool) -> u8 {
    let Some(cell) = state.cell_at(to) else {
        return BASE_MOVE_COOLDOWN;
    };
    let from_height = state.cell_at(from).map(|c| c.height).unwrap_or(0.0);
    let slope = (cell.height - from_height).max(0.0);
    let base = if convoy {
        CONVOY_MOVE_COOLDOWN as f32
    } else {
        BASE_MOVE_COOLDOWN as f32
    };
    let roughness = cell.terrain_value * TERRAIN_MOVE_PENALTY + slope * 2.0;
    let adjusted = (base + roughness) * (1.0 - road_bonus(cell.road_level) * 0.5);
    adjusted.max(1.0).round() as u8
}

fn move_units(state: &mut GameState) {
    let moves: Vec<(usize, Axial, u8, bool)> = state
        .units
        .iter()
        .enumerate()
        .filter_map(|(i, u)| {
            let dest = u.destination?;
            if u.move_cooldown > 0 || !u.engagements.is_empty() {
                return None;
            }

            match pathfinding::next_step(state, u.pos, dest) {
                Some(next_pos) => {
                    let cooldown = movement_cooldown(state, u.pos, next_pos, false);
                    Some((i, next_pos, cooldown, next_pos == dest))
                }
                None => Some((i, u.pos, 0, true)),
            }
        })
        .collect();

    for (i, new_pos, cooldown, clear_dest) in moves {
        let owner = state.units[i].owner;
        state.units[i].pos = new_pos;
        state.units[i].move_cooldown = cooldown;
        capture_hex(state, new_pos, owner);
        if clear_dest {
            state.units[i].destination = None;
        }
    }
}

fn move_convoys(state: &mut GameState) {
    let convoy_states: Vec<(u32, Axial, Axial, u8)> = state
        .convoys
        .iter()
        .filter(|c| c.move_cooldown == 0 && c.pos != c.destination)
        .map(|c| (c.id, c.pos, c.destination, c.owner))
        .collect();

    for (id, pos, dest, owner) in convoy_states {
        if let Some(next) = pathfinding::next_step(state, pos, dest) {
            let cooldown = movement_cooldown(state, pos, next, true);
            if let Some(convoy) = state.convoys.iter_mut().find(|c| c.id == id) {
                convoy.pos = next;
                convoy.move_cooldown = cooldown;
            }
            if let Some(enemy) = state
                .units
                .iter()
                .find(|u| u.owner != owner && u.pos == next)
                .map(|u| u.owner)
            {
                if let Some(idx) = state.convoys.iter().position(|c| c.id == id) {
                    let convoy = state.convoys.remove(idx);
                    add_convoy_cargo_to_cell(
                        state,
                        next,
                        enemy,
                        convoy.cargo_type,
                        convoy.cargo_amount,
                    );
                }
                continue;
            }
            if next == dest {
                if let Some(idx) = state.convoys.iter().position(|c| c.id == id) {
                    let convoy = state.convoys.remove(idx);
                    add_convoy_cargo_to_cell(
                        state,
                        dest,
                        owner,
                        convoy.cargo_type,
                        convoy.cargo_amount,
                    );
                }
            }
        }
    }
}

fn add_convoy_cargo_to_cell(
    state: &mut GameState,
    ax: Axial,
    owner: u8,
    cargo_type: CargoType,
    amount: f32,
) {
    if let Some(cell) = state.cell_at_mut(ax) {
        cell.stockpile_owner = Some(owner);
        let cap = storage_cap(cell.has_depot);
        match cargo_type {
            CargoType::Food => cell.food_stockpile = (cell.food_stockpile + amount).clamp(0.0, cap),
            CargoType::Material => {
                cell.material_stockpile = (cell.material_stockpile + amount).clamp(0.0, cap)
            }
        }
    }
}

fn decrement_cooldowns(state: &mut GameState) {
    for unit in &mut state.units {
        if unit.move_cooldown > 0 {
            unit.move_cooldown -= 1;
        }
    }
    for convoy in &mut state.convoys {
        if convoy.move_cooldown > 0 {
            convoy.move_cooldown -= 1;
        }
    }
}

fn cleanup(state: &mut GameState) {
    combat::cleanup_engagements(state);
    state.units.retain(|u| u.strength > 0.0);

    let eliminated: Vec<u8> = state
        .players
        .iter()
        .filter(|p| p.alive)
        .filter(|p| !state.units.iter().any(|u| u.id == p.general_id))
        .map(|p| p.id)
        .collect();

    for pid in eliminated {
        if let Some(player) = state.players.iter_mut().find(|p| p.id == pid) {
            player.alive = false;
        }
        let removed_ids: Vec<u32> = state
            .units
            .iter()
            .filter(|u| u.owner == pid)
            .map(|u| u.id)
            .collect();
        state.units.retain(|u| u.owner != pid);
        state.population.retain(|p| p.owner != pid);
        state.convoys.retain(|c| c.owner != pid);
        for cell in &mut state.grid {
            if cell.stockpile_owner == Some(pid) {
                cell.stockpile_owner = None;
                cell.food_stockpile = 0.0;
                cell.material_stockpile = 0.0;
            }
        }
        for unit in &mut state.units {
            unit.engagements
                .retain(|e| !removed_ids.contains(&e.enemy_id));
        }
    }
}

fn refresh_player_totals(state: &mut GameState) {
    for player in &mut state.players {
        player.food = 0.0;
        player.material = 0.0;
    }
    for cell in &state.grid {
        if let Some(owner) = cell.stockpile_owner {
            if let Some(player) = state.players.iter_mut().find(|p| p.id == owner) {
                player.food += cell.food_stockpile;
                player.material += cell.material_stockpile;
            }
        }
    }
}

fn check_stale_engagements(state: &GameState) {
    let unit_positions: HashMap<u32, Axial> = state.units.iter().map(|u| (u.id, u.pos)).collect();

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
    use crate::v2::directive::{Directive, apply_directives};
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
    fn stationary_units_generate_stockpiles() {
        let mut state = test_state();
        let before = state
            .grid
            .iter()
            .map(|c| (c.food_stockpile, c.material_stockpile))
            .collect::<Vec<_>>();
        tick(&mut state);
        assert!(state.grid.iter().zip(before.iter()).any(|(cell, before)| {
            cell.food_stockpile > before.0 || cell.material_stockpile > before.1
        }));
    }

    #[test]
    fn starvation_damages_units_when_no_local_food() {
        let mut state = test_state();
        for cell in &mut state.grid {
            cell.food_stockpile = 0.0;
        }
        for pop in &mut state.population {
            pop.count = 0;
        }
        for unit in &mut state.units {
            unit.destination = Some(offset_to_axial(0, 0));
        }
        let idx = state.units.iter().position(|u| !u.is_general).unwrap();
        let before = state.units[idx].strength;
        tick(&mut state);
        assert!(state.units[idx].strength < before);
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
        assert!(new_dist < initial_dist);
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
        assert!(state.units[unit_idx].destination.is_none());
    }

    #[test]
    fn trained_population_produces_units() {
        let mut state = test_state();
        let general_pos = state
            .units
            .iter()
            .find(|u| u.owner == 0 && u.is_general)
            .unwrap()
            .pos;
        apply_directives(
            &mut state,
            0,
            &[
                Directive::TrainSoldier {
                    hex_q: general_pos.q,
                    hex_r: general_pos.r,
                },
                Directive::TrainSoldier {
                    hex_q: general_pos.q,
                    hex_r: general_pos.r,
                },
            ],
        );
        for _ in 0..30 {
            tick(&mut state);
        }
        let before = state.units.iter().filter(|u| u.owner == 0).count();
        apply_directives(&mut state, 0, &[Directive::Produce]);
        let after = state.units.iter().filter(|u| u.owner == 0).count();
        assert!(after > before);
    }
}
