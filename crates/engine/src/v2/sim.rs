use super::combat;
use super::hex::{self, Axial};
use super::pathfinding;
use super::state::{CargoType, GameState, Population, Role};
use super::{
    BASE_MOVE_COOLDOWN, BASE_STORAGE_CAP, CONVOY_MOVE_COOLDOWN, DEPOT_STORAGE_CAP, FARMER_RATE,
    FOOD_RATE, FRONTIER_DECAY_RATE, MATERIAL_RATE, MIGRATION_DIVISOR, POPULATION_GROWTH_RATE, TIMEOUT_TICKS,
    SETTLEMENT_SUPPORT_RADIUS, SETTLEMENT_THRESHOLD, SOLDIER_READY_THRESHOLD, STARVATION_DAMAGE, TERRAIN_MOVE_PENALTY,
    TRAINING_RATE, UPKEEP_PER_UNIT, WORKER_RATE,
};
use std::collections::HashMap;

pub fn tick(state: &mut GameState) {
    generate_resources(state);
    grow_population(state);
    migrate_population(state);
    consume_upkeep(state);
    combat::resolve_combat(state);
    move_convoys(state);
    move_units(state);
    decay_frontier_stockpiles(state);
    decrement_cooldowns(state);
    cleanup(state);
    cleanup_stale_engagements(state);
    refresh_player_totals(state);
    state.tick += 1;
}

pub fn is_over(state: &GameState) -> bool {
    state.players.iter().filter(|p| p.alive).count() <= 1
}

pub fn timeout_limit(max_ticks: u64) -> u64 {
    max_ticks.min(TIMEOUT_TICKS)
}

pub fn reached_timeout(state: &GameState, max_ticks: u64) -> bool {
    state.tick >= timeout_limit(max_ticks)
}

pub fn winner(state: &GameState) -> Option<u8> {
    let alive: Vec<_> = state.players.iter().filter(|p| p.alive).collect();
    if alive.len() == 1 {
        Some(alive[0].id)
    } else {
        None
    }
}

