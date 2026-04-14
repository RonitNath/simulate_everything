use rayon::prelude::*;
use serde::Serialize;
use simulate_everything_engine::v3::{
    agent::{
        AgentOutput, EntityTask, LayeredAgent, OperationalCommand, StrategyLayer, TacticalCommand,
        validate_operational, validate_tactical,
    },
    damage_table::DamageEstimateTable,
    mapgen,
    operations::SharedOperationsLayer,
    state::{GameState, Role},
    strategy::{SpreadStrategy, StrikerStrategy, TurtleStrategy},
    tactical::SharedTacticalLayer,
    weapon::AttackState,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn main(args: &[String]) {
    let profile_mode = args.iter().any(|a| a == "--profile");
    let converge_mode = args.iter().any(|a| a == "--converge");
    let ascii_mode = args.iter().any(|a| a == "--ascii");
    let top_n: usize = flag_value(args, "--top")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let seeds = flag_value(args, "--seeds")
        .map(parse_seed_range)
        .unwrap_or_else(|| (0..149).collect());

    let max_ticks: u64 = flag_value(args, "--ticks")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);

    let (w, h) = flag_value(args, "--size")
        .map(parse_size)
        .unwrap_or((30, 30));

    let num_players: u8 = flag_value(args, "--players")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);

    let target_ci: f64 = flag_value(args, "--ci")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.04);

    let max_seeds: u64 = flag_value(args, "--max-seeds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);

    let batch_size: u64 = flag_value(args, "--batch")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    // Parse matchups.
    let matchups: Vec<Vec<&str>> = if let Some(m) = flag_value(args, "--matchups") {
        if m == "all" {
            let names = v3_agent_names();
            let mut pairs = Vec::new();
            for i in 0..names.len() {
                for j in (i + 1)..names.len() {
                    pairs.push(vec![names[i], names[j]]);
                }
            }
            if pairs.is_empty() {
                pairs.push(vec![names[0], names[0]]);
            }
            pairs
        } else {
            m.split(';')
                .map(|pair| pair.split(',').collect::<Vec<&str>>())
                .collect()
        }
    } else if let Some(agents_str) = flag_value(args, "--agents") {
        vec![agents_str.split(',').collect()]
    } else {
        // Default: alternate spread/striker.
        let agents = ["spread", "striker"];
        let default: Vec<&str> = (0..num_players as usize)
            .map(|i| agents[i % agents.len()])
            .collect();
        vec![default]
    };

    // Validate agent names and matchup length.
    for matchup in &matchups {
        if matchup.len() != num_players as usize {
            eprintln!(
                "error: matchup has {} agents but --players is {}. Each matchup must have exactly --players agents.",
                matchup.len(),
                num_players,
            );
            std::process::exit(1);
        }
        for name in matchup {
            if !v3_agent_names().contains(name) {
                eprintln!(
                    "error: unknown v3 agent '{}'. known agents: {:?}",
                    name,
                    v3_agent_names()
                );
                std::process::exit(1);
            }
        }
    }

    // Ctrl+C handler.
    let interrupted = Arc::new(AtomicBool::new(false));
    let int_flag = interrupted.clone();
    ctrlc::set_handler(move || {
        if int_flag.load(Ordering::Relaxed) {
            eprintln!("\nForce quit.");
            std::process::exit(130);
        }
        eprintln!("\nInterrupted — finishing current batch...");
        int_flag.store(true, Ordering::Relaxed);
    })
    .expect("failed to set Ctrl+C handler");

    if ascii_mode {
        let matchup = &matchups[0];
        let seed = seeds[0];
        run_ascii_game(seed, matchup, max_ticks, (w, h), num_players);
        return;
    }

    if profile_mode {
        let matchup = &matchups[0];
        let seed = seeds[0];
        run_profile_game(seed, matchup, max_ticks, (w, h), num_players);
        return;
    }

    if converge_mode {
        run_convergence(
            &matchups,
            max_ticks,
            (w, h),
            num_players,
            target_ci,
            max_seeds,
            batch_size,
            top_n,
            &interrupted,
        );
    } else {
        run_fixed_seeds(
            &matchups,
            &seeds,
            max_ticks,
            (w, h),
            num_players,
            top_n,
            &interrupted,
        );
    }
}

