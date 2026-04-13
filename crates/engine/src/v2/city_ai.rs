/// Autonomous city AI that manages population roles, infrastructure, settlers, and resource convoys.
/// Runs every CITY_AI_INTERVAL ticks, directly mutating GameState.
use super::hex::{self, Axial};
use super::state::{CargoType, Convoy, GameState, Population, Role, SettlementType};
use super::{
    CITY_THRESHOLD, CONVOY_CAPACITY, CONVOY_MOVE_COOLDOWN, DEPOT_BUILD_COST, FARM_CONVOY_SIZE,
    ROAD_LEVEL2_COST, SETTLER_CONVOY_SIZE, SOLDIER_EQUIP_COST, TRAIN_BATCH_SIZE,
};

/// Entry point called every CITY_AI_INTERVAL ticks.
pub fn run_city_ai(state: &mut GameState) {
    let player_ids: Vec<u8> = state
        .players
        .iter()
        .filter(|p| p.alive)
        .map(|p| p.id)
        .collect();

    for player_id in player_ids {
        manage_population_roles(state, player_id);
        manage_infrastructure(state, player_id);
        manage_auto_settlers(state, player_id);
        manage_resource_convoys(state, player_id);
        produce_units_from_settlements(state, player_id);
    }
}

// ---------------------------------------------------------------------------
// Population role management
// ---------------------------------------------------------------------------

fn manage_population_roles(state: &mut GameState, player_id: u8) {
    let settlement_hexes: Vec<(Axial, SettlementType)> = state
        .settlements
        .values()
        .filter(|s| s.owner == player_id)
        .map(|s| (s.hex, s.settlement_type))
        .collect();

    for (hex, stype) in settlement_hexes {
        let (target_farmer_pct, target_worker_pct) = match stype {
            SettlementType::Farm => (0.70, 0.20),
            SettlementType::Village => (0.50, 0.25),
            SettlementType::City => (0.40, 0.20),
        };

        // Collect current counts without holding a borrow.
        let (idle, farmers, workers, _trained, _untrained) = population_mix(state, player_id, hex);
        let total = (idle + farmers + workers + _trained + _untrained) as f32;
        if total == 0.0 {
            continue;
        }
        let target_farmers = (total * target_farmer_pct).ceil() as u16;
        let target_workers = (total * target_worker_pct).ceil() as u16;

        if farmers < target_farmers && idle > 0 {
            let needed = (target_farmers - farmers).min(idle).min(5);
            reassign_idle(state, player_id, hex, Role::Farmer, needed);
        } else if workers < target_workers && idle > 0 {
            let needed = (target_workers - workers).min(idle).min(3);
            reassign_idle(state, player_id, hex, Role::Worker, needed);
        } else if idle > 0 {
            // Train remaining idle as soldiers.
            train_soldiers_ai(state, player_id, hex);
        }
    }
}

fn reassign_idle(state: &mut GameState, player_id: u8, hex: Axial, role: Role, count: u16) {
    let mut remaining = count;
    for (_, pop) in state.population.iter_mut() {
        if remaining == 0 {
            break;
        }
        if pop.owner != player_id || pop.hex != hex || pop.role != Role::Idle {
            continue;
        }
        let take = remaining.min(pop.count);
        pop.count -= take;
        remaining -= take;
    }
    state.population.retain(|_, p| p.count > 0);
    if remaining < count {
        let assigned = count - remaining;
        if let Some(existing) = state.population.iter_mut().find(|(_, p)| {
            p.owner == player_id && p.hex == hex && p.role == role && p.training == 0.0
        }) {
            existing.1.count += assigned;
        } else {
            state.population.insert(Population {
                public_id: state.next_pop_id,
                hex,
                owner: player_id,
                count: assigned,
                role,
                training: 0.0,
            });
            state.next_pop_id += 1;
        }
    }
}

