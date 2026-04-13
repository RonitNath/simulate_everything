use std::collections::{BTreeMap, HashMap};

use super::agent::{Agent, SpreadAgent};
use super::combat;
use super::directive::Directive;
use super::hex::{Axial, distance, neighbors, offset_to_axial};
use super::mapgen::{MapConfig, generate};
use super::observation::{self, InitialObservation, ObservationDelta};
use super::replay;
use super::runner;
use super::sim;
use super::spatial::SpatialIndex;
use super::state::{
    Biome, Cell, Convoy, GameState, Player, Population, Role, Settlement, SettlementType,
    TickAccumulator, Unit,
};
use bitvec::vec::BitVec;
use slotmap::{Key, SlotMap};

struct ScriptedAgent {
    name: &'static str,
    schedule: BTreeMap<u64, Vec<Directive>>,
}

impl ScriptedAgent {
    fn pass() -> Self {
        Self {
            name: "pass",
            schedule: BTreeMap::new(),
        }
    }
}

impl Agent for ScriptedAgent {
    fn name(&self) -> &str {
        self.name
    }

    fn init(&mut self, _obs: &InitialObservation) {}

    fn act(&mut self, obs: &ObservationDelta) -> Vec<Directive> {
        self.schedule.remove(&obs.tick).unwrap_or_default()
    }
}

fn flat_cell() -> Cell {
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
        water_access: 0.5,
        region_id: 0,
        stockpile_owner: None,
    }
}

fn unit(id: u32, owner: u8, pos: Axial) -> Unit {
    Unit {
        public_id: id,
        owner,
        pos,
        strength: 100.0,
        move_cooldown: 0,
        engagements: Vec::new(),
        destination: None,
        rations: super::MAX_RATIONS,
        half_rations: false,
    }
}

fn blank_state(width: usize, height: usize, num_players: u8) -> GameState {
    let mut players = Vec::new();
    let mut units = SlotMap::with_key();
    let mut settlements: SlotMap<super::state::SettlementKey, Settlement> = SlotMap::with_key();
    let mut population: SlotMap<super::state::PopKey, Population> = SlotMap::with_key();
    let mut next_settlement_id: u32 = 0;
    let mut next_pop_id: u32 = 0;

    for player_id in 0..num_players {
        let start_pos = offset_to_axial(0, player_id as i32);
        units.insert(unit(player_id as u32, player_id, start_pos));
        players.push(Player {
            id: player_id,
            food: 0.0,
            material: 0.0,
            alive: true,
        });
        // Each player starts with a settlement with enough population to survive
        // update_settlement_types (FARM_THRESHOLD = 2).
        settlements.insert(Settlement {
            public_id: next_settlement_id,
            hex: start_pos,
            owner: player_id,
            settlement_type: SettlementType::Farm,
        });
        next_settlement_id += 1;
        population.insert(Population {
            public_id: next_pop_id,
            hex: start_pos,
            owner: player_id,
            count: 5,
            role: Role::Idle,
            training: 0.0,
        });
        next_pop_id += 1;
    }

    let mut state = GameState {
        width,
        height,
        grid: vec![flat_cell(); width * height],
        units,
        players,
        population,
        convoys: SlotMap::with_key(),
        settlements,
        regions: Vec::new(),
        tick: 0,
        next_unit_id: num_players as u32,
        next_pop_id,
        next_convoy_id: 0,
        next_settlement_id,
        scouted: vec![vec![true; width * height]; num_players as usize],
        spatial: SpatialIndex::new(width, height),
        dirty_hexes: BitVec::repeat(false, width * height),
        hex_revisions: vec![0; width * height],
        next_hex_revision: 0,
        territory_cache: vec![None; width * height],
        #[cfg(debug_assertions)]
        tick_accumulator: Some(TickAccumulator::default()),
        game_log: None,
    };
    state.rebuild_spatial();
    state
}

fn set_units(state: &mut GameState, units: Vec<Unit>) {
    let mut slotmap = SlotMap::with_key();
    for unit in units {
        slotmap.insert(unit);
    }
    state.units = slotmap;
    state.rebuild_spatial();
}

fn unit_ref(state: &GameState, public_id: u32) -> &Unit {
    state.unit_by_public_id(public_id).unwrap()
}

fn unit_mut(state: &mut GameState, public_id: u32) -> &mut Unit {
    state.unit_by_public_id_mut(public_id).unwrap()
}

