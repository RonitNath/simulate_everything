use super::city_ai;
use super::combat;
use super::hex::{self, Axial};
use super::pathfinding;
#[cfg(debug_assertions)]
use super::state::TickAccumulator;
use super::state::{
    CargoType, Convoy, ConvoyKey, GameState, PopKey, Population, Role, Settlement, SettlementType,
};
use super::{
    BASE_MOVE_COOLDOWN, BASE_STORAGE_CAP, CITY_AI_INTERVAL, CITY_RADIUS, CITY_THRESHOLD,
    CONVOY_MOVE_COOLDOWN, DEPOT_STORAGE_CAP, FARM_RADIUS, FARM_THRESHOLD, FARMER_RATE, FOOD_RATE,
    FRONTIER_DECAY_RATE, MATERIAL_RATE, MIGRATION_DIVISOR, POPULATION_GROWTH_RATE,
    SETTLEMENT_THRESHOLD, SOLDIER_READY_THRESHOLD, STARVATION_DAMAGE, TERRAIN_MOVE_PENALTY,
    TIMEOUT_TICKS, TRAINING_RATE, UPKEEP_PER_UNIT, VILLAGE_RADIUS, VILLAGE_THRESHOLD, WORKER_RATE,
};
use serde::{Deserialize, Serialize};
use slotmap::Key;
use std::collections::HashMap;