// ---------------------------------------------------------------------------
// Agent registry
// ---------------------------------------------------------------------------

fn v3_agent_names() -> &'static [&'static str] {
    &["spread", "striker", "turtle"]
}

fn make_agent(name: &str, player: u8) -> LayeredAgent {
    let damage_table = DamageEstimateTable::from_physics();
    let strategy: Box<dyn StrategyLayer> = match name {
        "spread" => Box::new(SpreadStrategy::new()),
        "striker" => Box::new(StrikerStrategy::new()),
        "turtle" => Box::new(TurtleStrategy::new()),
        _ => panic!("unknown v3 agent: {name}"),
    };
    let ops = Box::new(SharedOperationsLayer::new());
    let tactical = Box::new(SharedTacticalLayer::new(damage_table));
    LayeredAgent::new(strategy, ops, tactical, player, 50, 5)
}

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
struct V3GameResult {
    seed: u64,
    matchup: String,
    agents: Vec<String>,
    winner: Option<String>,
    winner_idx: Option<u8>,
    ticks: u64,
    draw: bool,
    compute_total_us: Vec<u64>,
    compute_mean_us: Vec<f64>,
    compute_max_us: Vec<u64>,
    final_entities: Vec<usize>,
    final_soldiers: Vec<usize>,
    final_territory: Vec<usize>,
    total_deaths: usize,
    interest_score: f64,
    interest_tags: Vec<String>,
    snapshots: Vec<V3Snapshot>,
}

#[derive(Serialize, Clone)]
struct V3Snapshot {
    tick: u64,
    entities: Vec<usize>,
    soldiers: Vec<usize>,
    territory: Vec<usize>,
    alive: Vec<bool>,
}

#[derive(Serialize)]
struct V3TickProfile {
    tick: u64,
    agent_us: Vec<u64>,
    sim_us: u64,
    entities: Vec<usize>,
    soldiers: Vec<usize>,
    territory: Vec<usize>,
    deaths: usize,
    impacts: usize,
}

struct V3MatchupStats {
    agents: Vec<String>,
    wins: Vec<u32>,
    draws: u32,
    total_compute_us: Vec<u64>,
    total_max_us: Vec<u64>,
    total_ticks: u64,
    games_played: u32,
    results: Vec<V3GameResult>,
}

impl V3MatchupStats {
    fn new(agents: &[&str]) -> Self {
        let n = agents.len();
        Self {
            agents: agents.iter().map(|s| s.to_string()).collect(),
            wins: vec![0; n],
            draws: 0,
            total_compute_us: vec![0; n],
            total_max_us: vec![0; n],
            total_ticks: 0,
            games_played: 0,
            results: Vec::new(),
        }
    }

    fn add(&mut self, result: V3GameResult) {
        if result.draw {
            self.draws += 1;
        } else if let Some(wi) = result.winner_idx {
            self.wins[wi as usize] += 1;
        }
        for (i, &ct) in result.compute_total_us.iter().enumerate() {
            self.total_compute_us[i] += ct;
            if result.compute_max_us[i] > self.total_max_us[i] {
                self.total_max_us[i] = result.compute_max_us[i];
            }
        }
        self.total_ticks += result.ticks;
        self.games_played += 1;
        self.results.push(result);
    }

    fn wilson_ci(&self) -> (f64, f64) {
        wilson_ci(self.wins[0] as u64, self.games_played as u64)
    }
}

// ---------------------------------------------------------------------------
// Fixed-seed mode
// ---------------------------------------------------------------------------

fn run_fixed_seeds(
    matchups: &[Vec<&str>],
    seeds: &[u64],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
    top_n: usize,
    interrupted: &Arc<AtomicBool>,
) {
    let total_start = Instant::now();

    for matchup in matchups {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }

        let matchup_key = matchup.join("-vs-");
        eprintln!(
            "\n--- {} ({} seeds, {}x{}, max_ticks={}, {} players) ---",
            matchup_key,
            seeds.len(),
            w,
            h,
            max_ticks,
            num_players,
        );

        let results: Vec<V3GameResult> = seeds
            .par_iter()
            .map(|&seed| run_bench_game(seed, matchup, max_ticks, (w, h), num_players))
            .collect();

        let mut stats = V3MatchupStats::new(matchup);
        for result in results {
            println!("{}", serde_json::to_string(&result).unwrap());
            stats.add(result);
        }

        print_matchup_summary(&stats);
        print_interesting_games(&stats.results, top_n);
    }

    eprintln!("\nTotal wall time: {:.2?}", total_start.elapsed());
}