fn stockpile_totals(state: &GameState) -> HashMap<u8, (f32, f32)> {
    let mut totals: HashMap<u8, (f32, f32)> = HashMap::new();
    for cell in &state.grid {
        if let Some(owner) = cell.stockpile_owner {
            let entry = totals.entry(owner).or_insert((0.0, 0.0));
            entry.0 += cell.food_stockpile;
            entry.1 += cell.material_stockpile;
        }
    }
    totals
}

fn assert_no_negative_stockpiles(state: &GameState) {
    for (idx, cell) in state.grid.iter().enumerate() {
        assert!(
            cell.food_stockpile >= 0.0,
            "negative food stockpile at cell {idx}: {}",
            cell.food_stockpile
        );
        assert!(
            cell.material_stockpile >= 0.0,
            "negative material stockpile at cell {idx}: {}",
            cell.material_stockpile
        );
    }
}

fn assert_player_totals_match_owned_cells(state: &GameState) {
    let totals = stockpile_totals(state);
    for player in &state.players {
        let (food, material) = totals.get(&player.id).copied().unwrap_or((0.0, 0.0));
        assert!(
            (player.food - food).abs() < 0.001,
            "player {} food total mismatch: {} vs {}",
            player.id,
            player.food,
            food
        );
        assert!(
            (player.material - material).abs() < 0.001,
            "player {} material total mismatch: {} vs {}",
            player.id,
            player.material,
            material
        );
    }
}

fn assert_entities_in_bounds(state: &GameState) {
    for unit in state.units.values() {
        assert!(
            state.in_bounds(unit.pos),
            "unit {} out of bounds",
            unit.public_id
        );
    }
    for convoy in state.convoys.values() {
        assert!(
            state.in_bounds(convoy.pos),
            "convoy {} out of bounds",
            convoy.public_id
        );
        assert!(
            state.in_bounds(convoy.destination),
            "convoy {} destination out of bounds",
            convoy.public_id
        );
    }
    for pop in state.population.values() {
        assert!(
            state.in_bounds(pop.hex),
            "population {} out of bounds",
            pop.public_id
        );
    }
}

fn assert_no_orphaned_engagements(state: &GameState) {
    let positions: HashMap<_, _> = state
        .units
        .iter()
        .map(|(key, unit)| (key, unit.pos))
        .collect();
    for unit in state.units.values() {
        for engagement in &unit.engagements {
            let enemy_pos = positions
                .get(&engagement.enemy_id)
                .copied()
                .expect("engagement references missing enemy");
            assert!(
                distance(unit.pos, enemy_pos) == 1,
                "unit {} engaged with non-adjacent enemy {}",
                unit.public_id,
                engagement.enemy_id.data().as_ffi()
            );
        }
    }
}

fn run_ticks(state: &mut GameState, agents: &mut [Box<dyn Agent>], ticks: usize) {
    for _ in 0..ticks {
        runner::advance_game_tick(state, agents);
    }
}

#[test]
fn passive_economy_preserves_non_negative_stockpiles_and_player_totals() {
    let mut state = generate(&MapConfig {
        width: 15,
        height: 15,
        num_players: 2,
        seed: 7,
    });
    let mut agents: Vec<Box<dyn Agent>> = vec![
        Box::new(ScriptedAgent::pass()),
        Box::new(ScriptedAgent::pass()),
    ];

    // City AI now runs autonomously (building infrastructure, training soldiers, etc.)
    // so material can decrease without explicit directives. We only verify that
    // stockpiles stay non-negative and player totals are consistent.
    for _ in 0..40 {
        runner::advance_game_tick(&mut state, &mut agents);
        assert_no_negative_stockpiles(&state);
        assert_player_totals_match_owned_cells(&state);
    }
}

