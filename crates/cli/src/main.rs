use simulate_everything_engine::agent::{self, Agent};
use simulate_everything_engine::game::Game;
use simulate_everything_engine::mapgen::{self, MapConfig};
use rand::rngs::StdRng;
use rand::SeedableRng;
use rayon::prelude::*;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "bench") {
        bench_main(&args);
    } else {
        sim_main(&args);
    }
}

// ---------------------------------------------------------------------------
// Original simulation mode
// ---------------------------------------------------------------------------

fn sim_main(args: &[String]) {
    let ascii_mode = args.iter().any(|a| a == "--ascii");
    let positional: Vec<&str> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .collect();

    let seed: u64 = positional.first().and_then(|s| s.parse().ok()).unwrap_or(42);
    let num_players: u8 = positional
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let max_turns: u32 = positional
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    let mut rng = StdRng::seed_from_u64(seed);
    let config = MapConfig::for_players(num_players);

    eprintln!(
        "Generating {}x{} map for {} players (seed={})...",
        config.width, config.height, num_players, seed
    );

    let gen_start = Instant::now();
    let state = mapgen::generate(&config, &mut rng);
    let gen_time = gen_start.elapsed();
    eprintln!("Map generated in {:?}", gen_time);

    use simulate_everything_engine::agent::all_builtin_agents;
    use rand::seq::SliceRandom;

    let mut pool = all_builtin_agents();
    pool.shuffle(&mut rng);
    let mut agents: Vec<Box<dyn Agent>> = pool.into_iter().take(num_players as usize).collect();

    let agent_names: Vec<String> = agents.iter().map(|a| a.id()).collect();
    eprintln!("Players: {:?}", agent_names);

    let mut game = Game::with_seed(state, max_turns, seed);

    let sim_start = Instant::now();
    while !game.is_over() {
        let observations = game.observations();

        let mut orders = Vec::new();
        for (i, agent) in agents.iter_mut().enumerate() {
            let obs = &observations[i];
            let actions = agent.act(obs, &mut rng);
            orders.push((i as u8, actions));
        }

        game.step(&orders);
    }
    let sim_time = sim_start.elapsed();

    if ascii_mode {
        println!("{}", game.state);
    } else {
        for event in &game.events {
            println!("{}", serde_json::to_string(event).unwrap());
        }
    }

    let turns = game.state.turn;
    let tps = turns as f64 / sim_time.as_secs_f64();
    eprintln!("---");
    eprintln!(
        "Game over in {} turns ({:.1?}, {:.0} turns/sec)",
        turns, sim_time, tps
    );
    if let Some(winner) = game.state.winner {
        eprintln!(
            "Winner: player {} ({})",
            winner, agent_names[winner as usize]
        );
    } else {
        eprintln!("Draw (max turns reached)");
    }
    for p in 0..num_players {
        eprintln!(
            "  P{} ({}): land={}, armies={}{}",
            p,
            agent_names[p as usize],
            game.state.land_count(p),
            game.state.army_count(p),
            if !game.state.alive[p as usize] {
                " [eliminated]"
            } else {
                ""
            }
        );
    }
}

// ---------------------------------------------------------------------------
// Bench mode
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

/// Per-game result emitted as JSON line.
#[derive(Serialize, Clone)]
struct GameResult {
    seed: u64,
    matchup: String,
    agents: Vec<String>,
    winner: Option<String>,
    winner_idx: Option<u8>,
    turns: u32,
    draw: bool,
    /// Per-agent total compute time in microseconds.
    compute_total_us: Vec<u64>,
    /// Per-agent mean compute time per turn in microseconds.
    compute_mean_us: Vec<f64>,
    /// Per-agent max single-turn compute time in microseconds.
    compute_max_us: Vec<u64>,
    /// Per-agent final land count.
    final_land: Vec<usize>,
    /// Per-agent final army count.
    final_army: Vec<i32>,
    /// Game interestingness score (higher = more worth investigating).
    interest_score: f64,
    /// Breakdown of what made the game interesting.
    interest_tags: Vec<String>,
    /// Sampled snapshots at turn checkpoints (every 25 turns).
    snapshots: Vec<Snapshot>,
}