fn train_soldiers_ai(state: &mut GameState, player_id: u8, hex: Axial) {
    let affordable = state
        .cell_at(hex)
        .map(|c| (c.material_stockpile / SOLDIER_EQUIP_COST).floor() as u16)
        .unwrap_or(0);
    if affordable == 0 {
        return;
    }
    let batch = affordable.min(TRAIN_BATCH_SIZE);
    let idle_key = state.population.iter().find_map(|(key, p)| {
        (p.owner == player_id && p.hex == hex && p.role == Role::Idle && p.count > 0).then_some(key)
    });
    let Some(key) = idle_key else { return };
    let take = batch.min(state.population[key].count);
    if let Some(cell) = state.cell_at_mut(hex) {
        cell.material_stockpile -= take as f32 * SOLDIER_EQUIP_COST;
        state.mark_dirty_axial(hex);
    }
    #[cfg(debug_assertions)]
    state.record_material_consumed(take as f32 * SOLDIER_EQUIP_COST);
    let idle_count = state.population[key].count;
    if idle_count == take {
        state.population[key].role = Role::Soldier;
        state.population[key].training = 0.0;
    } else {
        state.population[key].count -= take;
        state.population.insert(Population {
            public_id: state.next_pop_id,
            hex,
            owner: player_id,
            count: take,
            role: Role::Soldier,
            training: 0.0,
        });
        state.next_pop_id += 1;
    }
}

// ---------------------------------------------------------------------------
// Infrastructure
// ---------------------------------------------------------------------------