// ---------------------------------------------------------------------------
// Convergence mode
// ---------------------------------------------------------------------------

fn run_convergence(
    matchups: &[Vec<&str>],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
    target_ci: f64,
    max_seeds: u64,
    batch_size: u64,
    top_n: usize,
    interrupted: &Arc<AtomicBool>,
) {
    let total_start = Instant::now();

    for matchup in matchups {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }

        let matchup_key = matchup.join("-vs-");
        eprintln!(
            "\n--- {} (converge to CI<{:.1}%, max={} seeds, batch={}) ---",
            matchup_key,
            target_ci * 100.0,
            max_seeds,
            batch_size,
        );

        let mut stats = V3MatchupStats::new(matchup);
        let mut next_seed: u64 = 0;

        loop {
            if interrupted.load(Ordering::Relaxed) {
                eprintln!("  interrupted at {} games", stats.games_played);
                break;
            }

            if next_seed >= max_seeds {
                eprintln!("  reached max seeds ({}) without converging", max_seeds);
                break;
            }

            let batch_end = (next_seed + batch_size).min(max_seeds);
            let batch_seeds: Vec<u64> = (next_seed..batch_end).collect();

            let results: Vec<V3GameResult> = batch_seeds
                .par_iter()
                .map(|&seed| run_bench_game(seed, matchup, max_ticks, (w, h), num_players))
                .collect();

            for result in results {
                println!("{}", serde_json::to_string(&result).unwrap());
                stats.add(result);
            }

            next_seed = batch_end;

            let (lo, hi) = stats.wilson_ci();
            let width = hi - lo;
            let win_pct = if stats.games_played > 0 {
                stats.wins[0] as f64 / stats.games_played as f64 * 100.0
            } else {
                0.0
            };

            eprintln!(
                "  {} games: {} wins {:.1}% [CI: {:.1}%–{:.1}%, width={:.2}%]",
                stats.games_played,
                stats.agents[0],
                win_pct,
                lo * 100.0,
                hi * 100.0,
                width * 100.0,
            );

            if width <= target_ci && stats.games_played >= 30 {
                eprintln!("  converged!");
                break;
            }
        }

        print_matchup_summary(&stats);
        print_interesting_games(&stats.results, top_n);
    }

    eprintln!("\nTotal wall time: {:.2?}", total_start.elapsed());
}

// ---------------------------------------------------------------------------
// Profile mode
// ---------------------------------------------------------------------------

fn run_profile_game(
    seed: u64,
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
) {
    let mut state = mapgen::generate(w, h, num_players, seed);
    let mut agents: Vec<LayeredAgent> = agent_names
        .iter()
        .enumerate()
        .map(|(i, &name)| make_agent(name, i as u8))
        .collect();

    let np = num_players as usize;

    while state.tick < max_ticks {
        if let Some(_winner) = is_game_over(&state) {
            break;
        }

        // Time each agent.
        let mut agent_us = vec![0u64; np];
        let outputs: Vec<AgentOutput> = agents
            .iter_mut()
            .enumerate()
            .map(|(i, agent)| {
                let t0 = Instant::now();
                let out = agent.tick(&state);
                agent_us[i] = t0.elapsed().as_micros() as u64;
                out
            })
            .collect();

        for output in &outputs {
            apply_commands(&mut state, output);
        }

        let t0 = Instant::now();
        let result = simulate_everything_engine::v3::sim::tick(&mut state, 1.0);
        let sim_us = t0.elapsed().as_micros() as u64;

        let tp = V3TickProfile {
            tick: state.tick,
            agent_us,
            sim_us,
            entities: player_entity_counts(&state, num_players),
            soldiers: player_soldier_counts(&state, num_players),
            territory: estimate_territory(&state, num_players),
            deaths: result.deaths,
            impacts: result.impacts,
        };
        println!("{}", serde_json::to_string(&tp).unwrap());
    }

    let winner = is_game_over(&state);
    eprintln!("---");
    eprintln!(
        "Game ended at tick {} — winner: {}",
        state.tick,
        winner
            .map(|w| format!("P{} ({})", w, agent_names[w as usize]))
            .unwrap_or_else(|| "draw".into()),
    );
}