/// Lightweight state sample at a checkpoint turn.
#[derive(Serialize, Clone)]
struct Snapshot {
    turn: u32,
    land: Vec<usize>,
    army: Vec<i32>,
    alive: Vec<bool>,
}

/// Per-turn timing detail for --profile mode.
#[derive(Serialize)]
struct TurnProfile {
    turn: u32,
    compute_us: Vec<u64>,
    land: Vec<usize>,
    army: Vec<i32>,
}

/// Per-matchup aggregate stats.
struct MatchupStats {
    agents: Vec<String>,
    wins: Vec<u32>,
    draws: u32,
    total_compute_us: Vec<u64>,
    total_max_us: Vec<u64>,
    total_turns: u64,
    games_played: u32,
    results: Vec<GameResult>,
}

impl MatchupStats {
    fn new(agents: &[&str]) -> Self {
        let n = agents.len();
        Self {
            agents: agents.iter().map(|s| s.to_string()).collect(),
            wins: vec![0; n],
            draws: 0,
            total_compute_us: vec![0; n],
            total_max_us: vec![0; n],
            total_turns: 0,
            games_played: 0,
            results: Vec::new(),
        }
    }

    fn add(&mut self, result: GameResult) {
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
        self.total_turns += result.turns as u64;
        self.games_played += 1;
        self.results.push(result);
    }

    /// Wilson score 95% confidence interval for agent 0's win rate.
    fn wilson_ci(&self) -> (f64, f64) {
        wilson_ci(self.wins[0] as u64, self.games_played as u64)
    }

    /// CI width for agent 0.
    fn ci_width(&self) -> f64 {
        let (lo, hi) = self.wilson_ci();
        hi - lo
    }
}

/// Wilson score confidence interval for a binomial proportion.
/// Returns (lower, upper) bounds for the 95% CI.
fn wilson_ci(successes: u64, total: u64) -> (f64, f64) {
    if total == 0 {
        return (0.0, 1.0);
    }
    let n = total as f64;
    let p = successes as f64 / n;
    let z = 1.96; // 95% CI
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (p + z2 / (2.0 * n)) / denom;
    let margin = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denom;
    ((center - margin).max(0.0), (center + margin).min(1.0))
}