pub fn winner_at_limit(state: &GameState, max_ticks: u64) -> Option<u8> {
    winner(state).or_else(|| {
        if reached_timeout(state, max_ticks) {
            winner_by_score(state)
        } else {
            None
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreBreakdown {
    pub player_id: u8,
    pub population: f32,
    pub territory: f32,
    pub military: f32,
    pub stockpiles: f32,
    pub total: f32,
}

pub fn score_players(state: &GameState) -> Vec<ScoreBreakdown> {
    let total_population = state
        .population
        .iter()
        .map(|p| p.count as f32)
        .sum::<f32>()
        .max(1.0);
    let total_territory = (state
        .grid
        .iter()
        .filter(|c| c.stockpile_owner.is_some())
        .count() as f32)
        .max(1.0);
    let total_military = state
        .units
        .iter()
        .map(|u| u.strength.max(0.0))
        .sum::<f32>()
        .max(1.0);
    let total_stockpiles = state
        .grid
        .iter()
        .map(|c| c.food_stockpile + c.material_stockpile)
        .sum::<f32>()
        .max(1.0);

    state
        .players
        .iter()
        .map(|player| {
            let population = state
                .population
                .iter()
                .filter(|p| p.owner == player.id)
                .map(|p| p.count as f32)
                .sum::<f32>();
            let territory = state
                .grid
                .iter()
                .filter(|c| c.stockpile_owner == Some(player.id))
                .count() as f32;
            let military = state
                .units
                .iter()
                .filter(|u| u.owner == player.id)
                .map(|u| u.strength.max(0.0))
                .sum::<f32>();
            let stockpiles = state
                .grid
                .iter()
                .filter(|c| c.stockpile_owner == Some(player.id))
                .map(|c| c.food_stockpile + c.material_stockpile)
                .sum::<f32>();
            let total = 0.4 * (population / total_population)
                + 0.3 * (territory / total_territory)
                + 0.2 * (military / total_military)
                + 0.1 * (stockpiles / total_stockpiles);
            ScoreBreakdown {
                player_id: player.id,
                population,
                territory,
                military,
                stockpiles,
                total,
            }
        })
        .collect()
}

pub fn winner_by_score(state: &GameState) -> Option<u8> {
    score_players(state)
        .into_iter()
        .max_by(|a, b| {
            a.total
                .partial_cmp(&b.total)
                .unwrap()
                .then_with(|| a.military.partial_cmp(&b.military).unwrap())
        })
        .map(|s| s.player_id)
}

fn storage_cap(has_depot: bool) -> f32 {
    if has_depot {
        DEPOT_STORAGE_CAP
    } else {
        BASE_STORAGE_CAP
    }
}

fn settlement_hexes(state: &GameState, owner: u8) -> Vec<Axial> {
    let mut hexes = Vec::new();
    for pop in state.population.iter().filter(|p| p.owner == owner) {
        if state.population_on_hex(owner, pop.hex) >= SETTLEMENT_THRESHOLD && !hexes.contains(&pop.hex) {
            hexes.push(pop.hex);
        }
    }
    hexes
}

fn supported_settlement(state: &GameState, owner: u8, ax: Axial) -> Option<Axial> {
    settlement_hexes(state, owner)
        .into_iter()
        .filter(|settlement| hex::distance(*settlement, ax) <= SETTLEMENT_SUPPORT_RADIUS)
        .min_by_key(|settlement| hex::distance(*settlement, ax))
}

fn has_settlement_support(state: &GameState, owner: u8, ax: Axial) -> bool {
    supported_settlement(state, owner, ax).is_some()
}

fn add_stockpile(state: &mut GameState, ax: Axial, owner: u8, food: f32, material: f32) {
    let target = supported_settlement(state, owner, ax).unwrap_or(ax);
    if let Some(cell) = state.cell_at_mut(target) {
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
        if total_pop < SETTLEMENT_THRESHOLD {
            continue;
        }
        let Some(cell) = state.cell_at(hex) else {
            continue;
        };
        if cell.stockpile_owner != Some(owner) || cell.food_stockpile < 2.0 {
            continue;
        }
        let carrying_capacity = 20.0 + cell.terrain_value * 20.0 + cell.water_access * 12.0;
        let headroom = (1.0 - total_pop as f32 / carrying_capacity).max(0.0);
        let raw_growth = farmers as f32 * POPULATION_GROWTH_RATE * headroom;
        if raw_growth > 0.0 {
            growth_targets.push((hex, owner, raw_growth.floor().max(1.0) as u16));
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

fn migrate_population(state: &mut GameState) {
    let mut seen: Vec<(Axial, u8)> = Vec::new();
    let mut migrations: Vec<(u32, Axial)> = Vec::new();

    for pop in &state.population {
        let key = (pop.hex, pop.owner);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);

        let total_pop = state.population_on_hex(pop.owner, pop.hex);
        if total_pop <= 15 || total_pop < SETTLEMENT_THRESHOLD {
            continue;
        }

        let roll =
            (state.tick + pop.hex.q.unsigned_abs() as u64 * 17 + pop.hex.r.unsigned_abs() as u64 * 31)
                % MIGRATION_DIVISOR;
        if roll != 0 {
            continue;
        }

        let origin_fertility = state.cell_at(pop.hex).map(|c| c.terrain_value).unwrap_or(0.0);
        let target = hex::neighbors(pop.hex)
            .into_iter()
            .filter(|n| state.in_bounds(*n))
            .filter(|n| {
                state
                    .cell_at(*n)
                    .map(|c| c.stockpile_owner == Some(pop.owner))
                    .unwrap_or(false)
            })
            .filter(|n| !state.is_settlement(pop.owner, *n))
            .max_by(|a, b| {
                let af = state.cell_at(*a).map(|c| c.terrain_value).unwrap_or(0.0);
                let bf = state.cell_at(*b).map(|c| c.terrain_value).unwrap_or(0.0);
                af.partial_cmp(&bf).unwrap()
            });
        let Some(target_hex) = target else { continue };
        let target_fertility = state
            .cell_at(target_hex)
            .map(|c| c.terrain_value)
            .unwrap_or(0.0);
        if target_fertility < origin_fertility {
            continue;
        }
        if let Some(source) = state.population.iter().find(|p| {
            p.owner == pop.owner && p.hex == pop.hex && p.role != Role::Soldier && p.count > 0
        }) {
            migrations.push((source.id, target_hex));
        }
    }

    for (source_id, target_hex) in migrations {
        if let Some(idx) = state
            .population
            .iter()
            .position(|p| p.id == source_id && p.count > 0)
        {
            let owner = state.population[idx].owner;
            state.population[idx].count -= 1;
            if let Some(target) = state
                .population
                .iter_mut()
                .find(|p| p.owner == owner && p.hex == target_hex && p.role == Role::Idle)
            {
                target.count += 1;
            } else {
                state.population.push(Population {
                    id: state.next_pop_id,
                    hex: target_hex,
                    owner,
                    count: 1,
                    role: Role::Idle,
                    training: 0.0,
                });
                state.next_pop_id += 1;
            }
        }
    }
    state.population.retain(|p| p.count > 0);
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
            if let Some((enemy, raid_hex)) = convoy_raider(state, next, owner) {
                if let Some(idx) = state.convoys.iter().position(|c| c.id == id) {
                    let convoy = state.convoys.remove(idx);
                    add_convoy_cargo_to_cell(
                        state,
                        raid_hex,
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
                    match convoy.cargo_type {
                        CargoType::Settlers => {
                            if owner_controls_hex(state, owner, dest)
                                && state
                                    .cell_at(dest)
                                    .map(|c| c.terrain_value > 0.0)
                                    .unwrap_or(false)
                                && !state.is_settlement(owner, dest)
                            {
                                if let Some(target) = state.population.iter_mut().find(|p| {
                                    p.owner == owner && p.hex == dest && p.role == Role::Idle
                                }) {
                                    target.count += convoy.cargo_amount.round() as u16;
                                } else {
                                    state.population.push(Population {
                                        id: state.next_pop_id,
                                        hex: dest,
                                        owner,
                                        count: convoy.cargo_amount.round() as u16,
                                        role: Role::Idle,
                                        training: 0.0,
                                    });
                                    state.next_pop_id += 1;
                                }
                                if let Some(cell) = state.cell_at_mut(dest) {
                                    cell.stockpile_owner = Some(owner);
                                }
                            }
                        }
                        CargoType::Food | CargoType::Material => {
                            add_convoy_cargo_to_cell(
                                state,
                                dest,
                                owner,
                                convoy.cargo_type,
                                convoy.cargo_amount,
                            );
                            if !convoy.returning && convoy.origin != dest {
                                state.convoys.push(super::state::Convoy {
                                    id: state.next_convoy_id,
                                    owner: convoy.owner,
                                    pos: dest,
                                    origin: convoy.origin,
                                    destination: convoy.origin,
                                    cargo_type: convoy.cargo_type,
                                    cargo_amount: 0.0,
                                    capacity: convoy.capacity,
                                    speed: convoy.speed,
                                    move_cooldown: CONVOY_MOVE_COOLDOWN,
                                    returning: true,
                                });
                                state.next_convoy_id += 1;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn convoy_raider(state: &GameState, pos: Axial, owner: u8) -> Option<(u8, Axial)> {
    state
        .units
        .iter()
        .filter(|u| u.owner != owner)
        .filter(|u| u.pos == pos || hex::distance(u.pos, pos) == 1)
        .min_by_key(|u| u.id)
        .map(|u| (u.owner, u.pos))
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
            CargoType::Settlers => {}
        }
    }
}

fn decay_frontier_stockpiles(state: &mut GameState) {
    let support_mask: Vec<bool> = state
        .grid
        .iter()
        .enumerate()
        .map(|(idx, cell)| {
            if let Some(owner) = cell.stockpile_owner {
                has_settlement_support(state, owner, offset_index_to_axial(state, idx))
            } else {
                false
            }
        })
        .collect();

    for (idx, supported) in support_mask.into_iter().enumerate() {
        if !supported {
            let cell = &mut state.grid[idx];
            if cell.stockpile_owner.is_some() {
                cell.food_stockpile *= 1.0 - FRONTIER_DECAY_RATE;
                cell.material_stockpile *= 1.0 - FRONTIER_DECAY_RATE;
            }
        }
    }
}

fn offset_index_to_axial(state: &GameState, idx: usize) -> Axial {
    let row = idx / state.width;
    let col = idx % state.width;
    hex::offset_to_axial(row as i32, col as i32)
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

fn cleanup_stale_engagements(state: &mut GameState) {
    let unit_positions: HashMap<u32, Axial> = state.units.iter().map(|u| (u.id, u.pos)).collect();

    for unit in &mut state.units {
        unit.engagements.retain(|eng| match unit_positions.get(&eng.enemy_id) {
            None => {
                tracing::warn!(
                    tick = state.tick,
                    unit_id = unit.id,
                    enemy_id = eng.enemy_id,
                    edge = eng.edge,
                    "removing stale engagement: enemy does not exist"
                );
                false
            }
            Some(&enemy_pos) => {
                if hex::shared_edge(unit.pos, enemy_pos).is_none() {
                    tracing::warn!(
                        tick = state.tick,
                        unit_id = unit.id,
                        enemy_id = eng.enemy_id,
                        unit_pos = ?unit.pos,
                        enemy_pos = ?enemy_pos,
                        "removing stale engagement: units not adjacent"
                    );
                    false
                } else {
                    true
                }
            }
        });
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

    #[test]
    fn unsupported_frontier_stockpiles_decay() {
        let mut state = test_state();
        let general_pos = state
            .units
            .iter()
            .find(|u| u.owner == 0 && u.is_general)
            .unwrap()
            .pos;
        let frontier = neighbors(general_pos)
            .into_iter()
            .flat_map(neighbors)
            .find(|ax| state.in_bounds(*ax) && distance(*ax, general_pos) >= 2)
            .unwrap();
        let cell = state.cell_at_mut(frontier).unwrap();
        cell.stockpile_owner = Some(0);
        cell.food_stockpile = 10.0;
        cell.material_stockpile = 8.0;

        tick(&mut state);

        let cell = state.cell_at(frontier).unwrap();
        assert!(cell.food_stockpile < 10.0);
        assert!(cell.material_stockpile < 8.0);
    }

    #[test]
    fn settler_convoy_can_found_remote_settlement() {
        let mut state = test_state();
        let general_pos = state
            .units
            .iter()
            .find(|u| u.owner == 0 && u.is_general)
            .unwrap()
            .pos;
        let target = neighbors(general_pos)
            .into_iter()
            .flat_map(neighbors)
            .find(|ax| state.in_bounds(*ax) && distance(*ax, general_pos) >= 2)
            .unwrap();
        state.cell_at_mut(target).unwrap().stockpile_owner = Some(0);

        apply_directives(
            &mut state,
            0,
            &[Directive::LoadConvoy {
                hex_q: general_pos.q,
                hex_r: general_pos.r,
                cargo_type: CargoType::Settlers,
                amount: 10.0,
            }],
        );
        let convoy_id = state.convoys[0].id;
        apply_directives(
            &mut state,
            0,
            &[Directive::SendConvoy {
                convoy_id,
                dest_q: target.q,
                dest_r: target.r,
            }],
        );

        for _ in 0..40 {
            tick(&mut state);
            if state.is_settlement(0, target) {
                break;
            }
        }

        assert!(state.is_settlement(0, target));
    }

    #[test]
    fn timeout_winner_uses_score() {
        let mut state = test_state();
        state.tick = TIMEOUT_TICKS;
        for pop in state.population.iter_mut().filter(|p| p.owner == 0) {
            pop.count = pop.count.saturating_add(20);
        }

        assert_eq!(winner_at_limit(&state, TIMEOUT_TICKS), Some(0));
        assert!(reached_timeout(&state, TIMEOUT_TICKS));
    }

    #[test]
    fn stale_engagements_are_removed() {
        let mut state = test_state();
        let a_idx = state
            .units
            .iter()
            .position(|u| u.owner == 0 && !u.is_general)
            .unwrap();
        let b_idx = state
            .units
            .iter()
            .position(|u| u.owner == 1 && !u.is_general)
            .unwrap();
        let a_id = state.units[a_idx].id;
        let b_id = state.units[b_idx].id;
        state.units[a_idx]
            .engagements
            .push(crate::v2::state::Engagement {
                enemy_id: b_id,
                edge: 0,
            });
        state.units[b_idx]
            .engagements
            .push(crate::v2::state::Engagement {
                enemy_id: a_id,
                edge: 3,
            });
        state.units[b_idx].pos = offset_to_axial(19, 19);

        tick(&mut state);

        let a = state.units.iter().find(|u| u.id == a_id).unwrap();
        let b = state.units.iter().find(|u| u.id == b_id).unwrap();
        assert!(a.engagements.is_empty());
        assert!(b.engagements.is_empty());
    }

    #[test]
    fn convoy_is_raided_from_adjacent_hex() {
        let mut state = test_state();
        state.units.clear();
        state.population.clear();
        state.convoys.clear();
        for cell in &mut state.grid {
            cell.stockpile_owner = None;
            cell.food_stockpile = 0.0;
            cell.material_stockpile = 0.0;
        }

        let convoy_pos = offset_to_axial(5, 5);
        let destination = neighbors(convoy_pos)[0];
        let raid_hex = neighbors(destination)[1];

        state.players[0].general_id = 100;
        state.players[1].general_id = 200;
        state.units.push(crate::v2::state::Unit {
            id: 100,
            owner: 0,
            pos: offset_to_axial(1, 1),
            strength: 100.0,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: true,
        });
        state.units.push(crate::v2::state::Unit {
            id: 200,
            owner: 1,
            pos: offset_to_axial(10, 10),
            strength: 100.0,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: true,
        });
        state.units.push(crate::v2::state::Unit {
            id: 201,
            owner: 1,
            pos: raid_hex,
            strength: 100.0,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: false,
        });
        state.population.push(Population {
            id: 0,
            hex: raid_hex,
            owner: 1,
            count: 10,
            role: Role::Idle,
            training: 0.0,
        });
        state.next_pop_id = 1;
        state.cell_at_mut(raid_hex).unwrap().stockpile_owner = Some(1);
        state.cell_at_mut(raid_hex).unwrap().terrain_value = 0.0;
        state.cell_at_mut(raid_hex).unwrap().material_value = 0.0;
        state.convoys.push(crate::v2::state::Convoy {
            id: 0,
            owner: 0,
            pos: convoy_pos,
            origin: convoy_pos,
            destination,
            cargo_type: CargoType::Food,
            cargo_amount: 9.0,
            capacity: 20.0,
            speed: 1.0,
            move_cooldown: 0,
            returning: false,
        });

        tick(&mut state);

        assert!(state.convoys.is_empty());
        let raid_cell = state.cell_at(raid_hex).unwrap();
        assert_eq!(raid_cell.stockpile_owner, Some(1));
        assert_eq!(raid_cell.food_stockpile, 9.0);
    }
}
