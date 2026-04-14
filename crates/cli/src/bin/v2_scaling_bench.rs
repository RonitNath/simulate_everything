use serde::Serialize;
use simulate_everything_engine::v2::{
    AGENT_POLL_INTERVAL,
    agent::{self as v2_agent, Agent},
    directive,
    hex::offset_to_axial,
    mapgen::{self, MapConfig},
    observation::{self, Observation, ObservationSession},
    replay::UnitSnapshot,
    sim,
    state::{GameState, Unit},
};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
enum CurrentSpectatorMsg {
    #[serde(rename = "v2_game_start")]
    GameStart {
        width: usize,
        height: usize,
        terrain: Vec<f32>,
        material_map: Vec<f32>,
        num_players: u8,
        agent_names: Vec<String>,
    },
    #[serde(rename = "v2_frame")]
    Frame {
        tick: u64,
        units: Vec<UnitSnapshot>,
        player_food: Vec<f32>,
        player_material: Vec<f32>,
        alive: Vec<bool>,
    },
}

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    width: usize,
    height: usize,
    players: u8,
    ticks: u64,
    seed: u64,
}

#[derive(Default, Clone)]
struct DurationStats {
    samples: Vec<u64>,
}

impl DurationStats {
    fn push(&mut self, duration: Duration) {
        self.samples.push(duration.as_nanos() as u64);
    }

    fn avg_ms(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<u64>() as f64 / self.samples.len() as f64 / 1_000_000.0
    }

    fn max_ms(&self) -> f64 {
        self.samples.iter().copied().max().unwrap_or(0) as f64 / 1_000_000.0
    }

    fn percentile_ms(&self, pct: f64) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let idx = (((sorted.len() - 1) as f64) * pct).round() as usize;
        sorted[idx] as f64 / 1_000_000.0
    }
}

#[derive(Default, Clone)]
struct SizeStats {
    samples: Vec<usize>,
}

impl SizeStats {
    fn push(&mut self, value: usize) {
        self.samples.push(value);
    }

    fn avg_kib(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<usize>() as f64 / self.samples.len() as f64 / 1024.0
    }

    fn max_kib(&self) -> f64 {
        self.samples.iter().copied().max().unwrap_or(0) as f64 / 1024.0
    }
}

#[derive(Default)]
struct CountStats {
    samples: Vec<usize>,
}

impl CountStats {
    fn push(&mut self, value: usize) {
        self.samples.push(value);
    }

    fn avg(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().sum::<usize>() as f64 / self.samples.len() as f64
    }

    fn max(&self) -> usize {
        self.samples.iter().copied().max().unwrap_or(0)
    }
}

#[derive(Default)]
struct ScenarioMetrics {
    observe_time: DurationStats,
    observe_bytes: SizeStats,
    sim_time: DurationStats,
    frame_time: DurationStats,
    frame_bytes: SizeStats,
    units: CountStats,
    population: CountStats,
    convoys: CountStats,
    polling_ticks: u64,
    frame_samples: u64,
    game_start_bytes: usize,
}

fn snapshot_units(state: &GameState) -> Vec<UnitSnapshot> {
    state
        .units
        .values()
        .map(|u| UnitSnapshot {
            id: u.public_id,
            owner: u.owner,
            q: u.pos.q,
            r: u.pos.r,
            strength: u.strength,
            engagements: u.engagements.clone(),
            move_cooldown: u.move_cooldown,
            destination: u.destination,
            engaged: !u.engagements.is_empty(),
            rations: u.rations,
        })
        .collect()
}

fn current_frame(state: &GameState) -> CurrentSpectatorMsg {
    CurrentSpectatorMsg::Frame {
        tick: state.tick,
        units: snapshot_units(state),
        player_food: state.players.iter().map(|p| p.food).collect(),
        player_material: state.players.iter().map(|p| p.material).collect(),
        alive: state.players.iter().map(|p| p.alive).collect(),
    }
}