fn manage_infrastructure(state: &mut GameState, player_id: u8) {
    let settlement_hexes: Vec<(Axial, SettlementType)> = state
        .settlements
        .values()
        .filter(|s| s.owner == player_id)
        .map(|s| (s.hex, s.settlement_type))
        .collect();

    for (hex, stype) in settlement_hexes {
        if stype == SettlementType::Farm {
            continue;
        }

        let (has_depot, road_level, material) = state
            .cell_at(hex)
            .map(|c| (c.has_depot, c.road_level, c.material_stockpile))
            .unwrap_or((true, 3, 0.0));

        if !has_depot && material >= DEPOT_BUILD_COST {
            if let Some(cell) = state.cell_at_mut(hex) {
                cell.material_stockpile -= DEPOT_BUILD_COST;
                cell.has_depot = true;
                state.mark_dirty_axial(hex);
            }
            #[cfg(debug_assertions)]
            state.record_material_consumed(DEPOT_BUILD_COST);
        } else if road_level == 0 {
            if let Some(cell) = state.cell_at_mut(hex) {
                cell.road_level = 1;
                state.mark_dirty_axial(hex);
            }
        } else if road_level == 1 && material >= ROAD_LEVEL2_COST {
            if let Some(cell) = state.cell_at_mut(hex) {
                cell.material_stockpile -= ROAD_LEVEL2_COST;
                cell.road_level = 2;
                state.mark_dirty_axial(hex);
            }
            #[cfg(debug_assertions)]
            state.record_material_consumed(ROAD_LEVEL2_COST);
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-settler dispatch from Cities
// ---------------------------------------------------------------------------

fn manage_auto_settlers(state: &mut GameState, player_id: u8) {
    let city_hexes: Vec<Axial> = state
        .settlements
        .values()
        .filter(|s| s.owner == player_id && s.settlement_type == SettlementType::City)
        .map(|s| s.hex)
        .collect();

    for city_hex in city_hexes {
        let city_pop = state.population_on_hex(player_id, city_hex);
        // Only dispatch if city has headroom above settler cost.
        if city_pop < CITY_THRESHOLD + SETTLER_CONVOY_SIZE + 5 {
            continue;
        }

        // Check we don't already have a settler convoy enroute.
        let has_settler_convoy = state
            .convoys
            .values()
            .any(|c| c.owner == player_id && c.cargo_type == CargoType::Settlers);
        if has_settler_convoy {
            continue;
        }

        // Pick a target hex.
        let Some(target) = pick_settler_target(state, player_id, city_hex) else {
            continue;
        };

        let convoy_size = if hex::distance(city_hex, target) <= 3 {
            FARM_CONVOY_SIZE
        } else {
            SETTLER_CONVOY_SIZE
        };

        // Remove population from the city.
        let mut remaining = convoy_size;
        for (_, pop) in state.population.iter_mut() {
            if remaining == 0 {
                break;
            }
            if pop.owner != player_id || pop.hex != city_hex || pop.role == Role::Soldier {
                continue;
            }
            let take = remaining.min(pop.count);
            pop.count -= take;
            remaining -= take;
        }
        state.population.retain(|_, p| p.count > 0);

        if remaining > 0 {
            // Not enough non-soldier population; skip.
            continue;
        }

        state.convoys.insert(Convoy {
            public_id: state.next_convoy_id,
            owner: player_id,
            pos: city_hex,
            origin: city_hex,
            destination: target,
            cargo_type: CargoType::Settlers,
            cargo_amount: convoy_size as f32,
            capacity: convoy_size as f32,
            speed: 1.0,
            move_cooldown: CONVOY_MOVE_COOLDOWN,
            returning: false,
        });
        state.next_convoy_id += 1;
    }
}

fn pick_settler_target(state: &GameState, player_id: u8, origin: Axial) -> Option<Axial> {
    let existing: Vec<Axial> = state
        .settlements
        .values()
        .filter(|s| s.owner == player_id)
        .map(|s| s.hex)
        .collect();

    let mut best: Option<(Axial, f32)> = None;

    for (idx, tc) in state.territory_cache.iter().enumerate() {
        // Only consider own or unclaimed hexes.
        if tc.is_some_and(|o| o != player_id) {
            continue;
        }
        let row = idx / state.width;
        let col = idx % state.width;
        let ax = hex::offset_to_axial(row as i32, col as i32);

        // Must not already have a settlement.
        if existing.contains(&ax) {
            continue;
        }
        // Must not have population already settled there.
        if state.population_on_hex(player_id, ax) > 0 {
            continue;
        }

        let dist = hex::distance(origin, ax);
        if dist < 2 || dist > 10 {
            continue;
        }

        // Too close to an existing settlement.
        let min_settlement_dist = existing
            .iter()
            .map(|s| hex::distance(*s, ax))
            .min()
            .unwrap_or(i32::MAX);
        if min_settlement_dist < 2 {
            continue;
        }

        let fertility = state.cell_at(ax).map(|c| c.terrain_value).unwrap_or(0.0);
        if fertility <= 0.0 {
            continue;
        }

        let score = fertility * 2.0 - dist as f32 * 0.2;
        match best {
            Some((_, bs)) if bs >= score => {}
            _ => best = Some((ax, score)),
        }
    }

    best.map(|(ax, _)| ax)
}

// ---------------------------------------------------------------------------
// Resource convoy routing: surplus food/material to nearest settlement
// ---------------------------------------------------------------------------

fn manage_resource_convoys(state: &mut GameState, player_id: u8) {
    let general_hex: Option<Axial> = state.general_pos(player_id);

    // Collect surplus non-settlement hexes
    let surplus: Vec<(usize, f32, f32)> = state
        .territory_cache
        .iter()
        .enumerate()
        .filter(|(_, tc)| **tc == Some(player_id))
        .filter_map(|(idx, _)| {
            let cell = &state.grid[idx];
            if cell.food_stockpile > 20.0 || cell.material_stockpile > 15.0 {
                Some((idx, cell.food_stockpile, cell.material_stockpile))
            } else {
                None
            }
        })
        .collect();

    let settlement_hexes: Vec<Axial> = state
        .settlements
        .values()
        .filter(|s| s.owner == player_id)
        .map(|s| s.hex)
        .collect();

    for (idx, food, material) in surplus {
        let row = idx / state.width;
        let col = idx % state.width;
        let hex = hex::offset_to_axial(row as i32, col as i32);

        // Skip if this hex IS a settlement.
        if settlement_hexes.contains(&hex) {
            continue;
        }
        // Skip if already has a convoy from this hex.
        if state
            .convoys
            .values()
            .any(|c| c.owner == player_id && c.origin == hex && !c.returning)
        {
            continue;
        }

        // Find nearest settlement or general as destination.
        let dest = settlement_hexes
            .iter()
            .chain(general_hex.iter())
            .min_by_key(|&&s| hex::distance(hex, s))
            .copied();

        let Some(destination) = dest else { continue };
        if destination == hex {
            continue;
        }

        let (cargo_type, amount) = if food > 20.0 {
            (CargoType::Food, food.min(CONVOY_CAPACITY))
        } else {
            (CargoType::Material, material.min(CONVOY_CAPACITY))
        };

        let actual_amount = if let Some(cell) = state.cell_at_mut(hex) {
            match cargo_type {
                CargoType::Food => {
                    let amt = cell.food_stockpile.min(amount);
                    cell.food_stockpile -= amt;
                    state.mark_dirty_axial(hex);
                    amt
                }
                CargoType::Material => {
                    let amt = cell.material_stockpile.min(amount);
                    cell.material_stockpile -= amt;
                    state.mark_dirty_axial(hex);
                    amt
                }
                CargoType::Settlers => 0.0,
            }
        } else {
            0.0
        };
        // Convoy cargo is accounted as consumed from cell here; it reappears
        // when delivered. The conservation check covers cell + convoy totals,
        // so no explicit ledger entry is needed here.

        if actual_amount <= 0.0 {
            continue;
        }

        state.convoys.insert(Convoy {
            public_id: state.next_convoy_id,
            owner: player_id,
            pos: hex,
            origin: hex,
            destination,
            cargo_type,
            cargo_amount: actual_amount,
            capacity: CONVOY_CAPACITY,
            speed: 1.0,
            move_cooldown: CONVOY_MOVE_COOLDOWN,
            returning: false,
        });
        state.next_convoy_id += 1;
    }
}

// ---------------------------------------------------------------------------
// Unit production at settlements
// ---------------------------------------------------------------------------

fn produce_units_from_settlements(state: &mut GameState, player_id: u8) {
    use super::{
        INITIAL_STRENGTH, SOLDIER_READY_THRESHOLD, SOLDIERS_PER_UNIT, UNIT_FOOD_COST,
        UNIT_MATERIAL_COST,
    };

    let settlement_hexes: Vec<(Axial, SettlementType)> = state
        .settlements
        .values()
        .filter(|s| s.owner == player_id)
        .filter(|s| s.settlement_type != SettlementType::Farm)
        .map(|s| (s.hex, s.settlement_type))
        .collect();

    for (hex, _stype) in settlement_hexes {
        let trained: u16 = state
            .population
            .values()
            .filter(|p| {
                p.owner == player_id
                    && p.hex == hex
                    && p.role == Role::Soldier
                    && p.training >= SOLDIER_READY_THRESHOLD
            })
            .map(|p| p.count)
            .sum();

        if trained < SOLDIERS_PER_UNIT {
            continue;
        }

        let (food, material) = state
            .cell_at(hex)
            .map(|c| (c.food_stockpile, c.material_stockpile))
            .unwrap_or((0.0, 0.0));
        if food < UNIT_FOOD_COST || material < UNIT_MATERIAL_COST {
            continue;
        }

        // Find a spawn position adjacent to this hex.
        let neighbors = hex::neighbors(hex);
        let spawn_pos = neighbors
            .iter()
            .filter(|&&n| state.in_bounds(n))
            .find(|&&n| !state.has_unit_at(n))
            .or_else(|| neighbors.iter().find(|&&n| state.in_bounds(n)))
            .copied();
        let Some(spawn_pos) = spawn_pos else { continue };

        // Consume soldiers.
        let mut remaining = SOLDIERS_PER_UNIT;
        for (_, pop) in state.population.iter_mut().filter(|(_, p)| {
            p.owner == player_id
                && p.hex == hex
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
        state.population.retain(|_, p| p.count > 0);

        if let Some(cell) = state.cell_at_mut(hex) {
            cell.food_stockpile -= UNIT_FOOD_COST;
            cell.material_stockpile -= UNIT_MATERIAL_COST;
            state.mark_dirty_axial(hex);
        }
        #[cfg(debug_assertions)]
        {
            state.record_food_consumed(UNIT_FOOD_COST);
            state.record_material_consumed(UNIT_MATERIAL_COST);
        }

        let unit_id = state.next_unit_id;
        state.next_unit_id += 1;
        state.units.insert(super::state::Unit {
            public_id: unit_id,
            owner: player_id,
            pos: spawn_pos,
            strength: INITIAL_STRENGTH,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            is_general: false,
        });
        state.rebuild_spatial();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn population_mix(state: &GameState, player_id: u8, hex: Axial) -> (u16, u16, u16, u16, u16) {
    let mut idle = 0u16;
    let mut farmers = 0u16;
    let mut workers = 0u16;
    let mut trained = 0u16;
    let mut untrained = 0u16;
    for pop in state
        .population
        .values()
        .filter(|p| p.owner == player_id && p.hex == hex)
    {
        match pop.role {
            Role::Idle => idle += pop.count,
            Role::Farmer => farmers += pop.count,
            Role::Worker => workers += pop.count,
            Role::Soldier => {
                if pop.training >= super::SOLDIER_READY_THRESHOLD {
                    trained += pop.count;
                } else {
                    untrained += pop.count;
                }
            }
        }
    }
    (idle, farmers, workers, trained, untrained)
}