// ---------------------------------------------------------------------------
// ASCII mode
// ---------------------------------------------------------------------------

fn run_ascii_game(
    seed: u64,
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
) {
    let mut state = mapgen::generate(w, h, num_players, seed);
    let mut agents: Vec<LayeredAgent> = agent_names
        .iter()
        .enumerate()
        .map(|(i, &name)| make_agent(name, i as u8))
        .collect();

    while state.tick < max_ticks {
        if is_game_over(&state).is_some() {
            break;
        }

        let outputs: Vec<AgentOutput> = agents.iter_mut().map(|a| a.tick(&state)).collect();
        for output in &outputs {
            apply_commands(&mut state, output);
        }
        simulate_everything_engine::v3::sim::tick(&mut state, 1.0);
    }

    print_ascii(&state);

    let winner = is_game_over(&state);
    eprintln!(
        "Game ended at tick {} — winner: {}",
        state.tick,
        winner
            .map(|w| format!("P{} ({})", w, agent_names[w as usize]))
            .unwrap_or_else(|| "draw".into()),
    );
    for (i, name) in agent_names.iter().enumerate() {
        let ents = player_entity_counts(&state, num_players);
        let solds = player_soldier_counts(&state, num_players);
        eprintln!(
            "  P{} ({}): {} persons, {} soldiers",
            i, name, ents[i], solds[i]
        );
    }
}

// ---------------------------------------------------------------------------
// Core bench game
// ---------------------------------------------------------------------------

fn run_bench_game(
    seed: u64,
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
) -> V3GameResult {
    let mut state = mapgen::generate(w, h, num_players, seed);
    let mut agents: Vec<LayeredAgent> = agent_names
        .iter()
        .enumerate()
        .map(|(i, &name)| make_agent(name, i as u8))
        .collect();

    let ids: Vec<String> = agent_names.iter().map(|s| s.to_string()).collect();
    let matchup_key = agent_names.join("-vs-");
    let np = num_players as usize;

    let mut compute_total = vec![0u64; np];
    let mut compute_max = vec![0u64; np];
    let mut tick_count = 0u64;
    let mut total_deaths = 0usize;

    let mut snapshots: Vec<V3Snapshot> = Vec::new();
    let mut prev_leader: Option<u8> = None;
    let mut lead_changes: u32 = 0;

    // Count starting entities for heavy-casualties check.
    let starting_entities: usize = player_entity_counts(&state, num_players).iter().sum();

    let snap_interval: u64 = 100;

    while state.tick < max_ticks {
        if is_game_over(&state).is_some() {
            break;
        }

        // Run each agent and apply commands.
        let outputs: Vec<AgentOutput> = agents
            .iter_mut()
            .enumerate()
            .map(|(i, agent)| {
                let t0 = Instant::now();
                let out = agent.tick(&state);
                let elapsed = t0.elapsed().as_micros() as u64;
                compute_total[i] += elapsed;
                if elapsed > compute_max[i] {
                    compute_max[i] = elapsed;
                }
                out
            })
            .collect();

        for output in &outputs {
            apply_commands(&mut state, output);
        }

        let tick_result = simulate_everything_engine::v3::sim::tick(&mut state, 1.0);
        total_deaths += tick_result.deaths;
        tick_count += 1;

        if state.tick % snap_interval == 0 || is_game_over(&state).is_some() {
            let snap = take_snapshot(&state, state.tick, num_players);

            // Track lead changes by entity count.
            let leader = snap
                .entities
                .iter()
                .enumerate()
                .filter(|(i, _)| snap.alive[*i])
                .max_by_key(|(_, e)| *e)
                .map(|(i, _)| i as u8);

            if let (Some(prev), Some(curr)) = (prev_leader, leader) {
                if prev != curr {
                    lead_changes += 1;
                }
            }
            prev_leader = leader;

            snapshots.push(snap);
        }
    }

    let winner_idx = is_game_over(&state);
    let winner = winner_idx.map(|wi| ids[wi as usize].clone());

    let compute_mean: Vec<f64> = compute_total
        .iter()
        .map(|&t| {
            if tick_count > 0 {
                t as f64 / tick_count as f64
            } else {
                0.0
            }
        })
        .collect();

    let (interest_score, interest_tags) = score_game(
        winner_idx,
        state.tick,
        lead_changes,
        &snapshots,
        max_ticks,
        total_deaths,
        starting_entities,
    );

    let final_entities = player_entity_counts(&state, num_players);
    let final_soldiers = player_soldier_counts(&state, num_players);
    let final_territory = estimate_territory(&state, num_players);

    V3GameResult {
        seed,
        matchup: matchup_key,
        agents: ids,
        winner,
        winner_idx,
        ticks: state.tick,
        draw: winner_idx.is_none(),
        compute_total_us: compute_total,
        compute_mean_us: compute_mean,
        compute_max_us: compute_max,
        final_entities,
        final_soldiers,
        final_territory,
        total_deaths,
        interest_score,
        interest_tags,
        snapshots,
    }
}