#[test]
fn convoy_raiding_transfers_cargo_to_adjacent_raider_hex() {
    let mut state = blank_state(12, 12, 2);
    let convoy_start = offset_to_axial(5, 5);
    let convoy_dest = neighbors(convoy_start)[0];
    let raid_hex = neighbors(convoy_dest)[1];

    set_units(
        &mut state,
        vec![
            unit(100, 0, offset_to_axial(1, 1)),
            unit(200, 1, offset_to_axial(10, 10)),
            unit(201, 1, raid_hex),
        ],
    );
    state.population.insert(Population {
        public_id: 0,
        hex: raid_hex,
        owner: 1,
        count: 10,
        role: Role::Idle,
        training: 0.0,
    });
    state.next_pop_id = 1;
    state.cell_at_mut(raid_hex).unwrap().terrain_value = 0.0;
    state.cell_at_mut(raid_hex).unwrap().material_value = 0.0;
    state.cell_at_mut(raid_hex).unwrap().stockpile_owner = Some(1);
    // Settlement so the hex has territory support and raided food doesn't decay this tick.
    state.settlements.insert(Settlement {
        public_id: 0,
        hex: raid_hex,
        owner: 1,
        settlement_type: SettlementType::Village,
    });
    state.next_settlement_id = 1;
    state.convoys.insert(Convoy {
        public_id: 0,
        owner: 0,
        pos: convoy_start,
        origin: convoy_start,
        destination: convoy_dest,
        cargo_type: super::state::CargoType::Food,
        cargo_amount: 11.0,
        capacity: 20.0,
        speed: 1.0,
        move_cooldown: 0,
        returning: false,
        route: vec![],
    });

    run_ticks(&mut state, &mut [], 1);

    assert!(state.convoys.is_empty());
    let raid_cell = state.cell_at(raid_hex).unwrap();
    assert_eq!(raid_cell.stockpile_owner, Some(1));
    assert!(
        (raid_cell.food_stockpile - 11.0).abs() < 0.001,
        "expected 11 food at raid hex, got {}",
        raid_cell.food_stockpile
    );
}

#[test]
fn road_unit_arrives_faster_than_bare_terrain_unit() {
    let mut state = blank_state(14, 14, 1);
    let road_start = offset_to_axial(3, 3);
    let plain_start = offset_to_axial(8, 3);
    let road_dest = offset_to_axial(3, 8);
    let plain_dest = offset_to_axial(8, 8);

    set_units(
        &mut state,
        vec![
            unit(100, 0, offset_to_axial(0, 0)),
            unit(101, 0, road_start),
            unit(102, 0, plain_start),
        ],
    );

    for col in 3..=8 {
        let road_hex = offset_to_axial(3, col);
        state.cell_at_mut(road_hex).unwrap().road_level = 3;
        state.cell_at_mut(road_hex).unwrap().stockpile_owner = Some(0);
        let plain_hex = offset_to_axial(8, col);
        state.cell_at_mut(plain_hex).unwrap().stockpile_owner = Some(0);
    }

    unit_mut(&mut state, 101).destination = Some(road_dest);
    unit_mut(&mut state, 102).destination = Some(plain_dest);

    let mut road_arrival = None;
    let mut plain_arrival = None;
    for _ in 0..30 {
        runner::advance_game_tick(&mut state, &mut []);
        if road_arrival.is_none() && unit_ref(&state, 101).pos == road_dest {
            road_arrival = Some(state.tick);
        }
        if plain_arrival.is_none() && unit_ref(&state, 102).pos == plain_dest {
            plain_arrival = Some(state.tick);
        }
        if road_arrival.is_some() && plain_arrival.is_some() {
            break;
        }
    }

    let road_arrival = road_arrival.expect("road unit should arrive");
    let plain_arrival = plain_arrival.expect("plain unit should arrive");
    assert!(road_arrival < plain_arrival);
}

#[test]
fn higher_ground_reduces_incoming_combat_damage() {
    let mut state = blank_state(10, 10, 2);
    let high_hex = offset_to_axial(4, 4);
    let low_hex = neighbors(high_hex)[1];

    set_units(
        &mut state,
        vec![
            unit(100, 0, offset_to_axial(0, 0)),
            unit(200, 1, offset_to_axial(9, 9)),
            unit(101, 0, high_hex),
            unit(201, 1, low_hex),
        ],
    );
    state.cell_at_mut(high_hex).unwrap().height = 1.0;
    state.cell_at_mut(low_hex).unwrap().height = 0.0;

    let high_key = state.unit_key_by_public_id(101).unwrap();
    let low_key = state.unit_key_by_public_id(201).unwrap();
    assert!(combat::engage(&mut state, high_key, low_key));
    let high_before = unit_ref(&state, 101).strength;
    let low_before = unit_ref(&state, 201).strength;
    sim::tick(&mut state);
    let high_after = unit_ref(&state, 101).strength;
    let low_after = unit_ref(&state, 201).strength;

    assert!(high_before - high_after < low_before - low_after);
}

