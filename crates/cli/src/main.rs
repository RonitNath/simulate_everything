use generals_engine::agent::{self, Agent};
use generals_engine::game::Game;
use generals_engine::mapgen::{self, MapConfig};
use rand::rngs::StdRng;
use rand::SeedableRng;
use serde::Serialize;
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

    use generals_engine::agent::all_builtin_agents;
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
#[derive(Serialize)]
struct GameResult {
    seed: u64,
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
}

/// Per-turn timing detail for --profile mode.
#[derive(Serialize)]
struct TurnProfile {
    turn: u32,
    /// Per-agent compute time in microseconds for this turn.
    compute_us: Vec<u64>,
    /// Per-agent land count.
    land: Vec<usize>,
    /// Per-agent army count.
    army: Vec<i32>,
}

fn bench_main(args: &[String]) {
    let profile_mode = args.iter().any(|a| a == "--profile");

    let seeds = flag_value(args, "--seeds")
        .map(parse_seed_range)
        .unwrap_or_else(|| (100..=249).collect());

    let agent_names_str = flag_value(args, "--agents").unwrap_or("pressure,swarm");
    let agent_names: Vec<&str> = agent_names_str.split(',').collect();

    let num_players = agent_names.len() as u8;

    let max_turns: u32 = flag_value(args, "--turns")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    let size = flag_value(args, "--size").map(parse_size);

    // Validate agent names.
    for name in &agent_names {
        if agent::agent_by_name(name).is_none() {
            eprintln!(
                "error: unknown agent '{}'. known agents: {:?}",
                name,
                agent::builtin_agent_names()
            );
            std::process::exit(1);
        }
    }

    let (w, h) = size.unwrap_or_else(|| {
        let cfg = MapConfig::for_players(num_players);
        (cfg.width, cfg.height)
    });

    eprintln!(
        "bench: {} seeds, agents={:?}, {}x{}, max_turns={}{}",
        seeds.len(),
        agent_names,
        w,
        h,
        max_turns,
        if profile_mode { " [profile]" } else { "" },
    );

    // Aggregate stats for summary.
    let mut wins: Vec<u32> = vec![0; num_players as usize];
    let mut draws: u32 = 0;
    let mut total_compute_us: Vec<u64> = vec![0; num_players as usize];
    let mut total_max_us: Vec<u64> = vec![0; num_players as usize];
    let mut total_turns: u64 = 0;
    let mut games_played: u32 = 0;

    for &seed in &seeds {
        let result = run_bench_game(
            seed,
            &agent_names,
            max_turns,
            (w, h),
            profile_mode,
        );

        // Update aggregates.
        if result.draw {
            draws += 1;
        } else if let Some(wi) = result.winner_idx {
            wins[wi as usize] += 1;
        }
        for (i, &ct) in result.compute_total_us.iter().enumerate() {
            total_compute_us[i] += ct;
            if result.compute_max_us[i] > total_max_us[i] {
                total_max_us[i] = result.compute_max_us[i];
            }
        }
        total_turns += result.turns as u64;
        games_played += 1;

        // Emit per-game JSON line to stdout.
        if !profile_mode {
            println!("{}", serde_json::to_string(&result).unwrap());
        }
    }

    // Summary to stderr.
    eprintln!("\n===== SUMMARY ({} games) =====", games_played);
    eprintln!(
        "{:<16} {:>5} {:>6} {:>10} {:>10} {:>10}",
        "agent", "wins", "win%", "avg_us/t", "max_us", "total_ms"
    );
    for i in 0..num_players as usize {
        let pct = if games_played > 0 {
            wins[i] as f64 / games_played as f64 * 100.0
        } else {
            0.0
        };
        let avg_per_turn = if total_turns > 0 {
            total_compute_us[i] as f64 / total_turns as f64
        } else {
            0.0
        };
        eprintln!(
            "{:<16} {:>5} {:>5.1}% {:>10.1} {:>10} {:>10.1}",
            agent_names[i],
            wins[i],
            pct,
            avg_per_turn,
            total_max_us[i],
            total_compute_us[i] as f64 / 1000.0,
        );
    }
    if draws > 0 {
        eprintln!("draws: {}", draws);
    }
    eprintln!(
        "total: {} turns across {} games ({:.0} turns/game avg)",
        total_turns,
        games_played,
        total_turns as f64 / games_played.max(1) as f64,
    );
}

fn run_bench_game(
    seed: u64,
    agent_names: &[&str],
    max_turns: u32,
    (w, h): (usize, usize),
    profile_mode: bool,
) -> GameResult {
    let num_players = agent_names.len() as u8;
    let mut rng = StdRng::seed_from_u64(seed);
    let config = MapConfig::for_size(w, h, num_players);
    let state = mapgen::generate(&config, &mut rng);
    let mut game = Game::with_seed(state, max_turns, seed);

    // Create agents in the specified order (no shuffle).
    let mut agents: Vec<Box<dyn Agent>> = agent_names
        .iter()
        .map(|name| agent::agent_by_name(name).unwrap())
        .collect();

    let ids: Vec<String> = agents.iter().map(|a| a.id()).collect();

    // Per-agent timing accumulators.
    let np = num_players as usize;
    let mut compute_total = vec![0u64; np];
    let mut compute_max = vec![0u64; np];
    let mut turn_count = 0u32;

    while !game.is_over() {
        let observations = game.observations();
        let mut orders = Vec::new();
        let mut turn_us = vec![0u64; np];

        for (i, agent) in agents.iter_mut().enumerate() {
            let t0 = Instant::now();
            let actions = agent.act(&observations[i], &mut rng);
            let elapsed = t0.elapsed().as_micros() as u64;

            turn_us[i] = elapsed;
            compute_total[i] += elapsed;
            if elapsed > compute_max[i] {
                compute_max[i] = elapsed;
            }
            orders.push((i as u8, actions));
        }

        game.step(&orders);
        turn_count += 1;

        if profile_mode {
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

    let winner_idx = game.state.winner;
    let winner = winner_idx.map(|wi| ids[wi as usize].clone());

    let final_land: Vec<usize> = (0..num_players).map(|p| game.state.land_count(p)).collect();
    let final_army: Vec<i32> = (0..num_players).map(|p| game.state.army_count(p)).collect();

    let compute_mean: Vec<f64> = compute_total
        .iter()
        .map(|&t| if turn_count > 0 { t as f64 / turn_count as f64 } else { 0.0 })
        .collect();

    GameResult {
        seed,
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
    }
}