// ---------------------------------------------------------------------------
// Command application
// ---------------------------------------------------------------------------

fn apply_commands(state: &mut GameState, output: &AgentOutput) {
    for cmd in &output.operational_commands {
        if !validate_operational(cmd, state) {
            continue;
        }
        match cmd {
            OperationalCommand::RouteStack { stack, waypoints } => {
                // Find the stack and set waypoints on all mobile members.
                let members: Vec<_> = state
                    .stacks
                    .iter()
                    .find(|s| s.id == *stack)
                    .map(|s| s.members.to_vec())
                    .unwrap_or_default();
                for member_key in members {
                    if let Some(entity) = state.entities.get_mut(member_key) {
                        if let Some(mobile) = &mut entity.mobile {
                            mobile.waypoints = waypoints.clone();
                        }
                    }
                }
            }
            OperationalCommand::AssignTask { entity, task } => match task {
                EntityTask::Patrol { waypoints } => {
                    if let Some(e) = state.entities.get_mut(*entity) {
                        if let Some(mobile) = &mut e.mobile {
                            mobile.waypoints = waypoints.clone();
                        }
                    }
                }
                _ => {
                    // Other tasks (Farm, Build, Craft, Garrison, Train, Idle) not yet wired.
                }
            },
            _ => {
                // FormStack, DisbandStack, ProduceEquipment, EquipEntity,
                // EstablishSupplyRoute, FoundSettlement — stub.
            }
        }
    }

    for cmd in &output.tactical_commands {
        if !validate_tactical(cmd, state) {
            continue;
        }
        match cmd {
            TacticalCommand::Attack { attacker, target } => {
                if let Some(entity) = state.entities.get_mut(*attacker) {
                    if let Some(combatant) = &mut entity.combatant {
                        if combatant.attack.is_none() && combatant.cooldown.is_none() {
                            if let Some(eq) = &entity.equipment {
                                if let Some(weapon_key) = eq.weapon {
                                    combatant.attack = Some(AttackState::new(*target, weapon_key));
                                }
                            }
                        }
                    }
                }
            }
            TacticalCommand::SetFacing { entity, angle } => {
                if let Some(e) = state.entities.get_mut(*entity) {
                    if let Some(combatant) = &mut e.combatant {
                        combatant.facing = *angle;
                    }
                }
            }
            TacticalCommand::Retreat { entity, toward } => {
                if let Some(e) = state.entities.get_mut(*entity) {
                    if let Some(mobile) = &mut e.mobile {
                        mobile.waypoints = vec![*toward];
                    }
                }
            }
            TacticalCommand::Hold { entity } => {
                if let Some(e) = state.entities.get_mut(*entity) {
                    if let Some(mobile) = &mut e.mobile {
                        mobile.waypoints.clear();
                    }
                }
            }
            TacticalCommand::SetFormation { stack, formation } => {
                if let Some(s) = state.stacks.iter_mut().find(|s| s.id == *stack) {
                    s.formation = *formation;
                }
            }
            TacticalCommand::Block { .. } => {
                // Block not yet wired to a component.
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Game termination
// ---------------------------------------------------------------------------

/// Returns the winning player index if only one player has living persons,
/// or None if the game is still ongoing or it's a draw.
fn is_game_over(state: &GameState) -> Option<u8> {
    let alive = players_alive(state);
    let alive_players: Vec<u8> = alive
        .iter()
        .enumerate()
        .filter(|(_, a)| **a)
        .map(|(i, _)| i as u8)
        .collect();

    if alive_players.len() == 1 {
        Some(alive_players[0])
    } else if alive_players.is_empty() {
        // All players eliminated — call it a draw (return None).
        None
    } else {
        None
    }
}

fn players_alive(state: &GameState) -> Vec<bool> {
    let mut alive = vec![false; state.num_players as usize];
    for entity in state.entities.values() {
        if entity.person.is_none() {
            continue;
        }
        let is_dead = entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false);
        if is_dead {
            continue;
        }
        if let Some(owner) = entity.owner {
            if (owner as usize) < alive.len() {
                alive[owner as usize] = true;
            }
        }
    }
    alive
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

fn player_entity_counts(state: &GameState, num_players: u8) -> Vec<usize> {
    let mut counts = vec![0usize; num_players as usize];
    for entity in state.entities.values() {
        if entity.person.is_none() {
            continue;
        }
        let is_dead = entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false);
        if is_dead {
            continue;
        }
        if let Some(owner) = entity.owner {
            if (owner as usize) < counts.len() {
                counts[owner as usize] += 1;
            }
        }
    }
    counts
}

fn player_soldier_counts(state: &GameState, num_players: u8) -> Vec<usize> {
    let mut counts = vec![0usize; num_players as usize];
    for entity in state.entities.values() {
        if entity.person.is_none() {
            continue;
        }
        let is_soldier = entity
            .person
            .as_ref()
            .map(|p| p.role == Role::Soldier)
            .unwrap_or(false);
        if !is_soldier {
            continue;
        }
        let is_dead = entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false);
        if is_dead {
            continue;
        }
        if let Some(owner) = entity.owner {
            if (owner as usize) < counts.len() {
                counts[owner as usize] += 1;
            }
        }
    }
    counts
}

fn estimate_territory(state: &GameState, num_players: u8) -> Vec<usize> {
    use std::collections::HashMap;
    let mut hex_owners: HashMap<(i32, i32), Vec<u8>> = HashMap::new();
    for entity in state.entities.values() {
        if entity.person.is_none() {
            continue;
        }
        let is_dead = entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false);
        if is_dead {
            continue;
        }
        if let Some(owner) = entity.owner {
            if let Some(hex) = entity.hex {
                hex_owners.entry((hex.q, hex.r)).or_default().push(owner);
            }
        }
    }
    let mut territory = vec![0usize; num_players as usize];
    for (_, owners) in &hex_owners {
        let mut counts = vec![0u32; num_players as usize];
        for &o in owners {
            if (o as usize) < counts.len() {
                counts[o as usize] += 1;
            }
        }
        if let Some((player, &count)) = counts.iter().enumerate().max_by_key(|(_, c)| **c) {
            if count > 0 {
                territory[player] += 1;
            }
        }
    }
    territory
}

fn take_snapshot(state: &GameState, tick: u64, num_players: u8) -> V3Snapshot {
    V3Snapshot {
        tick,
        entities: player_entity_counts(state, num_players),
        soldiers: player_soldier_counts(state, num_players),
        territory: estimate_territory(state, num_players),
        alive: players_alive(state),
    }
}

// ---------------------------------------------------------------------------
// Interestingness scoring
// ---------------------------------------------------------------------------

fn score_game(
    winner_idx: Option<u8>,
    ticks: u64,
    _lead_changes: u32,
    snapshots: &[V3Snapshot],
    max_ticks: u64,
    total_deaths: usize,
    starting_entities: usize,
) -> (f64, Vec<String>) {
    let mut score: f64 = 0.0;
    let mut tags: Vec<String> = Vec::new();

    // Lead changes after tick 200.
    let late_lead_changes = count_late_lead_changes(snapshots, 200);
    if late_lead_changes >= 3 {
        score += 40.0;
        tags.push(format!("seesaw({})", late_lead_changes));
    } else if late_lead_changes >= 1 {
        score += 20.0 * late_lead_changes as f64;
        tags.push(format!("lead-flip({})", late_lead_changes));
    }

    // Comeback: winner was behind late.
    if let Some(wi) = winner_idx {
        if winner_was_behind_late(wi, snapshots, max_ticks / 4) {
            score += 30.0;
            tags.push("comeback".into());
        }
    }

    // Draw.
    if winner_idx.is_none() && ticks >= max_ticks {
        score += 25.0;
        tags.push("draw".into());
    }

    // Long game.
    if winner_idx.is_some() && ticks > max_ticks / 2 {
        let long_score = (ticks as f64 / max_ticks as f64 - 0.5) * 30.0;
        score += long_score;
        tags.push(format!("long(T{})", ticks));
    }

    // Blitz.
    if ticks < max_ticks / 6 {
        score += 15.0;
        tags.push(format!("blitz(T{})", ticks));
    }

    // Closeness at 75% mark (by entity count).
    let q3_idx = snapshots.len() * 3 / 4;
    if q3_idx > 0 && q3_idx < snapshots.len() {
        let snap = &snapshots[q3_idx];
        let entities_alive: Vec<usize> = snap
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| snap.alive[*i])
            .map(|(_, e)| *e)
            .collect();
        if entities_alive.len() >= 2 {
            let max_e = *entities_alive.iter().max().unwrap() as f64;
            let min_e = (*entities_alive.iter().min().unwrap()).max(1) as f64;
            let ratio = max_e / min_e;
            if ratio < 1.3 {
                score += 25.0;
                tags.push(format!("neck-and-neck({:.2}x)", ratio));
            } else if ratio < 1.8 {
                score += 10.0;
                tags.push(format!("competitive({:.2}x)", ratio));
            }
        }
    }

    // V3-specific: heavy casualties.
    if starting_entities > 0 {
        let casualty_rate = total_deaths as f64 / starting_entities as f64;
        if casualty_rate > 0.5 {
            score += 20.0;
            tags.push(format!("heavy-casualties({:.0}%)", casualty_rate * 100.0));
        }
    }

    // Upset: non-first player wins.
    if let Some(wi) = winner_idx {
        if wi != 0 {
            score += 5.0;
            tags.push("upset".into());
        }
    }

    (score, tags)
}