#[test]
fn uphill_movement_has_longer_cooldown_than_flat_movement() {
    let mut state = blank_state(10, 10, 1);
    let uphill_from = offset_to_axial(3, 3);
    let uphill_to = neighbors(uphill_from)[1];
    let flat_from = offset_to_axial(6, 3);
    let flat_to = neighbors(flat_from)[1];

    set_units(
        &mut state,
        vec![
            unit(100, 0, offset_to_axial(0, 0)),
            unit(101, 0, uphill_from),
            unit(102, 0, flat_from),
        ],
    );
    unit_mut(&mut state, 101).destination = Some(uphill_to);
    unit_mut(&mut state, 102).destination = Some(flat_to);

    state.cell_at_mut(uphill_from).unwrap().height = 0.0;
    state.cell_at_mut(uphill_to).unwrap().height = 1.0;
    state.cell_at_mut(flat_from).unwrap().height = 0.5;
    state.cell_at_mut(flat_to).unwrap().height = 0.5;

    run_ticks(&mut state, &mut [], 1);

    let uphill_cooldown = unit_ref(&state, 101).move_cooldown;
    let flat_cooldown = unit_ref(&state, 102).move_cooldown;
    assert!(uphill_cooldown > flat_cooldown);
}

#[test]
fn population_growth_respects_carrying_capacity() {
    let mut state = blank_state(10, 10, 1);
    let home = offset_to_axial(4, 4);

    set_units(&mut state, vec![unit(100, 0, home)]);
    // Move the blank_state settlement to home so all population stays at home.
    for s in state.settlements.values_mut() {
        if s.owner == 0 {
            s.hex = home;
        }
    }
    // Clear population inserted by blank_state and start fresh at home.
    state.population.clear();
    state.next_pop_id = 0;

    let home_cell = state.cell_at_mut(home).unwrap();
    home_cell.stockpile_owner = Some(0);
    home_cell.food_stockpile = 20.0;
    home_cell.terrain_value = 1.0;
    home_cell.water_access = 1.0;

    state.population.insert(Population {
        public_id: 0,
        hex: home,
        owner: 0,
        count: 20,
        role: Role::Farmer,
        training: 0.0,
    });
    state.next_pop_id = 1;

    run_ticks(&mut state, &mut [], 200);

    let total_pop: u16 = state.population.values().map(|p| p.count).sum();
    let capacity = 20.0
        + state.cell_at(home).unwrap().terrain_value * 20.0
        + state.cell_at(home).unwrap().water_access * 12.0;
    assert!(total_pop > 20);
    // Population at home hex must not exceed carrying capacity.
    // Population may migrate to adjacent owned hexes — that is valid engine behavior.
    let home_pop: u16 = state
        .population
        .values()
        .filter(|p| p.hex == home)
        .map(|p| p.count)
        .sum();
    assert!(home_pop as f32 <= capacity + 1.0);
}

#[test]
fn starving_units_die_and_are_removed_from_state() {
    let mut state = blank_state(12, 12, 1);
    let home_hex = offset_to_axial(2, 2);
    let starving_hex = offset_to_axial(8, 8);
    let starving_dest = offset_to_axial(8, 9);

    set_units(
        &mut state,
        vec![unit(100, 0, home_hex), unit(101, 0, starving_hex)],
    );
    unit_mut(&mut state, 101).destination = Some(starving_dest);
    state.cell_at_mut(home_hex).unwrap().stockpile_owner = Some(0);
    state.cell_at_mut(home_hex).unwrap().food_stockpile = 50.0;
    state.cell_at_mut(starving_hex).unwrap().stockpile_owner = Some(0);
    state.cell_at_mut(starving_hex).unwrap().food_stockpile = 0.0;
    state.cell_at_mut(starving_hex).unwrap().terrain_value = 0.0;
    state.cell_at_mut(starving_dest).unwrap().terrain_value = 0.0;

    let mut weakened = false;
    for _ in 0..260 {
        runner::advance_game_tick(&mut state, &mut []);
        if let Some(unit) = state.unit_by_public_id(101) {
            if unit.strength < 100.0 {
                weakened = true;
            }
        } else {
            break;
        }
    }

    assert!(weakened, "starving unit should lose strength before dying");
    assert!(state.unit_by_public_id(101).is_none());
    assert!(state.players[0].alive);
}

