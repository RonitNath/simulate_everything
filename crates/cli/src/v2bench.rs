use rayon::prelude::*;
use serde::Serialize;
use simulate_everything_engine::v2::{
    AGENT_POLL_INTERVAL,
    agent::{self as v2_agent, Agent as V2Agent},
    directive,
    mapgen::{self as v2_mapgen, MapConfig as V2MapConfig},
    observation::{self, ObservationSession},
    sim,
};
use std::collections::HashMap;
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
    let postmortem_mode = args.iter().any(|a| a == "--postmortem");
    let explain_mode = args.iter().any(|a| a == "--explain");
    let diagnose_mode = args.iter().any(|a| a == "--diagnose");
    let top_n: usize = flag_value(args, "--top")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let seeds = flag_value(args, "--seeds")
        .map(parse_seed_range)
        .unwrap_or_else(|| (0..149).collect());

    let max_ticks: u64 = flag_value(args, "--ticks")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);

    let (w, h) = flag_value(args, "--size")
        .map(parse_size)
        .unwrap_or((40, 40));

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
            let names: Vec<&str> = v2_agent::builtin_agent_names().iter().copied().collect();
            let mut pairs = Vec::new();
            for i in 0..names.len() {
                for j in (i + 1)..names.len() {
                    pairs.push(vec![names[i], names[j]]);
                }
            }
            if pairs.is_empty() {
                // Only one agent — mirror match.
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
        // Default: alternate spread/striker to fill player slots.
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
            if v2_agent::agent_by_name(name).is_none() {
                eprintln!(
                    "error: unknown v2 agent '{}'. known agents: {:?}",
                    name,
                    v2_agent::builtin_agent_names()
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

    // --snapshots 0,25,50,100,200,500 dumps ASCII at those ticks
    let snapshot_ticks: Option<Vec<u64>> = flag_value(args, "--snapshots").map(|s| {
        s.split(',')
            .map(|x| x.trim().parse::<u64>().expect("bad snapshot tick"))
            .collect()
    });

    if diagnose_mode {
        let matchup = &matchups[0];
        let check_seeds: Vec<u64> = if seeds.len() > 10 {
            seeds[..10].to_vec()
        } else {
            seeds.clone()
        };
        run_diagnose(&check_seeds, matchup, max_ticks, (w, h), num_players);
        return;
    }

    if ascii_mode || snapshot_ticks.is_some() {
        let matchup = &matchups[0];
        let seed = seeds[0];
        run_ascii_game(
            seed,
            matchup,
            max_ticks,
            (w, h),
            num_players,
            snapshot_ticks.as_deref(),
        );
        return;
    }

    if postmortem_mode {
        let matchup = &matchups[0];
        let seed = seeds[0];
        run_postmortem_game(seed, matchup, max_ticks, (w, h), num_players);
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
            explain_mode,
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
            explain_mode,
        );
    }
}

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
struct V2GameResult {
    seed: u64,
    matchup: String,
    agents: Vec<String>,
    winner: Option<String>,
    winner_idx: Option<u8>,
    ticks: u64,
    draw: bool,
    /// Per-agent total compute time in microseconds (across all agent polls).
    compute_total_us: Vec<u64>,
    /// Per-agent mean compute time per poll in microseconds.
    compute_mean_us: Vec<f64>,
    /// Per-agent max single-poll compute time in microseconds.
    compute_max_us: Vec<u64>,
    /// Per-agent final unit count.
    final_units: Vec<usize>,
    /// Per-agent final total strength.
    final_strength: Vec<f32>,
    /// Per-agent final food.
    final_food: Vec<f32>,
    /// Per-agent final material.
    final_material: Vec<f32>,
    /// Per-agent final hex count (stockpile_owner).
    final_hexes: Vec<usize>,
    /// Per-agent final total population.
    final_population: Vec<u16>,
    /// Per-agent final farmer count.
    final_farmers: Vec<u16>,
    /// Per-agent final settlement count.
    final_settlements: Vec<usize>,
    interest_score: f64,
    interest_tags: Vec<String>,
    snapshots: Vec<V2Snapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loss_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loss_explanation: Option<String>,
}

#[derive(Serialize, Clone)]
struct V2Snapshot {
    tick: u64,
    units: Vec<usize>,
    strength: Vec<f32>,
    food: Vec<f32>,
    material: Vec<f32>,
    hexes: Vec<usize>,
    population: Vec<u16>,
    farmers: Vec<u16>,
    settlements: Vec<usize>,
    alive: Vec<bool>,
}

#[derive(Serialize)]
struct V2TickProfile {
    tick: u64,
    compute_us: Vec<u64>,
    units: Vec<usize>,
    strength: Vec<f32>,
    food: Vec<f32>,
    material: Vec<f32>,
    hexes: Vec<usize>,
    population: Vec<u16>,
    farmers: Vec<u16>,
    settlements: Vec<usize>,
}

struct V2MatchupStats {
    agents: Vec<String>,
    wins: Vec<u32>,
    draws: u32,
    total_compute_us: Vec<u64>,
    total_max_us: Vec<u64>,
    total_ticks: u64,
    games_played: u32,
    results: Vec<V2GameResult>,
}

impl V2MatchupStats {
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

    fn add(&mut self, result: V2GameResult) {
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
    explain: bool,
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

        let results: Vec<V2GameResult> = seeds
            .par_iter()
            .map(|&seed| run_bench_game(seed, matchup, max_ticks, (w, h), num_players, explain))
            .collect();

        let mut stats = V2MatchupStats::new(matchup);
        for result in results {
            println!("{}", serde_json::to_string(&result).unwrap());
            stats.add(result);
        }

        print_matchup_summary(&stats);
        print_interesting_games(&stats.results, top_n);
        if explain {
            print_loss_patterns(&stats.results);
        }
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
    explain: bool,
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

        let mut stats = V2MatchupStats::new(matchup);
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

            let results: Vec<V2GameResult> = batch_seeds
                .par_iter()
                .map(|&seed| run_bench_game(seed, matchup, max_ticks, (w, h), num_players, explain))
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
        if explain {
            print_loss_patterns(&stats.results);
        }
    }

    eprintln!("\nTotal wall time: {:.2?}", total_start.elapsed());
}

// ---------------------------------------------------------------------------
// Diagnose mode: 4 automated behavioral health checks
// ---------------------------------------------------------------------------

fn run_diagnose(
    seeds: &[u64],
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
) {
    use simulate_everything_engine::v2::{gamelog::GameLog, runner};

    eprintln!(
        "\n=== BEHAVIORAL HEALTH CHECK: {} ({} seeds, {}x{}) ===\n",
        agent_names.join(" vs "),
        seeds.len(),
        w,
        h,
    );

    let mut spatial_scores: Vec<f64> = Vec::new();
    let mut retreat_scores: Vec<f64> = Vec::new();
    let mut concentration_scores: Vec<f64> = Vec::new();
    let mut production_ticks: Vec<u64> = Vec::new();

    for &seed in seeds {
        let mut state = v2_mapgen::generate(&V2MapConfig {
            width: w,
            height: h,
            num_players,
            seed,
        });
        state.game_log = Some(GameLog::new());

        let mut agents: Vec<Box<dyn V2Agent>> = agent_names
            .iter()
            .map(|name| v2_agent::agent_by_name(name).unwrap())
            .collect();

        let tick_limit = sim::timeout_limit(max_ticks);
        runner::run_loop(&mut state, &mut agents, tick_limit, |_| {});

        if let Some(log) = state.game_log.take() {
            if let Some(s) = check_spatial_diversity(&log, num_players) {
                spatial_scores.push(s);
            }
            if let Some(s) = check_retreat_rate(&log) {
                retreat_scores.push(s);
            }
            if let Some(s) = check_force_concentration(&log) {
                concentration_scores.push(s);
            }
            check_production_speed(&log, num_players, &mut production_ticks);
        }
    }

    let avg = |v: &[f64]| -> Option<f64> {
        if v.is_empty() {
            None
        } else {
            Some(v.iter().sum::<f64>() / v.len() as f64)
        }
    };
    let avg_tick = |v: &[u64]| -> Option<f64> {
        if v.is_empty() {
            None
        } else {
            Some(v.iter().sum::<u64>() as f64 / v.len() as f64)
        }
    };

    let spatial_avg = avg(&spatial_scores);
    let retreat_avg = avg(&retreat_scores);
    let concentration_avg = avg(&concentration_scores);
    let production_avg = avg_tick(&production_ticks);

    eprintln!("--- CHECK 1: Spatial Diversity (target >= 4.0 distinct hexes) ---");
    match spatial_avg {
        Some(v) => {
            let status = if v >= 4.0 { "PASS" } else { "FAIL" };
            eprintln!("  avg distinct hexes: {:.2}  [{}]", v, status);
        }
        None => eprintln!("  no data"),
    }

    eprintln!("\n--- CHECK 2: Retreat Rate (target >= 30% survival when critical) ---");
    match retreat_avg {
        Some(v) => {
            let status = if v >= 30.0 { "PASS" } else { "FAIL" };
            eprintln!("  survival rate when critical: {:.1}%  [{}]", v, status);
        }
        None => eprintln!("  no critical engagements observed  [PASS] (rout prevents critical state)"),
    }

    eprintln!("\n--- CHECK 3: Force Concentration (target >= 2.0 friendlies near attacker) ---");
    match concentration_avg {
        Some(v) => {
            let status = if v >= 2.0 { "PASS" } else { "FAIL" };
            eprintln!(
                "  avg friendlies near attacker at engagement: {:.2}  [{}]",
                v, status
            );
        }
        None => eprintln!("  no engagements observed"),
    }

    eprintln!("\n--- CHECK 4: Production Speed (target <= 30 ticks to first unit) ---");
    match production_avg {
        Some(v) => {
            let status = if v <= 30.0 { "PASS" } else { "FAIL" };
            eprintln!("  avg tick of first unit produced: {:.1}  [{}]", v, status);
        }
        None => eprintln!("  no units produced observed"),
    }

    eprintln!();
}

/// Check 1: avg distinct hexes occupied by non-general units at sample ticks 50,100,150,200.
fn check_spatial_diversity(
    log: &simulate_everything_engine::v2::gamelog::GameLog,
    num_players: u8,
) -> Option<f64> {
    use std::collections::HashSet;

    let sample_ticks = [50u64, 100, 150, 200];
    let mut total_distinct: u64 = 0;
    let mut sample_count: u64 = 0;

    for &tick in &sample_ticks {
        // Find the closest recorded tick within +/- 10.
        let positions_at_tick: Vec<_> = log
            .unit_positions
            .iter()
            .filter(|s| s.tick >= tick.saturating_sub(10) && s.tick <= tick + 10)
            .collect();

        if positions_at_tick.is_empty() {
            continue;
        }

        for pid in 0..num_players {
            let hexes: HashSet<(i32, i32)> = positions_at_tick
                .iter()
                .filter(|s| s.player == pid)
                .map(|s| (s.q, s.r))
                .collect();
            if !hexes.is_empty() {
                total_distinct += hexes.len() as u64;
                sample_count += 1;
            }
        }
    }

    if sample_count == 0 {
        None
    } else {
        Some(total_distinct as f64 / sample_count as f64)
    }
}

/// Check 2: of units that reached <=30 strength while engaged, what % survived.
fn check_retreat_rate(log: &simulate_everything_engine::v2::gamelog::GameLog) -> Option<f64> {
    use simulate_everything_engine::v2::gamelog::GameEvent;

    // Find all unit_ids that were killed.
    let killed_ids: std::collections::HashSet<u32> = log
        .events
        .iter()
        .filter_map(|e| {
            if let GameEvent::UnitKilled {
                unit_id,
                ..
            } = e
            {
                Some(*unit_id)
            } else {
                None
            }
        })
        .collect();

    // Find units that hit critical strength while engaged.
    let mut critical_unit_ids: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for sample in &log.unit_positions {
        if sample.engaged && sample.strength <= 30.0 {
            critical_unit_ids.insert(sample.unit_id);
        }
    }

    if critical_unit_ids.is_empty() {
        return None;
    }

    let survived = critical_unit_ids
        .iter()
        .filter(|id| !killed_ids.contains(id))
        .count();
    let total = critical_unit_ids.len();
    Some(survived as f64 / total as f64 * 100.0)
}

/// Check 3: at each EngagementCreated, count friendly non-general units within distance 2.
fn check_force_concentration(
    log: &simulate_everything_engine::v2::gamelog::GameLog,
) -> Option<f64> {
    use simulate_everything_engine::v2::{
        gamelog::GameEvent,
        hex::{self, Axial},
    };

    let mut total_nearby: u64 = 0;
    let mut engagement_count: u64 = 0;

    for event in &log.events {
        let GameEvent::EngagementCreated {
            tick,
            attacker,
            attacker_owner,
            ..
        } = event
        else {
            continue;
        };

        // Find the nearest unit_positions snapshot to this tick.
        let snap_tick = (tick / 10) * 10;
        let attacker_pos = log
            .unit_positions
            .iter()
            .filter(|s| s.tick == snap_tick && s.unit_id == *attacker)
            .map(|s| Axial::new(s.q, s.r))
            .next();

        let Some(attacker_hex) = attacker_pos else {
            continue;
        };

        let nearby = log
            .unit_positions
            .iter()
            .filter(|s| {
                s.tick == snap_tick
                    && s.player == *attacker_owner
                    && s.unit_id != *attacker
                    && hex::distance(Axial::new(s.q, s.r), attacker_hex) <= 2
            })
            .count();

        total_nearby += nearby as u64;
        engagement_count += 1;
    }

    if engagement_count == 0 {
        None
    } else {
        Some(total_nearby as f64 / engagement_count as f64)
    }
}

/// Check 4: tick of first UnitProduced per player; appends to out_ticks.
fn check_production_speed(
    log: &simulate_everything_engine::v2::gamelog::GameLog,
    num_players: u8,
    out_ticks: &mut Vec<u64>,
) {
    use simulate_everything_engine::v2::gamelog::GameEvent;

    for pid in 0..num_players {
        let first = log.events.iter().find_map(|e| {
            if let GameEvent::UnitProduced { tick, player, .. } = e {
                if *player == pid { Some(*tick) } else { None }
            } else {
                None
            }
        });
        if let Some(t) = first {
            out_ticks.push(t);
        }
    }
}

// ---------------------------------------------------------------------------
// Postmortem mode (single game, full analysis)
// ---------------------------------------------------------------------------

fn run_postmortem_game(
    seed: u64,
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
) {
    use simulate_everything_engine::v2::{gamelog::GameLog, runner};

    let mut state = v2_mapgen::generate(&V2MapConfig {
        width: w,
        height: h,
        num_players,
        seed,
    });
    state.game_log = Some(GameLog::new());

    let mut agents: Vec<Box<dyn V2Agent>> = agent_names
        .iter()
        .map(|name| v2_agent::agent_by_name(name).unwrap())
        .collect();

    let tick_limit = sim::timeout_limit(max_ticks);
    runner::run_loop(&mut state, &mut agents, tick_limit, |_| {});

    let winner = sim::winner_at_limit(&state, max_ticks);
    let timed_out = sim::reached_timeout(&state, tick_limit);
    let names: Vec<String> = agent_names.iter().map(|s| s.to_string()).collect();

    if let Some(log) = state.game_log.take() {
        let summary = log.summarize(&names, winner, state.tick, timed_out);
        println!("{}", summary.render());
    }
}

// ---------------------------------------------------------------------------
// Profile mode (single game, per-poll output)
// ---------------------------------------------------------------------------

fn run_profile_game(
    seed: u64,
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
) {
    let mut state = v2_mapgen::generate(&V2MapConfig {
        width: w,
        height: h,
        num_players,
        seed,
    });

    let mut agents: Vec<Box<dyn V2Agent>> = agent_names
        .iter()
        .map(|name| v2_agent::agent_by_name(name).unwrap())
        .collect();
    let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
    for (pid, agent) in agents.iter_mut().enumerate() {
        let init = observation::initial_observation(&state, pid as u8);
        agent.reset();
        agent.init(&init);
    }

    let np = num_players as usize;

    let tick_limit = sim::timeout_limit(max_ticks);
    while state.tick < tick_limit && !sim::is_over(&state) {
        if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
            let mut poll_us = vec![0u64; np];
            for (pid, agent) in agents.iter_mut().enumerate() {
                let p = pid as u8;
                if !state.players.iter().any(|pl| pl.id == p && pl.alive) {
                    continue;
                }
                let delta = observation::observe_delta(&mut state, p, &mut session);
                let t0 = Instant::now();
                let directives = agent.act(&delta);
                poll_us[pid] = t0.elapsed().as_micros() as u64;
                directive::apply_directives(&mut state, p, &directives);
            }
            state.clear_dirty_hexes();

            let tp = V2TickProfile {
                tick: state.tick,
                compute_us: poll_us,
                units: player_unit_counts(&state, num_players),
                strength: player_total_strength(&state, num_players),
                food: player_food(&state, num_players),
                material: player_material(&state, num_players),
                hexes: player_hex_counts(&state, num_players),
                population: player_population(&state, num_players),
                farmers: player_farmers(&state, num_players),
                settlements: player_settlements(&state, num_players),
            };
            println!("{}", serde_json::to_string(&tp).unwrap());
        }

        sim::tick(&mut state);
    }

    let winner = sim::winner_at_limit(&state, max_ticks);
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
// ASCII mode (single game, final state)
// ---------------------------------------------------------------------------

fn run_ascii_game(
    seed: u64,
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
    snapshot_ticks: Option<&[u64]>,
) {
    use simulate_everything_engine::v2::ascii;

    let mut state = v2_mapgen::generate(&V2MapConfig {
        width: w,
        height: h,
        num_players,
        seed,
    });

    let mut agents: Vec<Box<dyn V2Agent>> = agent_names
        .iter()
        .map(|name| v2_agent::agent_by_name(name).unwrap())
        .collect();
    let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
    for (pid, agent) in agents.iter_mut().enumerate() {
        let init = observation::initial_observation(&state, pid as u8);
        agent.reset();
        agent.init(&init);
    }

    let tick_limit = sim::timeout_limit(max_ticks);
    while state.tick < tick_limit && !sim::is_over(&state) {
        // Render snapshot before this tick's agent poll.
        if let Some(ticks) = snapshot_ticks {
            if ticks.contains(&state.tick) {
                println!("{}", ascii::render_state(&state));
                // Also print per-player unit details.
                for u in state.units.values() {
                    let engaged = if u.engagements.is_empty() {
                        ""
                    } else {
                        " ENGAGED"
                    };
                    let dest = u
                        .destination
                        .map(|d| format!(" -> ({},{})", d.q, d.r))
                        .unwrap_or_default();
                    eprintln!(
                        "  P{} unit {} str={:.0} at ({},{}){}{}",
                        u.owner, u.public_id, u.strength, u.pos.q, u.pos.r, dest, engaged,
                    );
                }
                eprintln!();
            }
        }

        if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
            for (pid, agent) in agents.iter_mut().enumerate() {
                let p = pid as u8;
                if !state.players.iter().any(|pl| pl.id == p && pl.alive) {
                    continue;
                }
                let delta = observation::observe_delta(&mut state, p, &mut session);
                let directives = agent.act(&delta);
                directive::apply_directives(&mut state, p, &directives);
            }
            state.clear_dirty_hexes();
        }
        sim::tick(&mut state);
    }

    // Always render final state.
    println!("{}", ascii::render_state(&state));

    let winner = sim::winner_at_limit(&state, max_ticks);
    eprintln!("---");
    eprintln!(
        "Game ended at tick {} — winner: {}",
        state.tick,
        winner
            .map(|w| format!("P{} ({})", w, agent_names[w as usize]))
            .unwrap_or_else(|| "draw".into()),
    );
    for (i, name) in agent_names.iter().enumerate() {
        let p = i as u8;
        let alive = state.players.iter().any(|pl| pl.id == p && pl.alive);
        let units = state.units.values().filter(|u| u.owner == p).count();
        let strength: f32 = state
            .units
            .values()
            .filter(|u| u.owner == p)
            .map(|u| u.strength)
            .sum();
        let pl = state.players.iter().find(|pl| pl.id == p);
        let food = pl.map(|pl| pl.food).unwrap_or(0.0);
        let material = pl.map(|pl| pl.material).unwrap_or(0.0);
        let hexes = state
            .grid
            .iter()
            .filter(|c| c.stockpile_owner == Some(p))
            .count();
        eprintln!(
            "  P{} ({}): units={}, str={:.0}, food={:.1}, mat={:.1}, hexes={}{}",
            i,
            name,
            units,
            strength,
            food,
            material,
            hexes,
            if !alive { " [eliminated]" } else { "" },
        );
    }
}