fn count_late_lead_changes(snapshots: &[V3Snapshot], after_tick: u64) -> u32 {
    let mut changes = 0u32;
    let mut prev_leader: Option<usize> = None;

    for snap in snapshots {
        let leader = snap
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| snap.alive[*i])
            .max_by_key(|(_, e)| *e)
            .map(|(i, _)| i);

        if snap.tick > after_tick {
            if let (Some(prev), Some(curr)) = (prev_leader, leader) {
                if prev != curr {
                    changes += 1;
                }
            }
        }
        prev_leader = leader;
    }
    changes
}

fn winner_was_behind_late(winner: u8, snapshots: &[V3Snapshot], after_tick: u64) -> bool {
    for snap in snapshots {
        if snap.tick <= after_tick || !snap.alive[winner as usize] {
            continue;
        }
        let winner_entities = snap.entities[winner as usize];
        let any_ahead = snap
            .entities
            .iter()
            .enumerate()
            .any(|(i, &e)| i != winner as usize && snap.alive[i] && e > winner_entities);
        if any_ahead {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn print_matchup_summary(stats: &V3MatchupStats) {
    eprintln!(
        "\n  {:>16} {:>5} {:>6} {:>10} {:>10} {:>10}",
        "agent", "wins", "win%", "avg_us/t", "max_us", "total_ms"
    );
    for i in 0..stats.agents.len() {
        let pct = if stats.games_played > 0 {
            stats.wins[i] as f64 / stats.games_played as f64 * 100.0
        } else {
            0.0
        };
        let avg_per_tick = if stats.total_ticks > 0 {
            stats.total_compute_us[i] as f64 / stats.total_ticks as f64
        } else {
            0.0
        };
        eprintln!(
            "  {:>16} {:>5} {:>5.1}% {:>10.1} {:>10} {:>10.1}",
            stats.agents[i],
            stats.wins[i],
            pct,
            avg_per_tick,
            stats.total_max_us[i],
            stats.total_compute_us[i] as f64 / 1000.0,
        );
    }

    let (lo, hi) = stats.wilson_ci();
    eprintln!(
        "  {} win rate: {:.1}% [95% CI: {:.1}%–{:.1}%, width={:.2}%]",
        stats.agents[0],
        stats.wins[0] as f64 / stats.games_played.max(1) as f64 * 100.0,
        lo * 100.0,
        hi * 100.0,
        (hi - lo) * 100.0,
    );

    if stats.draws > 0 {
        eprintln!("  draws: {}", stats.draws);
    }
    eprintln!(
        "  total: {} ticks across {} games ({:.0} ticks/game avg)",
        stats.total_ticks,
        stats.games_played,
        stats.total_ticks as f64 / stats.games_played.max(1) as f64,
    );
}

fn print_interesting_games(results: &[V3GameResult], top_n: usize) {
    if results.is_empty() {
        return;
    }

    let mut ranked: Vec<&V3GameResult> = results.iter().collect();
    ranked.sort_by(|a, b| b.interest_score.partial_cmp(&a.interest_score).unwrap());

    eprintln!("\n  Top {} interesting games:", top_n.min(ranked.len()));
    eprintln!(
        "  {:>6} {:>6} {:>10} {:>12} {:>8}  {}",
        "seed", "ticks", "winner", "final_ents", "score", "tags"
    );
    for r in ranked.iter().take(top_n) {
        let winner_str = r.winner.as_deref().unwrap_or("draw");
        let ents_str = r
            .final_entities
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join("/");
        eprintln!(
            "  {:>6} {:>6} {:>10} {:>12} {:>8.1}  {}",
            r.seed,
            r.ticks,
            winner_str,
            ents_str,
            r.interest_score,
            r.interest_tags.join(", "),
        );
    }
}

fn print_ascii(state: &GameState) {
    let colors = ['0', '1', '2', '3', '4', '5', '6', '7'];
    for r in 0..state.map_height as i32 {
        // Hex grid stagger: indent odd rows.
        if r % 2 == 1 {
            eprint!("  ");
        }
        for q in 0..state.map_width as i32 {
            use simulate_everything_engine::v2::hex::offset_to_axial;
            let hex = offset_to_axial(r, q);
            let entities: Vec<_> = state
                .spatial_index
                .entities_at(hex)
                .iter()
                .filter_map(|&k| state.entities.get(k))
                .filter(|e| e.person.is_some())
                .collect();

            if entities.is_empty() {
                eprint!(" .  ");
            } else {
                // Find dominant owner.
                let mut owner_counts = [0u32; 8];
                for e in &entities {
                    if let Some(o) = e.owner {
                        if (o as usize) < owner_counts.len() {
                            owner_counts[o as usize] += 1;
                        }
                    }
                }
                let dominant = owner_counts
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, c)| **c)
                    .map(|(i, _)| i)
                    .unwrap_or(0);

                let soldiers = entities
                    .iter()
                    .filter(|e| {
                        e.person
                            .as_ref()
                            .map(|p| p.role == Role::Soldier)
                            .unwrap_or(false)
                    })
                    .count();
                let typ = if soldiers > entities.len() / 2 {
                    'S'
                } else {
                    'C'
                };

                eprint!(
                    "{}{}{:>2}",
                    colors[dominant % colors.len()],
                    typ,
                    entities.len()
                );
            }
        }
        eprintln!();
    }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
}

fn parse_seed_range(s: &str) -> Vec<u64> {
    if let Some((a, b)) = s.split_once('-') {
        let start: u64 = a.parse().expect("bad seed range start");
        let end: u64 = b.parse().expect("bad seed range end");
        (start..=end).collect()
    } else {
        s.split(',')
            .map(|x| x.trim().parse::<u64>().expect("bad seed"))
            .collect()
    }
}

fn parse_size(s: &str) -> (usize, usize) {
    let (w, h) = s.split_once('x').expect("size must be WxH");
    (
        w.parse().expect("bad width"),
        h.parse().expect("bad height"),
    )
}

fn wilson_ci(successes: u64, total: u64) -> (f64, f64) {
    if total == 0 {
        return (0.0, 1.0);
    }
    let n = total as f64;
    let p = successes as f64 / n;
    let z = 1.96;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let margin = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
    ((center - margin).max(0.0), (center + margin).min(1.0))
}