fn bench_main(args: &[String]) {
    let profile_mode = args.iter().any(|a| a == "--profile");
    let converge_mode = args.iter().any(|a| a == "--converge");
    let top_n: usize = flag_value(args, "--top")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let seeds = flag_value(args, "--seeds")
        .map(parse_seed_range)
        .unwrap_or_else(|| (100..=249).collect());

    let max_turns: u32 = flag_value(args, "--turns")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    let size = flag_value(args, "--size").map(parse_size);

    // Target CI width for convergence mode.
    let target_ci: f64 = flag_value(args, "--ci")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.04); // 4% width by default

    // Max seeds per matchup in convergence mode.
    let max_seeds: u64 = flag_value(args, "--max-seeds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(5000);

    // Batch size for convergence mode.
    let batch_size: u64 = flag_value(args, "--batch")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    // Parse matchups.
    // --agents pressure,swarm         => single matchup (legacy)
    // --matchups all                   => round-robin all pairs
    // --matchups p,s;p,e              => explicit matchup list
    let matchups: Vec<Vec<&str>> = if let Some(m) = flag_value(args, "--matchups") {
        if m == "all" {
            // Round-robin all pairs of non-random built-in agents.
            let names: Vec<&str> = agent::builtin_agent_names()
                .iter()
                .filter(|&&n| n != "random")
                .copied()
                .collect();
            let mut pairs = Vec::new();
            for i in 0..names.len() {
                for j in (i + 1)..names.len() {
                    pairs.push(vec![names[i], names[j]]);
                }
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
        vec![vec!["pressure", "swarm"]]
    };

    // Validate all agent names up front.
    for matchup in &matchups {
        for name in matchup {
            if agent::agent_by_name(name).is_none() {
                eprintln!(
                    "error: unknown agent '{}'. known agents: {:?}",
                    name,
                    agent::builtin_agent_names()
                );
                std::process::exit(1);
            }
        }
    }

    // Set up Ctrl+C handler.
    let interrupted = Arc::new(AtomicBool::new(false));
    let int_flag = interrupted.clone();
    ctrlc::set_handler(move || {
        // First Ctrl+C: signal graceful stop. Second: hard exit.
        if int_flag.load(Ordering::Relaxed) {
            eprintln!("\nForce quit.");
            std::process::exit(130);
        }
        eprintln!("\nInterrupted — finishing current batch...");
        int_flag.store(true, Ordering::Relaxed);
    })
    .expect("failed to set Ctrl+C handler");

    if profile_mode {
        // Profile mode: single seed, single matchup, per-turn output.
        let matchup = &matchups[0];
        let seed = seeds[0];
        run_profile_game(seed, matchup, max_turns, size, matchup.len() as u8);
        return;
    }

    if converge_mode {
        run_convergence(
            &matchups,
            max_turns,
            size,
            target_ci,
            max_seeds,
            batch_size,
            top_n,
            &interrupted,
        );
    } else {
        run_fixed_seeds(&matchups, &seeds, max_turns, size, top_n, &interrupted);
    }
}

// ---------------------------------------------------------------------------
// Fixed-seed mode (original behavior, now parallel + multi-matchup)
// ---------------------------------------------------------------------------

fn run_fixed_seeds(
    matchups: &[Vec<&str>],
    seeds: &[u64],
    max_turns: u32,
    size: Option<(usize, usize)>,
    top_n: usize,
    interrupted: &Arc<AtomicBool>,
) {
    let total_start = Instant::now();

    for matchup in matchups {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }

        let num_players = matchup.len() as u8;
        let (w, h) = size.unwrap_or_else(|| {
            let cfg = MapConfig::for_players(num_players);
            (cfg.width, cfg.height)
        });

        let matchup_key = matchup.join("-vs-");
        eprintln!(
            "\n--- {} ({} seeds, {}x{}, max_turns={}) ---",
            matchup_key,
            seeds.len(),
            w,
            h,
            max_turns,
        );

        // Run all seeds in parallel.
        let results: Vec<GameResult> = seeds
            .par_iter()
            .map(|&seed| run_bench_game(seed, matchup, max_turns, (w, h)))
            .collect();

        // Aggregate.
        let mut stats = MatchupStats::new(matchup);
        for result in results {
            // Emit per-game JSON to stdout.
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
    max_turns: u32,
    size: Option<(usize, usize)>,
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

        let num_players = matchup.len() as u8;
        let (w, h) = size.unwrap_or_else(|| {
            let cfg = MapConfig::for_players(num_players);
            (cfg.width, cfg.height)
        });

        let matchup_key = matchup.join("-vs-");
        eprintln!(
            "\n--- {} (converge to CI<{:.1}%, max={} seeds, batch={}) ---",
            matchup_key,
            target_ci * 100.0,
            max_seeds,
            batch_size,
        );

        let mut stats = MatchupStats::new(matchup);
        let mut next_seed: u64 = 0;

        loop {
            if interrupted.load(Ordering::Relaxed) {
                eprintln!("  interrupted at {} games", stats.games_played);
                break;
            }

            if next_seed >= max_seeds {
                eprintln!(
                    "  reached max seeds ({}) without converging",
                    max_seeds
                );
                break;
            }

            // Run a batch in parallel.
            let batch_end = (next_seed + batch_size).min(max_seeds);
            let batch_seeds: Vec<u64> = (next_seed..batch_end).collect();

            let results: Vec<GameResult> = batch_seeds
                .par_iter()
                .map(|&seed| run_bench_game(seed, matchup, max_turns, (w, h)))
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
// Profile mode (single game, per-turn output)
// ---------------------------------------------------------------------------

fn run_profile_game(
    seed: u64,
    agent_names: &[&str],
    max_turns: u32,
    size: Option<(usize, usize)>,
    num_players: u8,
) {
    let (w, h) = size.unwrap_or_else(|| {
        let cfg = MapConfig::for_players(num_players);
        (cfg.width, cfg.height)
    });

    let mut rng = StdRng::seed_from_u64(seed);
    let config = MapConfig::for_size(w, h, num_players);
    let state = mapgen::generate(&config, &mut rng);
    let mut game = Game::with_seed(state, max_turns, seed);

    let mut agents: Vec<Box<dyn Agent>> = agent_names
        .iter()
        .map(|name| agent::agent_by_name(name).unwrap())
        .collect();

    while !game.is_over() {
        let observations = game.observations();
        let mut orders = Vec::new();
        let mut turn_us = vec![0u64; num_players as usize];

        for (i, agent) in agents.iter_mut().enumerate() {
            let t0 = Instant::now();
            let actions = agent.act(&observations[i], &mut rng);
            turn_us[i] = t0.elapsed().as_micros() as u64;
            orders.push((i as u8, actions));
        }

        game.step(&orders);

        let land: Vec<usize> = (0..num_players).map(|p| game.state.land_count(p)).collect();
        let army: Vec<i32> = (0..num_players).map(|p| game.state.army_count(p)).collect();
        let tp = TurnProfile {
            turn: game.state.turn,
            compute_us: turn_us,
            land,
            army,
        };
        println!("{}", serde_json::to_string(&tp).unwrap());
    }
}

// ---------------------------------------------------------------------------
// Core game runner
// ---------------------------------------------------------------------------

fn run_bench_game(
    seed: u64,
    agent_names: &[&str],
    max_turns: u32,
    (w, h): (usize, usize),
) -> GameResult {
    let num_players = agent_names.len() as u8;
    let mut rng = StdRng::seed_from_u64(seed);
    let config = MapConfig::for_size(w, h, num_players);
    let state = mapgen::generate(&config, &mut rng);
    let mut game = Game::with_seed(state, max_turns, seed);

    let mut agents: Vec<Box<dyn Agent>> = agent_names
        .iter()
        .map(|name| agent::agent_by_name(name).unwrap())
        .collect();

    let ids: Vec<String> = agents.iter().map(|a| a.id()).collect();
    let matchup_key = agent_names.join("-vs-");

    let np = num_players as usize;
    let mut compute_total = vec![0u64; np];
    let mut compute_max = vec![0u64; np];
    let mut turn_count = 0u32;

    // Snapshots for scoring — sample every 25 turns.
    let mut snapshots: Vec<Snapshot> = Vec::new();
    // Track lead changes for interestingness.
    let mut prev_leader: Option<u8> = None;
    let mut lead_changes: u32 = 0;
    // Track max army advantage either player achieved.
    let mut max_army_ratio: f64 = 1.0;
    // Track if the eventual winner was ever behind.
    let mut was_behind: Vec<bool> = vec![false; np];

    while !game.is_over() {
        let observations = game.observations();
        let mut orders = Vec::new();

        for (i, agent) in agents.iter_mut().enumerate() {
            let t0 = Instant::now();
            let actions = agent.act(&observations[i], &mut rng);
            let elapsed = t0.elapsed().as_micros() as u64;

            compute_total[i] += elapsed;
            if elapsed > compute_max[i] {
                compute_max[i] = elapsed;
            }
            orders.push((i as u8, actions));
        }

        game.step(&orders);
        turn_count += 1;

        // Sample every 25 turns for scoring.
        if game.state.turn % 25 == 0 || game.is_over() {
            let land: Vec<usize> = (0..num_players).map(|p| game.state.land_count(p)).collect();
            let army: Vec<i32> = (0..num_players).map(|p| game.state.army_count(p)).collect();
            let alive: Vec<bool> = (0..num_players)
                .map(|p| game.state.alive[p as usize])
                .collect();

            // Track leader and lead changes.
            let leader = land
                .iter()
                .enumerate()
                .filter(|(i, _)| alive[*i])
                .max_by_key(|(_, l)| **l)
                .map(|(i, _)| i as u8);

            if let (Some(prev), Some(curr)) = (prev_leader, leader) {
                if prev != curr {
                    lead_changes += 1;
                }
            }
            prev_leader = leader;

            // Track army ratio.
            let armies_alive: Vec<i32> = army
                .iter()
                .enumerate()
                .filter(|(i, _)| alive[*i])
                .map(|(_, &a)| a)
                .collect();
            if armies_alive.len() >= 2 {
                let max_a = *armies_alive.iter().max().unwrap() as f64;
                let min_a = (*armies_alive.iter().min().unwrap()).max(1) as f64;
                let ratio = max_a / min_a;
                if ratio > max_army_ratio {
                    max_army_ratio = ratio;
                }
            }

            // Track who was behind.
            if let Some(ldr) = leader {
                for i in 0..np {
                    if i as u8 != ldr && alive[i] {
                        was_behind[i] = true;
                    }
                }
            }

            snapshots.push(Snapshot {
                turn: game.state.turn,
                land,
                army,
                alive,
            });
        }
    }

    let winner_idx = game.state.winner;
    let winner = winner_idx.map(|wi| ids[wi as usize].clone());

    let final_land: Vec<usize> = (0..num_players).map(|p| game.state.land_count(p)).collect();
    let final_army: Vec<i32> = (0..num_players).map(|p| game.state.army_count(p)).collect();

    let compute_mean: Vec<f64> = compute_total
        .iter()
        .map(|&t| {
            if turn_count > 0 {
                t as f64 / turn_count as f64
            } else {
                0.0
            }
        })
        .collect();

    // Compute interestingness score.
    let (interest_score, interest_tags) =
        score_game(winner_idx, turn_count, lead_changes, max_army_ratio, &was_behind, &snapshots, max_turns);

    GameResult {
        seed,
        matchup: matchup_key,
        agents: ids,
        winner,
        winner_idx,
        turns: game.state.turn,
        draw: winner_idx.is_none(),
        compute_total_us: compute_total,
        compute_mean_us: compute_mean,
        compute_max_us: compute_max,
        final_land,
        final_army,
        interest_score,
        interest_tags,
        snapshots,
    }
}

// ---------------------------------------------------------------------------
// Game interestingness scoring
// ---------------------------------------------------------------------------

fn score_game(
    winner_idx: Option<u8>,
    turns: u32,
    lead_changes: u32,
    _max_army_ratio: f64,
    was_behind: &[bool],
    snapshots: &[Snapshot],
    max_turns: u32,
) -> (f64, Vec<String>) {
    let mut score: f64 = 0.0;
    let mut tags: Vec<String> = Vec::new();

    // Lead changes — weighted by when they happen. Early lead changes are
    // common (expander grabs land first), so only mid/late ones count heavily.
    // We count changes that happen after turn 50 as "real" lead changes.
    let late_lead_changes = count_late_lead_changes(snapshots, 50);
    if late_lead_changes >= 3 {
        score += 40.0;
        tags.push(format!("seesaw({})", late_lead_changes));
    } else if late_lead_changes >= 1 {
        score += 20.0 * late_lead_changes as f64;
        tags.push(format!("lead-flip({})", late_lead_changes));
    }

    // Comeback: winner was behind at a late checkpoint (after turn 75).
    if let Some(wi) = winner_idx {
        if winner_was_behind_late(wi, snapshots, 75) {
            score += 30.0;
            tags.push("comeback".into());
        }
    }

    // Draw: max turns reached without elimination.
    if winner_idx.is_none() && turns >= max_turns {
        score += 25.0;
        tags.push("draw".into());
    }

    // Long game (close to max turns but not a draw) — the closer to max, the more interesting.
    if winner_idx.is_some() && turns > max_turns / 2 {
        let long_score = (turns as f64 / max_turns as f64 - 0.5) * 30.0;
        score += long_score;
        tags.push(format!("long(T{})", turns));
    }

    // Very short game = rush/blitz — unusual, worth investigating.
    if turns < max_turns / 6 {
        score += 15.0;
        tags.push(format!("blitz(T{})", turns));
    }

    // Closeness at the 75% mark — games that are close late are the most interesting.
    let q3_idx = snapshots.len() * 3 / 4;
    if q3_idx > 0 && q3_idx < snapshots.len() {
        let snap = &snapshots[q3_idx];
        let lands_alive: Vec<usize> = snap
            .land
            .iter()
            .enumerate()
            .filter(|(i, _)| snap.alive[*i])
            .map(|(_, &l)| l)
            .collect();
        if lands_alive.len() >= 2 {
            let max_l = *lands_alive.iter().max().unwrap() as f64;
            let min_l = (*lands_alive.iter().min().unwrap()).max(1) as f64;
            let ratio = max_l / min_l;
            if ratio < 1.3 {
                score += 25.0;
                tags.push(format!("neck-and-neck({:.2}x)", ratio));
            } else if ratio < 1.8 {
                score += 10.0;
                tags.push(format!("competitive({:.2}x)", ratio));
            }
        }
    }

    // Upset: minority agent won (useful when one agent dominates overall).
    // Losses are inherently interesting when one agent is strong.
    // Tag it but don't score heavily — the user decides what matters.
    if let Some(wi) = winner_idx {
        if wi != 0 {
            score += 5.0;
            tags.push("upset".into());
        }
    }

    (score, tags)
}

/// Count lead changes that happen after a given turn threshold.
fn count_late_lead_changes(snapshots: &[Snapshot], after_turn: u32) -> u32 {
    let mut changes = 0u32;
    let mut prev_leader: Option<usize> = None;

    for snap in snapshots {
        let leader = snap
            .land
            .iter()
            .enumerate()
            .filter(|(i, _)| snap.alive[*i])
            .max_by_key(|(_, l)| **l)
            .map(|(i, _)| i);

        if snap.turn > after_turn {
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

/// Check if the winner was behind on land at any snapshot after a given turn.
fn winner_was_behind_late(winner: u8, snapshots: &[Snapshot], after_turn: u32) -> bool {
    for snap in snapshots {
        if snap.turn <= after_turn || !snap.alive[winner as usize] {
            continue;
        }
        let winner_land = snap.land[winner as usize];
        let any_ahead = snap
            .land
            .iter()
            .enumerate()
            .any(|(i, &l)| i != winner as usize && snap.alive[i] && l > winner_land);
        if any_ahead {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Output formatting
// ---------------------------------------------------------------------------

fn print_matchup_summary(stats: &MatchupStats) {
    eprintln!("\n  {:>16} {:>5} {:>6} {:>10} {:>10} {:>10}",
        "agent", "wins", "win%", "avg_us/t", "max_us", "total_ms"
    );
    for i in 0..stats.agents.len() {
        let pct = if stats.games_played > 0 {
            stats.wins[i] as f64 / stats.games_played as f64 * 100.0
        } else {
            0.0
        };
        let avg_per_turn = if stats.total_turns > 0 {
            stats.total_compute_us[i] as f64 / stats.total_turns as f64
        } else {
            0.0
        };
        eprintln!(
            "  {:>16} {:>5} {:>5.1}% {:>10.1} {:>10} {:>10.1}",
            stats.agents[i],
            stats.wins[i],
            pct,
            avg_per_turn,
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
        "  total: {} turns across {} games ({:.0} turns/game avg)",
        stats.total_turns,
        stats.games_played,
        stats.total_turns as f64 / stats.games_played.max(1) as f64,
    );
}

fn print_interesting_games(results: &[GameResult], top_n: usize) {
    if results.is_empty() {
        return;
    }

    let mut ranked: Vec<&GameResult> = results.iter().collect();
    ranked.sort_by(|a, b| b.interest_score.partial_cmp(&a.interest_score).unwrap());

    eprintln!("\n  Top {} interesting games:", top_n.min(ranked.len()));
    eprintln!(
        "  {:>6} {:>6} {:>8} {:>14} {:>8}  {}",
        "seed", "turns", "winner", "final_land", "score", "tags"
    );
    for r in ranked.iter().take(top_n) {
        let winner_str = r
            .winner
            .as_deref()
            .unwrap_or("draw");
        let land_str = r
            .final_land
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("/");
        eprintln!(
            "  {:>6} {:>6} {:>8} {:>14} {:>8.1}  {}",
            r.seed,
            r.turns,
            winner_str,
            land_str,
            r.interest_score,
            r.interest_tags.join(", "),
        );
    }
}