// ---------------------------------------------------------------------------
// Core game runner
// ---------------------------------------------------------------------------

fn run_bench_game(
    seed: u64,
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
    log_enabled: bool,
) -> V2GameResult {
    use simulate_everything_engine::v2::gamelog::{self, GameLog};

    let mut state = v2_mapgen::generate(&V2MapConfig {
        width: w,
        height: h,
        num_players,
        seed,
    });

    if log_enabled {
        state.game_log = Some(GameLog::new());
    }

    let mut agents: Vec<Box<dyn V2Agent>> = agent_names
        .iter()
        .map(|name| v2_agent::agent_by_name(name).unwrap())
        .collect();
    let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
    for (pid, agent) in agents.iter_mut().enumerate() {
        let init = observation::initial_observation(&state, pid as u8);
        agent.reset();
        agent.init(&init);
    }

    let ids: Vec<String> = agents.iter().map(|a| a.name().to_string()).collect();
    let matchup_key = agent_names.join("-vs-");
    let np = num_players as usize;

    let mut compute_total = vec![0u64; np];
    let mut compute_max = vec![0u64; np];
    let mut poll_count = 0u64;

    let mut snapshots: Vec<V2Snapshot> = Vec::new();
    let mut prev_leader: Option<u8> = None;
    let mut lead_changes: u32 = 0;

    // Snapshot interval: every 50 ticks.
    let snap_interval: u64 = 50;

    let tick_limit = sim::timeout_limit(max_ticks);
    while state.tick < tick_limit && !sim::is_over(&state) {
        if state.tick % AGENT_POLL_INTERVAL as u64 == 0 {
            for (pid, agent) in agents.iter_mut().enumerate() {
                let p = pid as u8;
                if !state.players.iter().any(|pl| pl.id == p && pl.alive) {
                    continue;
                }
                let delta = observation::observe_delta(&mut state, p, &mut session);
                let t0 = Instant::now();
                let directives = agent.act(&delta);
                let elapsed = t0.elapsed().as_micros() as u64;
                compute_total[pid] += elapsed;
                if elapsed > compute_max[pid] {
                    compute_max[pid] = elapsed;
                }
                directive::apply_directives(&mut state, p, &directives);
            }
            state.clear_dirty_hexes();
            poll_count += 1;
        }

        sim::tick(&mut state);

        if state.tick % snap_interval == 0 || sim::is_over(&state) {
            let snap = take_snapshot(&state, num_players);

            // Track lead changes by unit count.
            let leader = snap
                .units
                .iter()
                .enumerate()
                .filter(|(i, _)| snap.alive[*i])
                .max_by_key(|(_, u)| **u)
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

    let winner_idx = sim::winner_at_limit(&state, max_ticks);
    let winner = winner_idx.map(|wi| ids[wi as usize].clone());

    let compute_mean: Vec<f64> = compute_total
        .iter()
        .map(|&t| {
            if poll_count > 0 {
                t as f64 / poll_count as f64
            } else {
                0.0
            }
        })
        .collect();

    let (interest_score, interest_tags) =
        score_game(winner_idx, state.tick, lead_changes, &snapshots, max_ticks);

    let (loss_category, loss_explanation) = if let Some(log) = state.game_log.take() {
        let timed_out = sim::reached_timeout(&state, sim::timeout_limit(max_ticks));
        let summary = log.summarize(&ids, winner_idx, state.tick, timed_out);
        let loser = winner_idx.and_then(|w| (0..num_players).find(|&i| i != w));
        if let Some(l) = loser {
            (
                Some(gamelog::categorize_loss(&summary, l).to_string()),
                Some(summary.one_liner()),
            )
        } else {
            (None, Some(summary.one_liner()))
        }
    } else {
        (None, None)
    };

    V2GameResult {
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
        final_units: player_unit_counts(&state, num_players),
        final_strength: player_total_strength(&state, num_players),
        final_food: player_food(&state, num_players),
        final_material: player_material(&state, num_players),
        final_hexes: player_hex_counts(&state, num_players),
        final_population: player_population(&state, num_players),
        final_farmers: player_farmers(&state, num_players),
        final_settlements: player_settlements(&state, num_players),
        interest_score,
        interest_tags,
        snapshots,
        loss_category,
        loss_explanation,
    }
}

// ---------------------------------------------------------------------------
// Per-player stat helpers
// ---------------------------------------------------------------------------

fn player_unit_counts(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> Vec<usize> {
    (0..num_players)
        .map(|p| state.units.values().filter(|u| u.owner == p).count())
        .collect()
}

fn player_total_strength(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> Vec<f32> {
    (0..num_players)
        .map(|p| {
            state
                .units
                .values()
                .filter(|u| u.owner == p)
                .map(|u| u.strength)
                .sum()
        })
        .collect()
}

fn player_food(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> Vec<f32> {
    (0..num_players)
        .map(|p| {
            state
                .players
                .iter()
                .find(|pl| pl.id == p)
                .map(|pl| pl.food)
                .unwrap_or(0.0)
        })
        .collect()
}

fn player_material(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> Vec<f32> {
    (0..num_players)
        .map(|p| {
            state
                .players
                .iter()
                .find(|pl| pl.id == p)
                .map(|pl| pl.material)
                .unwrap_or(0.0)
        })
        .collect()
}

fn player_hex_counts(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> Vec<usize> {
    (0..num_players)
        .map(|p| {
            state
                .grid
                .iter()
                .filter(|c| c.stockpile_owner == Some(p))
                .count()
        })
        .collect()
}

fn player_population(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> Vec<u16> {
    (0..num_players)
        .map(|p| {
            state
                .population
                .values()
                .filter(|pop| pop.owner == p)
                .map(|pop| pop.count)
                .sum()
        })
        .collect()
}

fn player_farmers(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> Vec<u16> {
    use simulate_everything_engine::v2::state::Role;
    (0..num_players)
        .map(|p| {
            state
                .population
                .values()
                .filter(|pop| pop.owner == p && pop.role == Role::Farmer)
                .map(|pop| pop.count)
                .sum()
        })
        .collect()
}

fn player_settlements(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> Vec<usize> {
    use simulate_everything_engine::v2::hex::Axial;
    (0..num_players)
        .map(|p| {
            let mut seen: Vec<Axial> = Vec::new();
            for pop in state.population.values().filter(|pop| pop.owner == p) {
                if !seen.contains(&pop.hex) && state.is_settlement(p, pop.hex) {
                    seen.push(pop.hex);
                }
            }
            seen.len()
        })
        .collect()
}

fn take_snapshot(
    state: &simulate_everything_engine::v2::state::GameState,
    num_players: u8,
) -> V2Snapshot {
    V2Snapshot {
        tick: state.tick,
        units: player_unit_counts(state, num_players),
        strength: player_total_strength(state, num_players),
        food: player_food(state, num_players),
        material: player_material(state, num_players),
        hexes: player_hex_counts(state, num_players),
        population: player_population(state, num_players),
        farmers: player_farmers(state, num_players),
        settlements: player_settlements(state, num_players),
        alive: (0..num_players)
            .map(|p| state.players.iter().any(|pl| pl.id == p && pl.alive))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Interestingness scoring (adapted for V2)
// ---------------------------------------------------------------------------

fn score_game(
    winner_idx: Option<u8>,
    ticks: u64,
    _lead_changes: u32,
    snapshots: &[V2Snapshot],
    max_ticks: u64,
) -> (f64, Vec<String>) {
    let mut score: f64 = 0.0;
    let mut tags: Vec<String> = Vec::new();

    // Lead changes after tick 200 (past early expansion).
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

    // Closeness at 75% mark (by unit count).
    let q3_idx = snapshots.len() * 3 / 4;
    if q3_idx > 0 && q3_idx < snapshots.len() {
        let snap = &snapshots[q3_idx];
        let units_alive: Vec<usize> = snap
            .units
            .iter()
            .enumerate()
            .filter(|(i, _)| snap.alive[*i])
            .map(|(_, &u)| u)
            .collect();
        if units_alive.len() >= 2 {
            let max_u = *units_alive.iter().max().unwrap() as f64;
            let min_u = (*units_alive.iter().min().unwrap()).max(1) as f64;
            let ratio = max_u / min_u;
            if ratio < 1.3 {
                score += 25.0;
                tags.push(format!("neck-and-neck({:.2}x)", ratio));
            } else if ratio < 1.8 {
                score += 10.0;
                tags.push(format!("competitive({:.2}x)", ratio));
            }
        }
    }

    // Upset.
    if let Some(wi) = winner_idx {
        if wi != 0 {
            score += 5.0;
            tags.push("upset".into());
        }
    }

    (score, tags)
}

fn count_late_lead_changes(snapshots: &[V2Snapshot], after_tick: u64) -> u32 {
    let mut changes = 0u32;
    let mut prev_leader: Option<usize> = None;

    for snap in snapshots {
        let leader = snap
            .units
            .iter()
            .enumerate()
            .filter(|(i, _)| snap.alive[*i])
            .max_by_key(|(_, u)| **u)
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

fn winner_was_behind_late(winner: u8, snapshots: &[V2Snapshot], after_tick: u64) -> bool {
    for snap in snapshots {
        if snap.tick <= after_tick || !snap.alive[winner as usize] {
            continue;
        }
        let winner_units = snap.units[winner as usize];
        let any_ahead = snap
            .units
            .iter()
            .enumerate()
            .any(|(i, &u)| i != winner as usize && snap.alive[i] && u > winner_units);
        if any_ahead {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn print_matchup_summary(stats: &V2MatchupStats) {
    let polls_per_agent = if stats.total_ticks > 0 {
        stats.total_ticks / AGENT_POLL_INTERVAL as u64
    } else {
        1
    };

    eprintln!(
        "\n  {:>16} {:>5} {:>6} {:>10} {:>10} {:>10}",
        "agent", "wins", "win%", "avg_us/p", "max_us", "total_ms"
    );
    for i in 0..stats.agents.len() {
        let pct = if stats.games_played > 0 {
            stats.wins[i] as f64 / stats.games_played as f64 * 100.0
        } else {
            0.0
        };
        let avg_per_poll = if polls_per_agent > 0 {
            stats.total_compute_us[i] as f64 / polls_per_agent as f64
        } else {
            0.0
        };
        eprintln!(
            "  {:>16} {:>5} {:>5.1}% {:>10.1} {:>10} {:>10.1}",
            stats.agents[i],
            stats.wins[i],
            pct,
            avg_per_poll,
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

fn print_interesting_games(results: &[V2GameResult], top_n: usize) {
    if results.is_empty() {
        return;
    }

    let mut ranked: Vec<&V2GameResult> = results.iter().collect();
    ranked.sort_by(|a, b| b.interest_score.partial_cmp(&a.interest_score).unwrap());

    eprintln!("\n  Top {} interesting games:", top_n.min(ranked.len()));
    eprintln!(
        "  {:>6} {:>6} {:>8} {:>12} {:>12} {:>8}  {}",
        "seed", "ticks", "winner", "final_units", "final_hexes", "score", "tags"
    );
    for r in ranked.iter().take(top_n) {
        let winner_str = r.winner.as_deref().unwrap_or("draw");
        let units_str = r
            .final_units
            .iter()
            .map(|u| u.to_string())
            .collect::<Vec<_>>()
            .join("/");
        let hexes_str = r
            .final_hexes
            .iter()
            .map(|h| h.to_string())
            .collect::<Vec<_>>()
            .join("/");
        eprintln!(
            "  {:>6} {:>6} {:>8} {:>12} {:>12} {:>8.1}  {}",
            r.seed,
            r.ticks,
            winner_str,
            units_str,
            hexes_str,
            r.interest_score,
            r.interest_tags.join(", "),
        );
    }
}

fn print_loss_patterns(results: &[V2GameResult]) {
    if results.is_empty() {
        return;
    }

    // Collect loss categories
    let mut categories: HashMap<String, Vec<&V2GameResult>> = HashMap::new();
    for r in results {
        if let Some(ref cat) = r.loss_category {
            categories.entry(cat.clone()).or_default().push(r);
        }
    }

    if categories.is_empty() {
        return;
    }

    let total_losses: usize = categories.values().map(|v| v.len()).sum();
    eprintln!("\n  Loss patterns ({} losses examined):", total_losses);

    let mut sorted: Vec<_> = categories.iter().collect();
    sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    for (category, games) in &sorted {
        let pct = games.len() as f64 / total_losses as f64 * 100.0;
        eprintln!("  - {:>3} ({:>4.1}%): {}", games.len(), pct, category);
    }

    // Print a few example one-liners for the most common pattern
    if let Some((_, games)) = sorted.first() {
        let show = games.len().min(3);
        eprintln!("\n  Example losses:");
        for r in games.iter().take(show) {
            if let Some(ref expl) = r.loss_explanation {
                eprintln!("    seed={}: {}", r.seed, expl);
            }
        }
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