pub fn tick(state: &mut GameState) {
    #[cfg(debug_assertions)]
    if state.tick_accumulator.is_none() {
        state.tick_accumulator = Some(TickAccumulator::default());
    }
    #[cfg(debug_assertions)]
    let pre_totals = economic_snapshot(state);
    state.rebuild_spatial();
    compute_territory(state);
    update_settlement_types(state);
    if state.tick % CITY_AI_INTERVAL == 0 {
        city_ai::run_city_ai(state);
    }
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
    #[cfg(debug_assertions)]
    debug_assert_economy_sane(state, "post", pre_totals);
    #[cfg(debug_assertions)]
    {
        state.tick_accumulator = None;
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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
        .values()
        .map(|p| p.count as f32)
        .sum::<f32>()
        .max(1.0);
    let total_territory = state.territory_cache.iter().filter(|c| c.is_some()).count() as f32;
    let total_territory = total_territory.max(1.0);
    let total_military = state
        .units
        .values()
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
                .values()
                .filter(|p| p.owner == player.id)
                .map(|p| p.count as f32)
                .sum::<f32>();
            let territory = state
                .territory_cache
                .iter()
                .filter(|c| **c == Some(player.id))
                .count() as f32;
            let military = state
                .units
                .values()
                .filter(|u| u.owner == player.id)
                .map(|u| u.strength.max(0.0))
                .sum::<f32>();
            // Stockpiles at cells claimed by this player (via territory_cache)
            let stockpiles = state
                .territory_cache
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == Some(player.id))
                .map(|(idx, _)| state.grid[idx].food_stockpile + state.grid[idx].material_stockpile)
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

/// Collect the hex positions of all settlements belonging to a player.
fn settlement_hexes(state: &GameState, owner: u8) -> Vec<Axial> {
    state
        .settlements
        .values()
        .filter(|s| s.owner == owner)
        .map(|s| s.hex)
        .collect()
}

/// Territory radius for a given settlement type.
fn settlement_radius(settlement_type: super::state::SettlementType) -> i32 {
    match settlement_type {
        super::state::SettlementType::Farm => FARM_RADIUS,
        super::state::SettlementType::Village => VILLAGE_RADIUS,
        super::state::SettlementType::City => CITY_RADIUS,
    }
}

/// Recompute the territory_cache and sync it back to cell.stockpile_owner.
/// Called at the start of each tick, and after mapgen.
pub fn compute_territory(state: &mut GameState) {
    let cells = state.width * state.height;
    let mut cache: Vec<Option<u8>> = vec![None; cells];

    // Settlement-based territory claims (larger radius wins if contested, else first owner wins).
    let settlement_info: Vec<(Axial, u8, i32)> = state
        .settlements
        .values()
        .map(|s| (s.hex, s.owner, settlement_radius(s.settlement_type)))
        .collect();

    for (hex, owner, radius) in &settlement_info {
        for claimed in hex::within_radius(*hex, *radius) {
            let (row, col) = hex::axial_to_offset(claimed);
            if row < 0 || col < 0 {
                continue;
            }
            let (row, col) = (row as usize, col as usize);
            if row >= state.height || col >= state.width {
                continue;
            }
            let idx = row * state.width + col;
            // First writer wins (settlement order is deterministic via slotmap iteration)
            if cache[idx].is_none() {
                cache[idx] = Some(*owner);
            }
        }
    }

    // Unit presence claims their own hex (overrides neutral, not enemy).
    let unit_claims: Vec<(usize, u8)> = state
        .units
        .values()
        .filter_map(|u| {
            let (row, col) = hex::axial_to_offset(u.pos);
            if row < 0 || col < 0 {
                return None;
            }
            let (row, col) = (row as usize, col as usize);
            if row >= state.height || col >= state.width {
                return None;
            }
            Some((row * state.width + col, u.owner))
        })
        .collect();

    for (idx, owner) in unit_claims {
        if cache[idx].is_none() {
            cache[idx] = Some(owner);
        }
    }

    // Sync cache back into cell.stockpile_owner so observation/replay still work.
    for (idx, claimed) in cache.iter().enumerate() {
        let prev = state.grid[idx].stockpile_owner;
        if prev != *claimed {
            state.grid[idx].stockpile_owner = *claimed;
            state.mark_dirty_index(idx);
        }
    }

    state.territory_cache = cache;
}

/// Promote/demote Settlement types based on current population, remove extinct settlements.
pub fn update_settlement_types(state: &mut GameState) {
    if state.tick % 10 != 0 {
        return;
    }

    let updates: Vec<(super::state::SettlementKey, u16)> = state
        .settlements
        .iter()
        .map(|(key, s)| (key, state.population_on_hex(s.owner, s.hex)))
        .collect();

    let mut remove_keys = Vec::new();
    for (key, total_pop) in updates {
        let new_type = if total_pop == 0 {
            None
        } else if total_pop >= CITY_THRESHOLD {
            Some(SettlementType::City)
        } else if total_pop >= VILLAGE_THRESHOLD {
            Some(SettlementType::Village)
        } else if total_pop >= FARM_THRESHOLD {
            Some(SettlementType::Farm)
        } else {
            None
        };

        match new_type {
            None => remove_keys.push(key),
            Some(t) => {
                if let Some(s) = state.settlements.get_mut(key) {
                    s.settlement_type = t;
                }
            }
        }
    }

    for key in remove_keys {
        state.settlements.remove(key);
    }
}

fn supported_settlement(state: &GameState, owner: u8, ax: Axial) -> Option<Axial> {
    settlement_hexes(state, owner)
        .into_iter()
        .filter(|settlement| {
            // Find the settlement entity at this hex to get its radius
            let radius = state
                .settlements
                .values()
                .find(|s| s.owner == owner && s.hex == *settlement)
                .map(|s| settlement_radius(s.settlement_type))
                .unwrap_or(VILLAGE_RADIUS);
            hex::distance(*settlement, ax) <= radius
        })
        .min_by_key(|settlement| hex::distance(*settlement, ax))
}

fn has_settlement_support(state: &GameState, owner: u8, ax: Axial) -> bool {
    supported_settlement(state, owner, ax).is_some()
}

fn add_stockpile(state: &mut GameState, ax: Axial, owner: u8, food: f32, material: f32) {
    let target = supported_settlement(state, owner, ax).unwrap_or(ax);
    let mut food_overflow = food;
    let mut material_overflow = material;
    let mut changed = false;
    // Check territory_cache: only deposit if we claim the target hex (or it's unclaimed).
    let target_idx = {
        let (row, col) = hex::axial_to_offset(target);
        if row >= 0 && col >= 0 && (row as usize) < state.height && (col as usize) < state.width {
            Some(row as usize * state.width + col as usize)
        } else {
            None
        }
    };
    let can_deposit = target_idx
        .map(|idx| {
            state.territory_cache[idx].is_none() || state.territory_cache[idx] == Some(owner)
        })
        .unwrap_or(false);

    if can_deposit {
        if let Some(cell) = state.cell_at_mut(target) {
            let cap = storage_cap(cell.has_depot);
            let next_food = (cell.food_stockpile + food).clamp(0.0, cap);
            let next_material = (cell.material_stockpile + material).clamp(0.0, cap);
            food_overflow = (cell.food_stockpile + food - next_food).max(0.0);
            material_overflow = (cell.material_stockpile + material - next_material).max(0.0);
            changed |= (next_food - cell.food_stockpile).abs() > 0.0001;
            changed |= (next_material - cell.material_stockpile).abs() > 0.0001;
            cell.food_stockpile = next_food;
            cell.material_stockpile = next_material;
        }
    }
    // else: target hex is enemy-owned; overflow defaults cover the full amount
    #[cfg(debug_assertions)]
    {
        state.record_food_destroyed(food_overflow);
        state.record_material_destroyed(material_overflow);
    }
    if changed {
        state.mark_dirty_axial(target);
    }
}

fn generate_resources(state: &mut GameState) {
    let unit_income: Vec<(Axial, u8, f32, f32)> = state
        .units
        .values()
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
        #[cfg(debug_assertions)]
        {
            state.record_food_produced(food);
            state.record_material_produced(material);
        }
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
    let mut training_updates: Vec<(PopKey, f32)> = Vec::new();

    for (key, pop) in state.population.iter() {
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
                    training_updates.push((key, new_training));
                }
                Role::Idle => {}
            }
        }
    }

    for (key, training) in training_updates {
        state.population[key].training = training;
    }

    for (hex, owner, food, material) in pop_income {
        #[cfg(debug_assertions)]
        {
            state.record_food_produced(food);
            state.record_material_produced(material);
        }
        add_stockpile(state, hex, owner, food, material);
    }
}