fn game_start_message(
    state: &GameState,
    players: u8,
    agent_names: &[String],
) -> CurrentSpectatorMsg {
    CurrentSpectatorMsg::GameStart {
        width: state.width,
        height: state.height,
        terrain: state.grid.iter().map(|c| c.terrain_value).collect(),
        material_map: state.grid.iter().map(|c| c.material_value).collect(),
        num_players: players,
        agent_names: agent_names.to_vec(),
    }
}

fn serialize_len<T: Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .expect("serialization should succeed")
        .len()
}

fn benchmark_observation(
    state: &mut GameState,
    player_id: u8,
    metrics: &mut ScenarioMetrics,
) -> Observation {
    let start = Instant::now();
    let obs = observation::observe(state, player_id);
    metrics.observe_time.push(start.elapsed());
    metrics.observe_bytes.push(serialize_len(&obs));
    obs
}

fn run_scenario(scenario: Scenario) {
    let mut state = mapgen::generate(&MapConfig {
        width: scenario.width,
        height: scenario.height,
        num_players: scenario.players,
        seed: scenario.seed,
    });

    let agent_names: Vec<String> = (0..scenario.players)
        .map(|_| "spread".to_string())
        .collect();
    let mut agents: Vec<Box<dyn Agent>> = (0..scenario.players)
        .map(|_| v2_agent::agent_by_name("spread").expect("spread agent exists"))
        .collect();
    let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
    for (pid, agent) in agents.iter_mut().enumerate() {
        let init = observation::initial_observation(&state, pid as u8);
        agent.reset();
        agent.init(&init);
    }

    let mut metrics = ScenarioMetrics {
        game_start_bytes: serialize_len(&game_start_message(
            &state,
            scenario.players,
            &agent_names,
        )),
        ..ScenarioMetrics::default()
    };

    for _ in 0..scenario.ticks {
        if state.tick.is_multiple_of(AGENT_POLL_INTERVAL as u64) {
            metrics.polling_ticks += 1;
            for (pid, agent) in agents.iter_mut().enumerate() {
                let player_id = pid as u8;
                if !state.players.iter().any(|p| p.id == player_id && p.alive) {
                    continue;
                }
                let _obs = benchmark_observation(&mut state, player_id, &mut metrics);
                let delta = observation::observe_delta(&mut state, player_id, &mut session);
                let directives = agent.act(&delta);
                directive::apply_directives(&mut state, player_id, &directives);
            }
            state.clear_dirty_hexes();
        }

        let sim_start = Instant::now();
        sim::tick(&mut state);
        metrics.sim_time.push(sim_start.elapsed());

        metrics.units.push(state.units.len());
        metrics.population.push(state.population.len());
        metrics.convoys.push(state.convoys.len());

        let frame = current_frame(&state);
        let frame_start = Instant::now();
        let frame_bytes = serialize_len(&frame);
        metrics.frame_time.push(frame_start.elapsed());
        metrics.frame_bytes.push(frame_bytes);
        metrics.frame_samples += 1;
    }

    println!(
        "scenario={} map={}x{} players={} ticks={} seed={}",
        scenario.name,
        scenario.width,
        scenario.height,
        scenario.players,
        scenario.ticks,
        scenario.seed
    );
    println!(
        "  entities avg_units={:.1} max_units={} avg_population={} max_population={} avg_convoys={:.1} max_convoys={}",
        metrics.units.avg(),
        metrics.units.max(),
        metrics.population.avg().round() as usize,
        metrics.population.max(),
        metrics.convoys.avg(),
        metrics.convoys.max()
    );
    println!(
        "  observe per_player samples={} avg_ms={:.3} p50_ms={:.3} p95_ms={:.3} max_ms={:.3} avg_json_kib={:.1} max_json_kib={:.1}",
        metrics.observe_time.samples.len(),
        metrics.observe_time.avg_ms(),
        metrics.observe_time.percentile_ms(0.50),
        metrics.observe_time.percentile_ms(0.95),
        metrics.observe_time.max_ms(),
        metrics.observe_bytes.avg_kib(),
        metrics.observe_bytes.max_kib()
    );
    println!(
        "  tick sim samples={} avg_ms={:.3} p50_ms={:.3} p95_ms={:.3} max_ms={:.3}",
        metrics.sim_time.samples.len(),
        metrics.sim_time.avg_ms(),
        metrics.sim_time.percentile_ms(0.50),
        metrics.sim_time.percentile_ms(0.95),
        metrics.sim_time.max_ms()
    );
    println!(
        "  spectator current_game_start_kib={:.1} frame_samples={} avg_frame_kib={:.1} max_frame_kib={:.1} avg_frame_serialize_ms={:.3} max_frame_serialize_ms={:.3}",
        metrics.game_start_bytes as f64 / 1024.0,
        metrics.frame_samples,
        metrics.frame_bytes.avg_kib(),
        metrics.frame_bytes.max_kib(),
        metrics.frame_time.avg_ms(),
        metrics.frame_time.max_ms()
    );
    if scenario.width * scenario.height >= 90_000 {
        for target_units in [500usize, 2_000, 10_000] {
            run_synthetic_unit_probe(&state, scenario.players, target_units);
        }
    }
    println!();
}