#[test]
fn fog_of_war_hides_distant_enemy_units_and_stockpiles() {
    let mut state = blank_state(20, 20, 2);
    let friendly_hex = offset_to_axial(2, 2);
    let enemy_hex = offset_to_axial(15, 15);

    set_units(
        &mut state,
        vec![
            unit(100, 0, friendly_hex),
            unit(200, 1, enemy_hex),
            unit(201, 1, enemy_hex),
        ],
    );
    let enemy_cell = state.cell_at_mut(enemy_hex).unwrap();
    enemy_cell.stockpile_owner = Some(1);
    enemy_cell.food_stockpile = 18.0;
    enemy_cell.material_stockpile = 9.0;

    let obs = observation::observe(&mut state, 0);
    assert_eq!(obs.own_units.len(), 1);
    assert!(obs.visible_enemies.is_empty());

    let (row, col) = super::hex::axial_to_offset(enemy_hex);
    let idx = row as usize * state.width + col as usize;
    assert!(!obs.visible[idx]);
    assert_eq!(obs.food_stockpiles[idx], 0.0);
    assert_eq!(obs.material_stockpiles[idx], 0.0);
    assert_eq!(obs.stockpile_owner[idx], None);
}

#[test]
fn replay_reconstruction_matches_final_state() {
    let config = MapConfig {
        width: 20,
        height: 20,
        num_players: 2,
        seed: 99,
    };
    let mut agents: Vec<Box<dyn Agent>> =
        vec![Box::new(SpreadAgent::new()), Box::new(SpreadAgent::new())];
    let (replay, final_state) = replay::record_game_with_final_state(&config, &mut agents, 120, 5);
    let reconstructed = replay::reconstruct_state(&replay, replay.frames.last().unwrap());

    assert_eq!(reconstructed.tick, final_state.tick);
    assert_eq!(reconstructed.units.len(), final_state.units.len());
    assert_eq!(reconstructed.population.len(), final_state.population.len());
    assert_eq!(reconstructed.convoys.len(), final_state.convoys.len());
    assert_eq!(reconstructed.players.len(), final_state.players.len());

    for (lhs, rhs) in reconstructed.grid.iter().zip(final_state.grid.iter()) {
        assert!((lhs.food_stockpile - rhs.food_stockpile).abs() < 0.001);
        assert!((lhs.material_stockpile - rhs.material_stockpile).abs() < 0.001);
        assert_eq!(lhs.stockpile_owner, rhs.stockpile_owner);
        assert_eq!(lhs.road_level, rhs.road_level);
        assert_eq!(lhs.has_depot, rhs.has_depot);
    }
}

#[test]
fn convoys_have_non_empty_routes_after_creation() {
    let config = MapConfig {
        width: 20,
        height: 20,
        num_players: 2,
        seed: 42,
    };
    let mut agents: Vec<Box<dyn Agent>> =
        vec![Box::new(SpreadAgent::new()), Box::new(SpreadAgent::new())];

    // Run long enough for the city AI to dispatch some convoys.
    let mut state = super::mapgen::generate(&config);
    for _ in 0..200 {
        runner::advance_game_tick(&mut state, &mut agents);
        // Once a convoy exists, verify it has a non-empty route (or is already at destination).
        for convoy in state.convoys.values() {
            if convoy.pos != convoy.destination {
                assert!(
                    !convoy.route.is_empty(),
                    "convoy {} has empty route but has not reached destination (pos={:?}, dest={:?})",
                    convoy.public_id,
                    convoy.pos,
                    convoy.destination
                );
            }
        }
    }
}

#[test]
#[ignore = "long-running soak test"]
fn spread_vs_spread_converges_without_invalid_final_states() {
    let mut decisive = 0;

    for seed in 100..110 {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed,
        });
        let mut agents: Vec<Box<dyn Agent>> =
            vec![Box::new(SpreadAgent::new()), Box::new(SpreadAgent::new())];
        let winner = runner::run_game(&mut state, &mut agents, 1000);
        if winner.is_some() {
            decisive += 1;
        }
        assert_entities_in_bounds(&state);
        assert_no_orphaned_engagements(&state);
        assert_no_negative_stockpiles(&state);
        assert_player_totals_match_owned_cells(&state);
    }

    assert!(
        decisive >= 8,
        "expected at least 8 decisive games, got {decisive}"
    );
}