fn grow_population(state: &mut GameState) {
    if state.tick % 10 != 0 {
        return;
    }

    let mut growth_targets: Vec<(Axial, u8, u16)> = Vec::new();
    let mut by_hex: HashMap<(i32, i32, u8), Vec<&Population>> = HashMap::new();
    for (_, pop) in state.population.iter() {
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
        // Only grow if the hex is claimed by this owner and has food.
        let (row, col) = hex::axial_to_offset(hex);
        let in_territory = if row >= 0
            && col >= 0
            && (row as usize) < state.height
            && (col as usize) < state.width
        {
            state.territory_cache[row as usize * state.width + col as usize] == Some(owner)
        } else {
            false
        };
        if !in_territory || cell.food_stockpile < 2.0 {
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
            .find(|(_, p)| p.owner == owner && p.hex == hex && p.role == Role::Idle)
        {
            idle.1.count = idle.1.count.saturating_add(growth);
        } else {
            state.population.insert(Population {
                public_id: state.next_pop_id,
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
    let mut migrations: Vec<(PopKey, Axial)> = Vec::new();

    for (_, pop) in state.population.iter() {
        let key = (pop.hex, pop.owner);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);

        let total_pop = state.population_on_hex(pop.owner, pop.hex);
        if total_pop <= 15 || total_pop < SETTLEMENT_THRESHOLD {
            continue;
        }

        let roll = (state.tick
            + pop.hex.q.unsigned_abs() as u64 * 17
            + pop.hex.r.unsigned_abs() as u64 * 31)
            % MIGRATION_DIVISOR;
        if roll != 0 {
            continue;
        }

        let origin_fertility = state
            .cell_at(pop.hex)
            .map(|c| c.terrain_value)
            .unwrap_or(0.0);
        let owner = pop.owner;
        let target = hex::neighbors(pop.hex)
            .into_iter()
            .filter(|n| state.in_bounds(*n))
            .filter(|n| {
                let (row, col) = hex::axial_to_offset(*n);
                if row < 0 || col < 0 {
                    return false;
                }
                let idx = row as usize * state.width + col as usize;
                state.territory_cache.get(idx).copied().flatten() == Some(owner)
            })
            .filter(|n| !state.is_settlement(owner, *n))
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
            p.1.owner == pop.owner
                && p.1.hex == pop.hex
                && p.1.role != Role::Soldier
                && p.1.count > 0
        }) {
            migrations.push((source.0, target_hex));
        }
    }

    for (source_id, target_hex) in migrations {
        if state.population.get(source_id).is_some_and(|p| p.count > 0) {
            let owner = state.population[source_id].owner;
            state.population[source_id].count -= 1;
            if let Some(target) = state
                .population
                .iter_mut()
                .find(|(_, p)| p.owner == owner && p.hex == target_hex && p.role == Role::Idle)
            {
                target.1.count += 1;
            } else {
                state.population.insert(Population {
                    public_id: state.next_pop_id,
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
    state.population.retain(|_, p| p.count > 0);
}

pub(super) fn territory_owner(state: &GameState, ax: Axial) -> Option<u8> {
    let (row, col) = hex::axial_to_offset(ax);
    if row < 0 || col < 0 {
        return None;
    }
    let (row, col) = (row as usize, col as usize);
    if row >= state.height || col >= state.width {
        return None;
    }
    state.territory_cache[row * state.width + col]
}

fn consume_upkeep(state: &mut GameState) {
    let unit_info: Vec<(super::state::UnitKey, Axial, u8)> = state
        .units
        .iter()
        .map(|(key, u)| (key, u.pos, u.owner))
        .collect();

    let mut starved_units = Vec::new();

    for (i, pos, owner) in &unit_info {
        let hex_owner = territory_owner(state, *pos);
        let mut fed = false;
        let mut partial_food = 0.0;
        if let Some(cell) = state.cell_at_mut(*pos) {
            // Units can draw from their own territory or unclaimed hexes.
            if hex_owner.is_none() || hex_owner == Some(*owner) {
                if cell.food_stockpile >= UPKEEP_PER_UNIT {
                    cell.food_stockpile -= UPKEEP_PER_UNIT;
                    state.mark_dirty_axial(*pos);
                    fed = true;
                } else if cell.food_stockpile > 0.0 {
                    partial_food = cell.food_stockpile;
                    cell.food_stockpile = 0.0;
                    state.mark_dirty_axial(*pos);
                }
            }
        }
        #[cfg(debug_assertions)]
        {
            if fed {
                state.record_food_consumed(UPKEEP_PER_UNIT);
            } else if partial_food > 0.0 {
                state.record_food_consumed(partial_food);
            }
        }
        if !fed {
            starved_units.push(*i);
        }
    }

    for i in starved_units {
        state.units[i].strength -= STARVATION_DAMAGE;
    }

    let convoy_info: Vec<(Axial, u8)> = state.convoys.values().map(|c| (c.pos, c.owner)).collect();

    for (pos, owner) in convoy_info {
        let hex_owner = territory_owner(state, pos);
        let mut consumed = 0.0;
        if let Some(cell) = state.cell_at_mut(pos) {
            if (hex_owner.is_none() || hex_owner == Some(owner))
                && cell.food_stockpile >= UPKEEP_PER_UNIT * 0.5
            {
                cell.food_stockpile -= UPKEEP_PER_UNIT * 0.5;
                state.mark_dirty_axial(pos);
                consumed = UPKEEP_PER_UNIT * 0.5;
            }
        }
        #[cfg(debug_assertions)]
        state.record_food_consumed(consumed);
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
    let moves: Vec<(super::state::UnitKey, Axial, u8, bool)> = state
        .units
        .iter()
        .filter_map(|(key, u)| {
            let dest = u.destination?;
            if u.move_cooldown > 0 || !u.engagements.is_empty() {
                return None;
            }

            match pathfinding::next_step(state, u.pos, dest) {
                Some(next_pos) => {
                    let cooldown = movement_cooldown(state, u.pos, next_pos, false);
                    Some((key, next_pos, cooldown, next_pos == dest))
                }
                None => Some((key, u.pos, 0, true)),
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

fn move_convoys(state: &mut GameState) {
    let convoy_states: Vec<(ConvoyKey, Axial, Axial, u8)> = state
        .convoys
        .iter()
        .filter(|(_, c)| c.move_cooldown == 0 && c.pos != c.destination)
        .map(|(key, c)| (key, c.pos, c.destination, c.owner))
        .collect();

    for (id, pos, dest, owner) in convoy_states {
        if let Some(next) = pathfinding::next_step(state, pos, dest) {
            let cooldown = movement_cooldown(state, pos, next, true);
            if let Some(convoy) = state.convoys.get_mut(id) {
                convoy.pos = next;
                convoy.move_cooldown = cooldown;
            }
            if let Some((enemy, raid_hex)) = convoy_raider(state, next, owner) {
                if let Some(convoy) = state.convoys.remove(id) {
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
                if let Some(convoy) = state.convoys.remove(id) {
                    match convoy.cargo_type {
                        CargoType::Settlers => {
                            let dest_claimed = {
                                let (row, col) = hex::axial_to_offset(dest);
                                if row >= 0
                                    && col >= 0
                                    && (row as usize) < state.height
                                    && (col as usize) < state.width
                                {
                                    let idx = row as usize * state.width + col as usize;
                                    let tc = state.territory_cache.get(idx).copied().flatten();
                                    tc.is_none() || tc == Some(owner)
                                } else {
                                    false
                                }
                            };
                            if dest_claimed
                                && state
                                    .cell_at(dest)
                                    .map(|c| c.terrain_value > 0.0)
                                    .unwrap_or(false)
                                && !state.is_settlement(owner, dest)
                            {
                                let settler_count = convoy.cargo_amount.round() as u16;
                                if let Some(target) = state.population.iter_mut().find(|p| {
                                    p.1.owner == owner && p.1.hex == dest && p.1.role == Role::Idle
                                }) {
                                    target.1.count += settler_count;
                                } else {
                                    state.population.insert(Population {
                                        public_id: state.next_pop_id,
                                        hex: dest,
                                        owner,
                                        count: settler_count,
                                        role: Role::Idle,
                                        training: 0.0,
                                    });
                                    state.next_pop_id += 1;
                                }
                                // Create a Settlement entity for the new colony.
                                let stype = if settler_count >= super::CITY_THRESHOLD {
                                    SettlementType::City
                                } else if settler_count >= super::VILLAGE_THRESHOLD {
                                    SettlementType::Village
                                } else {
                                    SettlementType::Farm
                                };
                                state.settlements.insert(Settlement {
                                    public_id: state.next_settlement_id,
                                    hex: dest,
                                    owner,
                                    settlement_type: stype,
                                });
                                state.next_settlement_id += 1;
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
                                state.convoys.insert(Convoy {
                                    public_id: state.next_convoy_id,
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
        .values()
        .filter(|u| u.owner != owner)
        .filter(|u| u.pos == pos || hex::distance(u.pos, pos) == 1)
        .min_by_key(|u| u.public_id)
        .map(|u| (u.owner, u.pos))
}

fn add_convoy_cargo_to_cell(
    state: &mut GameState,
    ax: Axial,
    _owner: u8,
    cargo_type: CargoType,
    amount: f32,
) {
    let mut food_overflow = 0.0;
    let mut material_overflow = 0.0;
    let mut changed = false;
    if let Some(cell) = state.cell_at_mut(ax) {
        let cap = storage_cap(cell.has_depot);
        match cargo_type {
            CargoType::Food => {
                let next = (cell.food_stockpile + amount).clamp(0.0, cap);
                food_overflow = (cell.food_stockpile + amount - next).max(0.0);
                changed |= (next - cell.food_stockpile).abs() > 0.0001;
                cell.food_stockpile = next;
            }
            CargoType::Material => {
                let next = (cell.material_stockpile + amount).clamp(0.0, cap);
                material_overflow = (cell.material_stockpile + amount - next).max(0.0);
                changed |= (next - cell.material_stockpile).abs() > 0.0001;
                cell.material_stockpile = next;
            }
            CargoType::Settlers => {}
        }
    }
    #[cfg(debug_assertions)]
    {
        state.record_food_destroyed(food_overflow);
        state.record_material_destroyed(material_overflow);
    }
    if changed {
        state.mark_dirty_axial(ax);
    }
}

fn decay_frontier_stockpiles(state: &mut GameState) {
    // Build a mask: for each claimed hex, is it within settlement support?
    let support_mask: Vec<bool> = (0..state.grid.len())
        .map(|idx| {
            if let Some(owner) = state.territory_cache[idx] {
                has_settlement_support(state, owner, offset_index_to_axial(state, idx))
            } else {
                false
            }
        })
        .collect();

    for (idx, supported) in support_mask.into_iter().enumerate() {
        if !supported {
            let mut food_destroyed = 0.0;
            let mut material_destroyed = 0.0;
            let cell = &mut state.grid[idx];
            // Decay cells that have stockpiles but are not within settlement support.
            if cell.food_stockpile > 0.0 || cell.material_stockpile > 0.0 {
                food_destroyed = cell.food_stockpile * FRONTIER_DECAY_RATE;
                material_destroyed = cell.material_stockpile * FRONTIER_DECAY_RATE;
                cell.food_stockpile *= 1.0 - FRONTIER_DECAY_RATE;
                cell.material_stockpile *= 1.0 - FRONTIER_DECAY_RATE;
                state.mark_dirty_index(idx);
            }
            #[cfg(debug_assertions)]
            {
                state.record_food_destroyed(food_destroyed);
                state.record_material_destroyed(material_destroyed);
            }
        }
    }
}

fn offset_index_to_axial(state: &GameState, idx: usize) -> Axial {
    let row = idx / state.width;
    let col = idx % state.width;
    hex::offset_to_axial(row as i32, col as i32)
}

fn decrement_cooldowns(state: &mut GameState) {
    for (_, unit) in &mut state.units {
        if unit.move_cooldown > 0 {
            unit.move_cooldown -= 1;
        }
    }
    for (_, convoy) in &mut state.convoys {
        if convoy.move_cooldown > 0 {
            convoy.move_cooldown -= 1;
        }
    }
}

fn cleanup(state: &mut GameState) {
    combat::cleanup_engagements(state);

    // Record unit deaths before removing them
    if state.game_log.is_some() {
        let dead: Vec<_> = state
            .units
            .iter()
            .filter(|(_, u)| u.strength <= 0.0)
            .map(|(_, u)| {
                let killer = u
                    .engagements
                    .first()
                    .and_then(|e| state.units.get(e.enemy_id).map(|enemy| enemy.owner));
                (u.public_id, u.owner, u.pos, killer, u.is_general)
            })
            .collect();
        if let Some(log) = &mut state.game_log {
            for (unit_id, player, pos, killer, is_general) in dead {
                log.record(super::gamelog::GameEvent::UnitKilled {
                    tick: state.tick,
                    player,
                    unit_id,
                    pos,
                    killer,
                    was_general: is_general,
                });
            }
        }
    }

    state.units.retain(|_, u| u.strength > 0.0);

    let eliminated: Vec<u8> = state
        .players
        .iter()
        .filter(|p| p.alive)
        .filter(|p| !state.units.contains_key(p.general_id))
        .map(|p| p.id)
        .collect();

    for pid in eliminated {
        if let Some(log) = &mut state.game_log {
            log.record(super::gamelog::GameEvent::PlayerEliminated {
                tick: state.tick,
                player: pid,
            });
        }
        if let Some(player) = state.players.iter_mut().find(|p| p.id == pid) {
            player.alive = false;
        }
        let removed_ids: Vec<_> = state
            .units
            .iter()
            .filter(|(_, u)| u.owner == pid)
            .map(|(key, _)| key)
            .collect();
        state.units.retain(|_, u| u.owner != pid);
        state.population.retain(|_, p| p.owner != pid);
        #[cfg(debug_assertions)]
        {
            let mut convoy_food = 0.0;
            let mut convoy_material = 0.0;
            for (_, convoy) in state.convoys.iter() {
                if convoy.owner == pid {
                    match convoy.cargo_type {
                        CargoType::Food => convoy_food += convoy.cargo_amount,
                        CargoType::Material => convoy_material += convoy.cargo_amount,
                        CargoType::Settlers => {}
                    }
                }
            }
            state.record_food_destroyed(convoy_food);
            state.record_material_destroyed(convoy_material);
        }
        state.convoys.retain(|_, c| c.owner != pid);
        // Remove all settlements belonging to the eliminated player.
        state.settlements.retain(|_, s| s.owner != pid);

        let mut cleared = Vec::new();
        let mut destroyed_food = 0.0;
        let mut destroyed_material = 0.0;
        for (idx, cell) in state.grid.iter_mut().enumerate() {
            if cell.stockpile_owner == Some(pid) {
                destroyed_food += cell.food_stockpile;
                destroyed_material += cell.material_stockpile;
                cell.stockpile_owner = None;
                cell.food_stockpile = 0.0;
                cell.material_stockpile = 0.0;
                cleared.push(idx);
            }
        }
        // Also clear territory_cache for this player.
        for slot in state.territory_cache.iter_mut() {
            if *slot == Some(pid) {
                *slot = None;
            }
        }
        #[cfg(debug_assertions)]
        {
            state.record_food_destroyed(destroyed_food);
            state.record_material_destroyed(destroyed_material);
        }
        for idx in cleared {
            state.mark_dirty_index(idx);
        }
        for (_, unit) in &mut state.units {
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
    let unit_positions: HashMap<_, _> = state.units.iter().map(|(key, u)| (key, u.pos)).collect();

    for (_, unit) in &mut state.units {
        unit.engagements
            .retain(|eng| match unit_positions.get(&eng.enemy_id) {
                None => {
                    tracing::warn!(
                        tick = state.tick,
                        unit_id = unit.public_id,
                        enemy_id = eng.enemy_id.data().as_ffi(),
                        edge = eng.edge,
                        "removing stale engagement: enemy does not exist"
                    );
                    false
                }
                Some(&enemy_pos) => {
                    if hex::shared_edge(unit.pos, enemy_pos).is_none() {
                        tracing::warn!(
                            tick = state.tick,
                            unit_id = unit.public_id,
                            enemy_id = eng.enemy_id.data().as_ffi(),
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

#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy)]
struct EconomicSnapshot {
    food_assets: f32,
    material_assets: f32,
}

#[cfg(debug_assertions)]
fn economic_snapshot(state: &GameState) -> EconomicSnapshot {
    let mut food_assets = 0.0;
    let mut material_assets = 0.0;
    for cell in &state.grid {
        food_assets += cell.food_stockpile;
        material_assets += cell.material_stockpile;
    }
    for convoy in state.convoys.values() {
        match convoy.cargo_type {
            CargoType::Food => food_assets += convoy.cargo_amount,
            CargoType::Material => material_assets += convoy.cargo_amount,
            CargoType::Settlers => {}
        }
    }
    EconomicSnapshot {
        food_assets,
        material_assets,
    }
}

#[cfg(debug_assertions)]
fn debug_assert_economy_sane(state: &GameState, phase: &str, pre_snapshot: EconomicSnapshot) {
    let snapshot = economic_snapshot(state);
    let acc = state.tick_accumulator.unwrap_or_default();
    let expected_food_delta = acc.food_produced - acc.food_consumed - acc.food_destroyed;
    let expected_material_delta =
        acc.material_produced - acc.material_consumed - acc.material_destroyed;
    // All production/consumption/destruction flows should be ledgered. Tolerance covers
    // f32 rounding across many operations per tick.
    let tolerance = 1.0;
    assert!(
        ((snapshot.food_assets - pre_snapshot.food_assets) - expected_food_delta).abs() < tolerance,
        "{phase} tick {}: food conservation violated delta={} expected={}",
        state.tick,
        snapshot.food_assets - pre_snapshot.food_assets,
        expected_food_delta
    );
    assert!(
        ((snapshot.material_assets - pre_snapshot.material_assets) - expected_material_delta).abs()
            < tolerance,
        "{phase} tick {}: material conservation violated delta={} expected={}",
        state.tick,
        snapshot.material_assets - pre_snapshot.material_assets,
        expected_material_delta
    );
    assert!(
        snapshot.food_assets.is_finite() && snapshot.food_assets >= -0.001,
        "{phase} tick {}: invalid total food assets {}",
        state.tick,
        snapshot.food_assets
    );
    assert!(
        snapshot.material_assets.is_finite() && snapshot.material_assets >= -0.001,
        "{phase} tick {}: invalid total material assets {}",
        state.tick,
        snapshot.material_assets
    );
    for (idx, cell) in state.grid.iter().enumerate() {
        assert!(
            cell.food_stockpile.is_finite() && cell.food_stockpile >= -0.001,
            "{phase} tick {}: invalid food stockpile at cell {idx}: {}",
            state.tick,
            cell.food_stockpile
        );
        assert!(
            cell.material_stockpile.is_finite() && cell.material_stockpile >= -0.001,
            "{phase} tick {}: invalid material stockpile at cell {idx}: {}",
            state.tick,
            cell.material_stockpile
        );
    }
    for convoy in state.convoys.values() {
        assert!(
            convoy.cargo_amount.is_finite() && convoy.cargo_amount >= -0.001,
            "{phase} tick {}: invalid convoy cargo on {}",
            state.tick,
            convoy.public_id
        );
    }
    let mut food_by_owner = vec![0.0f32; state.players.len()];
    let mut material_by_owner = vec![0.0f32; state.players.len()];
    for cell in &state.grid {
        if let Some(owner) = cell.stockpile_owner {
            food_by_owner[owner as usize] += cell.food_stockpile;
            material_by_owner[owner as usize] += cell.material_stockpile;
        }
    }
    for player in &state.players {
        assert!(
            (player.food - food_by_owner[player.id as usize]).abs() < 0.01,
            "{phase} tick {}: player {} food total mismatch {} vs {}",
            state.tick,
            player.id,
            player.food,
            food_by_owner[player.id as usize]
        );
        assert!(
            (player.material - material_by_owner[player.id as usize]).abs() < 0.01,
            "{phase} tick {}: player {} material total mismatch {} vs {}",
            state.tick,
            player.id,
            player.material,
            material_by_owner[player.id as usize]
        );
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

    fn non_general_key(state: &GameState, owner: u8) -> crate::v2::state::UnitKey {
        state
            .units
            .iter()
            .find_map(|(key, unit)| (unit.owner == owner && !unit.is_general).then_some(key))
            .unwrap()
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
        for (_, pop) in &mut state.population {
            pop.count = 0;
        }
        for (_, unit) in &mut state.units {
            unit.destination = Some(offset_to_axial(0, 0));
        }
        let idx = non_general_key(&state, 0);
        let before = state.units[idx].strength;
        tick(&mut state);
        assert!(state.units[idx].strength < before);
    }

    #[test]
    fn unit_moves_toward_destination() {
        let mut state = test_state();
        let unit_idx = non_general_key(&state, 0);
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
        let unit_idx = non_general_key(&state, 0);
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
        let unit_idx = non_general_key(&state, 0);
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
            .values()
            .find(|u| u.owner == 0 && u.is_general)
            .unwrap()
            .pos;
        let before = state.units.values().filter(|u| u.owner == 0).count();
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
        let before = state.units.values().filter(|u| u.owner == 0).count();
        // Run enough ticks for soldiers to finish training; city AI will auto-produce.
        for _ in 0..30 {
            tick(&mut state);
        }
        let after = state.units.values().filter(|u| u.owner == 0).count();
        // City AI auto-produces once trained soldiers are ready, so unit count grows.
        assert!(
            after > before,
            "expected unit count to grow via city AI auto-produce"
        );
    }

    #[test]
    fn unsupported_frontier_stockpiles_decay() {
        let mut state = test_state();
        let general_pos = state
            .units
            .values()
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
            .values()
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
        let convoy_id = state.convoys.keys().next().unwrap();
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
        for (_, pop) in state.population.iter_mut().filter(|(_, p)| p.owner == 0) {
            pop.count = pop.count.saturating_add(20);
        }

        assert_eq!(winner_at_limit(&state, TIMEOUT_TICKS), Some(0));
        assert!(reached_timeout(&state, TIMEOUT_TICKS));
    }

    #[test]
    fn stale_engagements_are_removed() {
        let mut state = test_state();
        let a_idx = non_general_key(&state, 0);
        let b_idx = non_general_key(&state, 1);
        state.units[a_idx]
            .engagements
            .push(crate::v2::state::Engagement {
                enemy_id: b_idx,
                edge: 0,
            });
        state.units[b_idx]
            .engagements
            .push(crate::v2::state::Engagement {
                enemy_id: a_idx,
                edge: 3,
            });
        state.units[b_idx].pos = offset_to_axial(19, 19);

        tick(&mut state);

        let a = state.units.get(a_idx).unwrap();
        let b = state.units.get(b_idx).unwrap();
        assert!(a.engagements.is_empty());
        assert!(b.engagements.is_empty());
    }

    #[test]
    fn convoy_is_raided_from_adjacent_hex() {
        let mut state = test_state();
        state.units.clear();
        state.population.clear();
        state.convoys.clear();
        state.settlements.clear();
        state.territory_cache.fill(None);
        state.rebuild_spatial();
        for cell in &mut state.grid {
            cell.stockpile_owner = None;
            cell.food_stockpile = 0.0;
            cell.material_stockpile = 0.0;
        }

        let convoy_pos = offset_to_axial(5, 5);
        let destination = neighbors(convoy_pos)[0];
        let raid_hex = neighbors(destination)[1];

        let general_a = state.units.insert(crate::v2::state::Unit {
            public_id: 100,
            owner: 0,
            pos: offset_to_axial(1, 1),
            strength: 100.0,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: true,
        });
        let general_b = state.units.insert(crate::v2::state::Unit {
            public_id: 200,
            owner: 1,
            pos: offset_to_axial(10, 10),
            strength: 100.0,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: true,
        });
        state.units.insert(crate::v2::state::Unit {
            public_id: 201,
            owner: 1,
            pos: raid_hex,
            strength: 100.0,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: false,
        });
        state.players[0].general_id = general_a;
        state.players[1].general_id = general_b;
        state.population.insert(Population {
            public_id: 0,
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
        // Settlement entity so raid_hex has territory support and food doesn't decay.
        state.settlements.insert(crate::v2::state::Settlement {
            public_id: 0,
            hex: raid_hex,
            owner: 1,
            settlement_type: crate::v2::state::SettlementType::Village,
        });
        state.next_settlement_id = 1;
        state.convoys.insert(crate::v2::state::Convoy {
            public_id: 0,
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
        state.rebuild_spatial();

        tick(&mut state);

        assert!(state.convoys.is_empty());
        let raid_cell = state.cell_at(raid_hex).unwrap();
        assert_eq!(raid_cell.stockpile_owner, Some(1));
        assert_eq!(raid_cell.food_stockpile, 9.0);
    }
}