fn inflate_units(state: &mut GameState, target_units: usize) {
    if state.units.len() >= target_units {
        return;
    }
    let additional = target_units - state.units.len();
    for i in 0..additional {
        let cell = i % (state.width * state.height);
        let row = (cell / state.width) as i32;
        let col = (cell % state.width) as i32;
        let owner = (i as u8) % state.players.len() as u8;
        state.units.insert(Unit {
            public_id: state.next_unit_id,
            owner,
            pos: offset_to_axial(row, col),
            strength: 25.0,
            move_cooldown: 0,
            engagements: Vec::new(),
            destination: None,
            rations: simulate_everything_engine::v2::MAX_RATIONS,
            half_rations: false,
        });
        state.next_unit_id += 1;
    }
    state.rebuild_spatial();
}

fn run_synthetic_unit_probe(base_state: &GameState, players: u8, target_units: usize) {
    let mut state = base_state.clone();
    inflate_units(&mut state, target_units);

    let mut observe_stats = DurationStats::default();
    let mut observe_bytes = SizeStats::default();
    for player_id in 0..players {
        let start = Instant::now();
        let obs = observation::observe(&mut state, player_id);
        observe_stats.push(start.elapsed());
        observe_bytes.push(serialize_len(&obs));
    }

    let frame = current_frame(&state);
    let frame_start = Instant::now();
    let frame_bytes = serialize_len(&frame);
    let frame_ms = frame_start.elapsed().as_secs_f64() * 1000.0;

    println!(
        "  synthetic units={} observe_avg_ms={:.3} observe_max_ms={:.3} observe_avg_json_kib={:.1} frame_kib={:.1} frame_serialize_ms={:.3}",
        state.units.len(),
        observe_stats.avg_ms(),
        observe_stats.max_ms(),
        observe_bytes.avg_kib(),
        frame_bytes as f64 / 1024.0,
        frame_ms
    );
}

fn main() {
    let scenarios = [
        Scenario {
            name: "small_rr",
            width: 30,
            height: 30,
            players: 2,
            ticks: 500,
            seed: 100,
        },
        Scenario {
            name: "medium_4p",
            width: 100,
            height: 100,
            players: 4,
            ticks: 400,
            seed: 200,
        },
        Scenario {
            name: "large_4p",
            width: 300,
            height: 300,
            players: 4,
            ticks: 250,
            seed: 300,
        },
    ];

    for scenario in scenarios {
        run_scenario(scenario);
    }
}
