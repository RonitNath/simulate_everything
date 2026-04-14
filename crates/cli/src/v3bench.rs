use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use simulate_everything_engine::v2::hex::offset_to_axial;
use simulate_everything_engine::v2::state::EntityKey;
use simulate_everything_engine::v3::{
    agent::{AgentOutput, LayeredAgent, StrategyLayer},
    armor::{self, ArmorProperties, BodyZone, DamageType},
    combat_log::CombatObservation,
    damage::{self, BlockCapability, DefenderState, Impact, ImpactResult},
    damage_table::DamageEstimateTable,
    equipment::{self, Equipment},
    formation::FormationType,
    hex::hex_to_world,
    lifecycle::{contain, spawn_entity},
    mapgen,
    martial::{AttackMotion, BlockManeuver},
    movement::Mobile,
    operations::{NullOperationsLayer, SharedOperationsLayer},
    projectile, sim,
    spatial::{GeoMaterial, Heightfield, Vec3},
    state::{Combatant, EntityBuilder, GameState, Person, Role, Stack},
    strategy::{NullStrategy, SpreadStrategy, StrikerStrategy, TurtleStrategy},
    tactical::{NullTacticalLayer, SharedTacticalLayer},
    vitals::{MovementMode, Vitals},
    weapon::{self, AttackState},
    wound::{Severity, Wound},
};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use simulate_everything_web::v3_protocol;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn main(args: &[String]) {
    if args.iter().any(|a| a == "--mechanics") {
        run_mechanics_suite(args);
        return;
    }

    if args.iter().any(|a| a == "--swordplay-drill") {
        run_swordplay_drill(args);
        return;
    }

    if args.iter().any(|a| a == "--arena") {
        let arena_mode = flag_value(args, "--arena-mode").unwrap_or("null-vs-striker");
        let arena_config = flag_value(args, "--arena-config")
            .map(load_arena_config)
            .unwrap_or_else(|| ArenaConfigFile::for_mode(arena_mode));
        let replay_path = flag_value(args, "--replay");
        run_arena(&arena_config, replay_path);
        return;
    }

    let profile_mode = args.iter().any(|a| a == "--profile");
    let converge_mode = args.iter().any(|a| a == "--converge");
    let ascii_mode = args.iter().any(|a| a == "--ascii");
    let personality_report_mode = args.iter().any(|a| a == "--personality-report");
    let top_n: usize = flag_value(args, "--top")
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);

    let seeds = flag_value(args, "--seeds")
        .map(parse_seed_range)
        .unwrap_or_else(|| {
            if personality_report_mode {
                (0..100).collect()
            } else {
                (0..149).collect()
            }
        });

    let max_ticks: u64 = flag_value(args, "--ticks")
        .and_then(|s| s.parse().ok())
        .unwrap_or(2000);

    let (w, h) = flag_value(args, "--size")
        .map(parse_size)
        .unwrap_or_else(|| {
            if personality_report_mode {
                (20, 20)
            } else {
                (30, 30)
            }
        });

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
    let matchups: Vec<Vec<&str>> = if personality_report_mode {
        personality_report_matchups()
    } else if let Some(m) = flag_value(args, "--matchups") {
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

    if personality_report_mode {
        if num_players != 2 {
            eprintln!("error: --personality-report requires --players 2");
            std::process::exit(1);
        }
        let report_out =
            flag_value(args, "--report-out").unwrap_or("docs/v3-personality-report.md");
        let report_data_dir =
            flag_value(args, "--report-data-dir").unwrap_or("var/v3_personality_report");
        run_personality_report(
            &matchups,
            &seeds,
            max_ticks,
            (w, h),
            num_players,
            Path::new(report_out),
            Path::new(report_data_dir),
            args,
            &interrupted,
        );
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
    &["spread", "striker", "turtle", "null"]
}

fn personality_report_matchups() -> Vec<Vec<&'static str>> {
    let personalities = ["spread", "striker", "turtle"];
    let mut matchups = Vec::new();
    for &left in &personalities {
        for &right in &personalities {
            matchups.push(vec![left, right]);
        }
    }
    matchups
}

fn make_agent(name: &str, player: u8) -> LayeredAgent {
    if name == "null" {
        return LayeredAgent::new(
            Box::new(NullStrategy),
            Box::new(NullOperationsLayer),
            Box::new(NullTacticalLayer),
            player,
            50,
            5,
        );
    }
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

fn init_bench_state(width: usize, height: usize, num_players: u8, seed: u64) -> GameState {
    mapgen::generate_economy_ready(width, height, num_players, seed)
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

#[derive(Serialize)]
struct PersonalityReportData {
    metadata: PersonalityReportMetadata,
    matchup_summaries: Vec<MatchupReportSummary>,
    personality_summaries: Vec<PersonalityReportSummary>,
}

#[derive(Serialize)]
struct PersonalityReportMetadata {
    generated_at_epoch_s: u64,
    git_head: String,
    git_dirty: bool,
    command: String,
    seeds: String,
    ticks: u64,
    size: String,
    snapshot_interval: u64,
    games: usize,
}

#[derive(Serialize)]
struct MatchupReportSummary {
    matchup: String,
    agents: Vec<String>,
    games: usize,
    wins: Vec<u32>,
    draws: u32,
    win_rates: Vec<f64>,
    draw_rate: f64,
    avg_ticks: f64,
    avg_deaths: f64,
    avg_final_entities: Vec<f64>,
    avg_final_soldiers: Vec<f64>,
    avg_final_territory: Vec<f64>,
    diagnosis: MatchupDiagnosis,
}

#[derive(Serialize)]
struct MatchupDiagnosis {
    zero_deaths: bool,
    flat_entities: bool,
    flat_soldiers: bool,
    flat_territory: bool,
    attrition_without_resolution: bool,
    notes: Vec<String>,
}

#[derive(Serialize)]
struct PersonalityReportSummary {
    personality: String,
    games: usize,
    wins: u32,
    draws: u32,
    losses: u32,
    win_rate: f64,
    draw_rate: f64,
    avg_ticks: f64,
    avg_deaths: f64,
    avg_final_entities: f64,
    avg_final_soldiers: f64,
    avg_final_territory: f64,
}

// ---------------------------------------------------------------------------
// Arena config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct ArenaConfigFile {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    max_ticks: Option<u64>,
    #[serde(default)]
    cluster_radius_m: Option<f32>,
    #[serde(default)]
    side_a_center: Option<[f32; 2]>,
    #[serde(default)]
    side_b_center: Option<[f32; 2]>,
    #[serde(default)]
    side_a: Option<ArenaSideConfigFile>,
    #[serde(default)]
    side_b: Option<ArenaSideConfigFile>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ArenaSideConfigFile {
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    soldiers: Option<usize>,
    #[serde(default)]
    weapon_preset: Option<String>,
    #[serde(default)]
    armor: Option<String>,
    #[serde(default)]
    armor_ratio: Option<f32>,
    #[serde(default)]
    formation: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArenaWeaponPreset {
    Swords,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArenaArmorPreset {
    None,
    LeatherCuirass,
    BronzeBreastplate,
}

#[derive(Debug, Clone)]
struct ArenaScenario {
    title: String,
    max_ticks: u64,
    cluster_radius_m: f32,
    side_a: ArenaSideScenario,
    side_b: ArenaSideScenario,
}

#[derive(Debug, Clone)]
struct ArenaSideScenario {
    owner: u8,
    agent: String,
    soldiers: usize,
    weapon_preset: ArenaWeaponPreset,
    armor_preset: ArenaArmorPreset,
    armor_ratio: f32,
    formation: FormationType,
    center: Vec3,
}

#[derive(Debug, Clone)]
struct ArenaSideState {
    agent: String,
    members: Vec<EntityKey>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ArenaSideSummary {
    alive: usize,
    total: usize,
    wounds: usize,
    avg_blood: f32,
    attacking: usize,
    cooling_down: usize,
}

#[derive(Debug, Clone, Serialize)]
struct MechanicsSuiteResult {
    suite: &'static str,
    strict: bool,
    passed: usize,
    failed: usize,
    scenarios: Vec<MechanicsScenarioResult>,
    implementation_gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MechanicsScenarioResult {
    id: String,
    level: &'static str,
    description: String,
    intended_effect: String,
    observed_effect: String,
    meets_intended: bool,
    metrics: BTreeMap<String, f64>,
    artifact_paths: Vec<String>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ArenaArtifact {
    id: String,
    description: String,
    winner: Option<u8>,
    final_tick: u64,
    side_a: ArenaSideSummary,
    side_b: ArenaSideSummary,
    timeline: Vec<ArenaTimelineFrame>,
}

#[derive(Debug, Clone, Serialize)]
struct ArenaTimelineFrame {
    tick: u64,
    avg_distance: f32,
    side_a: ArenaSideSummary,
    side_b: ArenaSideSummary,
    soldiers: Vec<ArenaUnitFrame>,
    combat_log: Vec<CombatObservation>,
}

#[derive(Debug, Clone, Serialize)]
struct ArenaUnitFrame {
    id: u32,
    owner: u8,
    x: f32,
    y: f32,
    z: f32,
    blood: f32,
    stamina: f32,
    alive: bool,
    wounds: usize,
    combat_skill: f32,
}

#[derive(Debug, Clone)]
struct ArenaMechanicVariant {
    id: String,
    description: String,
    scenario: ArenaScenario,
    post_setup: ArenaPostSetup,
}

#[derive(Debug, Clone)]
struct ArenaAggregate {
    mean_advantaged_score: f64,
    success_rate: f64,
    draw_rate: f64,
    sample: ArenaArtifact,
}

#[derive(Debug, Clone, Default)]
struct ArenaPostSetup {
    side_a_z_offset: f32,
    side_b_z_offset: f32,
    side_a_skill: Option<f32>,
    side_b_skill: Option<f32>,
    side_a_blood: Option<f32>,
    side_b_blood: Option<f32>,
    side_a_stamina: Option<f32>,
    side_b_stamina: Option<f32>,
    side_a_movement_mode: Option<MovementMode>,
    side_b_movement_mode: Option<MovementMode>,
    side_a_start_wounds: Vec<Wound>,
    side_b_start_wounds: Vec<Wound>,
}

#[derive(Debug, Clone, Copy)]
enum ArenaExpectation {
    AdvantagedWins,
    AdvantagedLoses,
    SpecGap,
}

impl ArenaConfigFile {
    fn for_mode(mode: &str) -> Self {
        let [side_a_center, side_b_center] = arena_default_centers();
        let side_a_agent = if mode == "mutual" { "striker" } else { "null" };
        Self {
            mode: Some(mode.to_string()),
            max_ticks: Some(200),
            cluster_radius_m: Some(0.0),
            side_a_center: Some(side_a_center),
            side_b_center: Some(side_b_center),
            side_a: Some(ArenaSideConfigFile {
                agent: Some(side_a_agent.to_string()),
                soldiers: Some(1),
                weapon_preset: Some("swords".to_string()),
                armor: Some("none".to_string()),
                armor_ratio: Some(0.0),
                formation: Some("line".to_string()),
            }),
            side_b: Some(ArenaSideConfigFile {
                agent: Some("striker".to_string()),
                soldiers: Some(1),
                weapon_preset: Some("swords".to_string()),
                armor: Some("none".to_string()),
                armor_ratio: Some(0.0),
                formation: Some("line".to_string()),
            }),
        }
    }

    fn resolve(&self) -> ArenaScenario {
        let mode = self.mode.as_deref().unwrap_or("null-vs-striker");
        let defaults = ArenaConfigFile::for_mode(mode);
        let default_cluster = if defaults
            .side_a
            .as_ref()
            .and_then(|s| s.soldiers)
            .unwrap_or(1)
            > 1
        {
            30.0
        } else {
            defaults.cluster_radius_m.unwrap_or(0.0)
        };
        let side_a = resolve_arena_side(
            0,
            self.side_a.as_ref().or(defaults.side_a.as_ref()),
            self.side_a_center
                .or(defaults.side_a_center)
                .unwrap_or([50.0, 50.0]),
        );
        let side_b = resolve_arena_side(
            1,
            self.side_b.as_ref().or(defaults.side_b.as_ref()),
            self.side_b_center
                .or(defaults.side_b_center)
                .unwrap_or([200.0, 50.0]),
        );
        let cluster_radius_m = self.cluster_radius_m.unwrap_or({
            if side_a.soldiers > 1 || side_b.soldiers > 1 {
                30.0
            } else {
                default_cluster
            }
        });
        let matchup_label = if side_a.agent == "striker" && side_b.agent == "striker" {
            "Mutual Combat".to_string()
        } else if side_a.agent == "null" && side_b.agent == "striker" {
            "Null-vs-Striker".to_string()
        } else {
            format!("{}-vs-{}", side_a.agent, side_b.agent)
        };
        let title = format!(
            "=== V3 Arena: {}v{} {} ===",
            side_a.soldiers, side_b.soldiers, matchup_label
        );

        ArenaScenario {
            title,
            max_ticks: self.max_ticks.unwrap_or(200),
            cluster_radius_m,
            side_a,
            side_b,
        }
    }
}

fn arena_default_centers() -> [[f32; 2]; 2] {
    let row = 10;
    let side_a = hex_to_world(offset_to_axial(row, 8));
    let side_b = hex_to_world(offset_to_axial(row, 12));
    [[side_a.x, side_a.y], [side_b.x, side_b.y]]
}

fn resolve_arena_side(
    owner: u8,
    side: Option<&ArenaSideConfigFile>,
    center_xy: [f32; 2],
) -> ArenaSideScenario {
    let side = side.cloned().unwrap_or_default();
    let soldiers = side.soldiers.unwrap_or(1);
    assert!(
        (1..=32).contains(&soldiers),
        "arena side {} soldiers must be between 1 and 32, got {}",
        owner,
        soldiers
    );

    ArenaSideScenario {
        owner,
        agent: side.agent.unwrap_or_else(|| "striker".to_string()),
        soldiers,
        weapon_preset: parse_arena_weapon_preset(side.weapon_preset.as_deref().unwrap_or("swords")),
        armor_preset: parse_arena_armor_preset(side.armor.as_deref().unwrap_or("none")),
        armor_ratio: side.armor_ratio.unwrap_or_else(|| {
            if side.weapon_preset.as_deref() == Some("mixed") {
                0.4
            } else {
                0.0
            }
        }),
        formation: parse_formation_name(side.formation.as_deref().unwrap_or("line")),
        center: Vec3::new(center_xy[0], center_xy[1], 0.0),
    }
}

fn load_arena_config(path: &str) -> ArenaConfigFile {
    let raw = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read arena config {}: {}", path, err));
    let mut config: ArenaConfigFile = toml::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse arena config {}: {}", path, err));
    if config.side_a_center.is_none() {
        config.side_a_center = Some([50.0, 50.0]);
    }
    if config.side_b_center.is_none() {
        config.side_b_center = Some([200.0, 50.0]);
    }
    config
}

fn parse_arena_weapon_preset(value: &str) -> ArenaWeaponPreset {
    match value {
        "swords" => ArenaWeaponPreset::Swords,
        "mixed" => ArenaWeaponPreset::Mixed,
        other => panic!("unsupported arena weapon_preset '{}'", other),
    }
}

fn parse_arena_armor_preset(value: &str) -> ArenaArmorPreset {
    match value {
        "none" => ArenaArmorPreset::None,
        "leather_cuirass" => ArenaArmorPreset::LeatherCuirass,
        "bronze_breastplate" => ArenaArmorPreset::BronzeBreastplate,
        other => panic!("unsupported arena armor '{}'", other),
    }
}

fn parse_formation_name(value: &str) -> FormationType {
    match value {
        "column" => FormationType::Column,
        "line" => FormationType::Line,
        "wedge" => FormationType::Wedge,
        "square" => FormationType::Square,
        "skirmish" => FormationType::Skirmish,
        other => panic!("unsupported arena formation '{}'", other),
    }
}

const SNAP_INTERVAL: u64 = 100;

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

fn run_personality_report(
    matchups: &[Vec<&str>],
    seeds: &[u64],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
    report_out: &Path,
    report_data_dir: &Path,
    args: &[String],
    interrupted: &Arc<AtomicBool>,
) {
    let total_start = Instant::now();
    let mut all_results = Vec::new();
    let mut matchup_summaries = Vec::new();

    for matchup in matchups {
        if interrupted.load(Ordering::Relaxed) {
            break;
        }

        let matchup_key = matchup.join("-vs-");
        eprintln!(
            "\n--- report {} ({} seeds, {}x{}, max_ticks={}) ---",
            matchup_key,
            seeds.len(),
            w,
            h,
            max_ticks,
        );

        let results: Vec<V3GameResult> = seeds
            .par_iter()
            .map(|&seed| run_bench_game(seed, matchup, max_ticks, (w, h), num_players))
            .collect();

        let mut stats = V3MatchupStats::new(matchup);
        for result in results {
            stats.add(result);
        }
        eprintln!(
            "  games={} draws={} avg_ticks={:.1} avg_deaths={:.2}",
            stats.games_played,
            stats.draws,
            average_u64(stats.results.iter().map(|r| r.ticks)),
            average_usize(stats.results.iter().map(|r| r.total_deaths)),
        );
        matchup_summaries.push(build_matchup_report_summary(&stats));
        all_results.extend(stats.results.into_iter());
    }

    let metadata = build_personality_report_metadata(
        args,
        seeds,
        max_ticks,
        (w, h),
        SNAP_INTERVAL,
        all_results.len(),
    );
    let personality_summaries = build_personality_summaries(&all_results);
    let report = PersonalityReportData {
        metadata,
        matchup_summaries,
        personality_summaries,
    };

    persist_personality_report(report_out, report_data_dir, &all_results, &report);
    eprintln!(
        "\nPersonality report written in {:.2?}: {}",
        total_start.elapsed(),
        report_out.display()
    );
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
    let mut state = init_bench_state(w, h, num_players, seed);
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

        sim::apply_agent_outputs(&mut state, &outputs);

        let t0 = Instant::now();
        let result = sim::tick(&mut state, 1.0);
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

#[derive(Debug, Clone, Serialize)]
struct SwordplayDrillFrame {
    step: usize,
    attack_motion: AttackMotion,
    block_maneuver: BlockManeuver,
    attacker_skill: f32,
    defender_skill: f32,
    blocked: bool,
    attack_force: f32,
    stamina_cost: f32,
    note: String,
    ascii: String,
}

#[derive(Debug, Clone)]
struct SwordplayDrillStep {
    motion: AttackMotion,
    block_maneuver: BlockManeuver,
    attacker_skill: f32,
    defender_skill: f32,
    note: &'static str,
}

fn run_swordplay_drill(args: &[String]) {
    let ascii_mode = args.iter().any(|a| a == "--ascii");
    let artifacts_dir = flag_value(args, "--artifacts-dir").map(PathBuf::from);
    let replay_path = flag_value(args, "--replay").map(PathBuf::from);
    let emit_ascii = ascii_mode || artifacts_dir.is_none();
    let (attacker, defender) = mechanics_entity_keys();
    let sword = weapon::iron_sword();
    let attacker_pos = Vec3::new(2.0, 1.0, 0.0);
    let defender_pos = Vec3::new(3.2, 1.0, 0.0);
    let defender_facing = std::f32::consts::PI;

    let steps = vec![
        SwordplayDrillStep {
            motion: AttackMotion::Overhead,
            block_maneuver: BlockManeuver::HighGuard,
            attacker_skill: 0.9,
            defender_skill: 0.9,
            note: "lower defender uses an upper block",
        },
        SwordplayDrillStep {
            motion: AttackMotion::Overhead,
            block_maneuver: BlockManeuver::LowGuard,
            attacker_skill: 0.9,
            defender_skill: 0.05,
            note: "late low guard loses the overhead line",
        },
        SwordplayDrillStep {
            motion: AttackMotion::Forehand,
            block_maneuver: BlockManeuver::OutsideParry,
            attacker_skill: 0.7,
            defender_skill: 0.8,
            note: "standard outside parry against a forehand cut",
        },
        SwordplayDrillStep {
            motion: AttackMotion::Backhand,
            block_maneuver: BlockManeuver::InsideParry,
            attacker_skill: 0.8,
            defender_skill: 0.8,
            note: "inside parry catches the return backhand",
        },
        SwordplayDrillStep {
            motion: AttackMotion::Thrust,
            block_maneuver: BlockManeuver::LowGuard,
            attacker_skill: 0.95,
            defender_skill: 0.95,
            note: "low guard closes the line on a thrust",
        },
    ];

    let mut frames = Vec::new();
    for (idx, step) in steps.iter().enumerate() {
        let attack_state =
            AttackState::for_melee(defender, attacker, step.motion, step.attacker_skill);
        let impact = weapon::resolve_melee(
            &sword,
            attacker,
            attacker_pos,
            0.0,
            defender_pos,
            0.0,
            &attack_state,
            None,
            None,
            idx as u64 + 1,
        )
        .expect("drill attacks are in range");
        let vitals = Vitals::new();
        let defender_state = DefenderState {
            entity_id: defender,
            facing: defender_facing,
            vitals: &vitals,
            block: Some(BlockCapability {
                arc: std::f32::consts::PI,
                efficiency: sword.block_efficiency,
                maneuver: step.block_maneuver,
                read_skill: step.defender_skill,
            }),
            armor_at_zone: [None, None, None, None, None],
        };
        let result = damage::resolve_impact(&impact, &defender_state);
        let (blocked, stamina_cost) = match result {
            ImpactResult::Blocked { stamina_cost, .. } => (true, stamina_cost),
            _ => (false, 0.0),
        };

        frames.push(SwordplayDrillFrame {
            step: idx + 1,
            attack_motion: step.motion,
            block_maneuver: step.block_maneuver,
            attacker_skill: step.attacker_skill,
            defender_skill: step.defender_skill,
            blocked,
            attack_force: impact.kinetic_energy,
            stamina_cost,
            note: step.note.to_string(),
            ascii: render_swordplay_ascii(idx + 1, step.motion, step.block_maneuver, blocked),
        });
    }

    let auto_replay_path = artifacts_dir
        .as_ref()
        .map(|dir| dir.join("swordplay_drill").join("replay.jsonl"));

    if let Some(dir) = artifacts_dir.as_ref() {
        let out_dir = dir.join("swordplay_drill");
        let _ = fs::create_dir_all(&out_dir);
        let _ = fs::write(
            out_dir.join("frames.json"),
            serde_json::to_vec_pretty(&frames).unwrap(),
        );
    }

    if let Some(path) = replay_path.as_ref().or(auto_replay_path.as_ref()) {
        write_swordplay_drill_replay(path, &steps, &frames);
    }

    if emit_ascii {
        for frame in &frames {
            println!("{}", frame.ascii);
            println!(
                "step={} attack={} block={} blocked={} atk_skill={:.2} def_skill={:.2} force={:.2} stamina_cost={:.2} note={}",
                frame.step,
                frame.attack_motion.short_name(),
                frame.block_maneuver.short_name(),
                frame.blocked,
                frame.attacker_skill,
                frame.defender_skill,
                frame.attack_force,
                frame.stamina_cost,
                frame.note
            );
            println!();
        }
    } else {
        println!("{}", serde_json::to_string_pretty(&frames).unwrap());
    }
}

fn render_swordplay_ascii(
    step: usize,
    motion: AttackMotion,
    block: BlockManeuver,
    blocked: bool,
) -> String {
    let attack_icon = match motion {
        AttackMotion::Generic => '?',
        AttackMotion::Overhead => '^',
        AttackMotion::Forehand => '>',
        AttackMotion::Backhand => '<',
        AttackMotion::Thrust => '-',
    };
    let block_icon = match block {
        BlockManeuver::Generic => '?',
        BlockManeuver::HighGuard => 'H',
        BlockManeuver::InsideParry => 'I',
        BlockManeuver::OutsideParry => 'O',
        BlockManeuver::LowGuard => 'L',
    };
    let result_icon = if blocked { '#' } else { '!' };

    format!("Step {step}\n........\n..A{attack_icon}{result_icon}D{block_icon}.\n........")
}

fn write_swordplay_drill_replay(
    path: &Path,
    steps: &[SwordplayDrillStep],
    frames: &[SwordplayDrillFrame],
) {
    let mut state = swordplay_drill_replay_state();
    let mut file = std::fs::File::create(path).expect("failed to create swordplay replay file");
    let agent_names = vec![
        "sword-drill-attacker".to_string(),
        "sword-drill-defender".to_string(),
    ];
    let agent_versions = vec!["v3-sword-drill".to_string(), "v3-sword-drill".to_string()];
    let init = v3_protocol::build_init(&state, &agent_names, &agent_versions, 0);
    let init_msg = v3_protocol::V3ServerToSpectator::Init { init };
    writeln!(file, "{}", serde_json::to_string(&init_msg).unwrap()).unwrap();

    let mut delta_tracker = v3_protocol::DeltaTracker::new();
    let snapshot = v3_protocol::build_snapshot(&state, 1.0);
    let snapshot_msg = v3_protocol::V3ServerToSpectator::Snapshot { snapshot };
    writeln!(file, "{}", serde_json::to_string(&snapshot_msg).unwrap()).unwrap();
    let _ = delta_tracker.build_delta(&mut state, 1.0);

    for (idx, (step, frame)) in steps.iter().zip(frames).enumerate() {
        apply_swordplay_step_to_replay(&mut state, step, frame, idx as u64 + 1);
        let delta = delta_tracker.build_delta(&mut state, 1.0);
        let msg = v3_protocol::V3ServerToSpectator::SnapshotDelta { delta };
        writeln!(file, "{}", serde_json::to_string(&msg).unwrap()).unwrap();
    }
}

fn swordplay_drill_replay_state() -> GameState {
    let hf = Heightfield::new(8, 4, 0.0, GeoMaterial::Soil);
    let mut state = GameState::new(8, 4, 2, hf);
    let attacker_pos = hex_to_world(offset_to_axial(1, 2));
    let defender_pos = hex_to_world(offset_to_axial(1, 4));
    let attacker = spawn_entity(
        &mut state,
        EntityBuilder::new()
            .pos(attacker_pos)
            .owner(0)
            .person(Person {
                role: Role::Soldier,
                combat_skill: 0.9,
                task: None,
            })
            .mobile(Mobile::new(2.0, 10.0))
            .combatant(Combatant::new())
            .vitals()
            .equipment(Equipment::empty()),
    );
    let defender = spawn_entity(
        &mut state,
        EntityBuilder::new()
            .pos(defender_pos)
            .owner(1)
            .person(Person {
                role: Role::Soldier,
                combat_skill: 0.9,
                task: None,
            })
            .mobile(Mobile::new(2.0, 10.0))
            .combatant(Combatant::new())
            .vitals()
            .equipment(Equipment::empty()),
    );

    let sword_a = spawn_entity(
        &mut state,
        EntityBuilder::new()
            .owner(0)
            .weapon_props(weapon::iron_sword()),
    );
    contain(&mut state, attacker, sword_a);
    state.entities[attacker].equipment.as_mut().unwrap().weapon = Some(sword_a);

    let sword_b = spawn_entity(
        &mut state,
        EntityBuilder::new()
            .owner(1)
            .weapon_props(weapon::iron_sword()),
    );
    contain(&mut state, defender, sword_b);
    state.entities[defender].equipment.as_mut().unwrap().weapon = Some(sword_b);

    state.entities[attacker].combatant.as_mut().unwrap().target = Some(defender);
    state.entities[attacker].combatant.as_mut().unwrap().facing = 0.0;
    state.entities[defender].combatant.as_mut().unwrap().target = Some(attacker);
    state.entities[defender].combatant.as_mut().unwrap().facing = std::f32::consts::PI;
    state
}

fn apply_swordplay_step_to_replay(
    state: &mut GameState,
    step: &SwordplayDrillStep,
    frame: &SwordplayDrillFrame,
    tick: u64,
) {
    let mut people = state
        .entities
        .keys()
        .filter(|&key| state.entities[key].person.is_some());
    let attacker = people.next().expect("attacker exists");
    let defender = people.next().expect("defender exists");
    let attacker_weapon = state.entities[attacker]
        .equipment
        .as_ref()
        .and_then(|eq| eq.weapon)
        .expect("attacker weapon exists");

    state.tick = tick;
    state.entities[attacker]
        .person
        .as_mut()
        .unwrap()
        .combat_skill = step.attacker_skill;
    state.entities[defender]
        .person
        .as_mut()
        .unwrap()
        .combat_skill = step.defender_skill;
    state.entities[attacker].combatant.as_mut().unwrap().facing = match step.motion {
        AttackMotion::Overhead => 0.0,
        AttackMotion::Forehand => 0.18,
        AttackMotion::Backhand => -0.18,
        AttackMotion::Thrust => 0.0,
        AttackMotion::Generic => 0.0,
    };
    state.entities[defender].combatant.as_mut().unwrap().facing = std::f32::consts::PI;
    state.entities[attacker].combatant.as_mut().unwrap().attack = Some(AttackState::for_melee(
        defender,
        attacker_weapon,
        step.motion,
        step.attacker_skill,
    ));
    state.entities[defender]
        .combatant
        .as_mut()
        .unwrap()
        .cooldown = None;
    state.entities[attacker]
        .combatant
        .as_mut()
        .unwrap()
        .cooldown = None;

    let defender_vitals = state.entities[defender].vitals.as_mut().unwrap();
    defender_vitals.stamina = (1.0 - frame.stamina_cost * 0.6).clamp(0.2, 1.0);
    if !frame.blocked {
        defender_vitals.blood = (defender_vitals.blood - 0.18).max(0.35);
        state.entities[defender]
            .wounds
            .as_mut()
            .unwrap()
            .push(Wound {
                zone: swordplay_motion_zone(step.motion),
                severity: Severity::Laceration,
                bleed_rate: 0.005,
                damage_type: if step.motion == AttackMotion::Thrust {
                    DamageType::Pierce
                } else {
                    DamageType::Slash
                },
                attacker_id: attacker,
                created_at: tick,
            });
    }
}

fn swordplay_motion_zone(motion: AttackMotion) -> BodyZone {
    match motion {
        AttackMotion::Generic | AttackMotion::Thrust => BodyZone::Torso,
        AttackMotion::Overhead => BodyZone::Head,
        AttackMotion::Forehand => BodyZone::RightArm,
        AttackMotion::Backhand => BodyZone::LeftArm,
    }
}

fn run_ascii_game(
    seed: u64,
    agent_names: &[&str],
    max_ticks: u64,
    (w, h): (usize, usize),
    num_players: u8,
) {
    let mut state = init_bench_state(w, h, num_players, seed);
    let mut agents: Vec<LayeredAgent> = agent_names
        .iter()
        .enumerate()
        .map(|(i, &name)| make_agent(name, i as u8))
        .collect();

    while state.tick < max_ticks {
        if is_game_over(&state).is_some() {
            break;
        }

        let _phase = sim::run_agent_phase(&mut state, &mut agents);
        sim::tick(&mut state, 1.0);
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
    let mut state = init_bench_state(w, h, num_players, seed);
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

        sim::apply_agent_outputs(&mut state, &outputs);

        let tick_result = sim::tick(&mut state, 1.0);
        total_deaths += tick_result.deaths;
        tick_count += 1;

        if state.tick.is_multiple_of(SNAP_INTERVAL) || is_game_over(&state).is_some() {
            let snap = take_snapshot(&state, state.tick, num_players);

            // Track lead changes by entity count.
            let leader = snap
                .entities
                .iter()
                .enumerate()
                .filter(|(i, _)| snap.alive[*i])
                .max_by_key(|(_, e)| *e)
                .map(|(i, _)| i as u8);

            if let (Some(prev), Some(curr)) = (prev_leader, leader)
                && prev != curr
            {
                lead_changes += 1;
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
// Arena: config-driven scenarios
// ---------------------------------------------------------------------------

fn run_arena(config: &ArenaConfigFile, replay_path: Option<&str>) {
    let scenario = config.resolve();
    eprintln!("{}", scenario.title);
    eprintln!(
        "P0 = {} ({} soldiers, {:?}, {:?} {:.0}%)",
        scenario.side_a.agent,
        scenario.side_a.soldiers,
        scenario.side_a.weapon_preset,
        scenario.side_a.armor_preset,
        scenario.side_a.armor_ratio * 100.0,
    );
    eprintln!(
        "P1 = {} ({} soldiers, {:?}, {:?} {:.0}%)\n",
        scenario.side_b.agent,
        scenario.side_b.soldiers,
        scenario.side_b.weapon_preset,
        scenario.side_b.armor_preset,
        scenario.side_b.armor_ratio * 100.0,
    );

    let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
    let mut state = GameState::new(20, 20, 2, hf);
    let mut rng = StdRng::seed_from_u64(0xA63E_0F11);

    let side_a = spawn_arena_side(
        &mut state,
        &scenario.side_a,
        scenario.cluster_radius_m,
        &mut rng,
    );
    let side_b = spawn_arena_side(
        &mut state,
        &scenario.side_b,
        scenario.cluster_radius_m,
        &mut rng,
    );

    let mut agents = [
        make_agent(&scenario.side_a.agent, 0),
        make_agent(&scenario.side_b.agent, 1),
    ];

    // Replay writer
    let mut replay_file = replay_path.map(|path| {
        let mut f = std::fs::File::create(path).expect("Failed to create replay file");
        let agent_names = vec![scenario.side_a.agent.clone(), scenario.side_b.agent.clone()];
        let agent_versions = vec!["v3-arena".to_string(); 2];
        let init = v3_protocol::build_init(&state, &agent_names, &agent_versions, 0);
        let init_msg = v3_protocol::V3ServerToSpectator::Init { init };
        writeln!(f, "{}", serde_json::to_string(&init_msg).unwrap()).unwrap();
        eprintln!("  [replay] Writing to {}", path);
        f
    });
    let mut delta_tracker = v3_protocol::DeltaTracker::new();

    for t in 0..scenario.max_ticks {
        let summary_a = summarize_arena_side(&state, &side_a.members);
        let summary_b = summarize_arena_side(&state, &side_b.members);
        let dist = average_inter_side_distance(&state, &side_a.members, &side_b.members);

        let should_print = t < 10 || (t < 50 && t % 5 == 0) || t % 10 == 0;
        if should_print {
            eprintln!(
                "T{:>4} | dist={:>6.1} | A alive={:>2}/{:>2} w={} avg_bl={:.2} atk={} cd={} | B alive={:>2}/{:>2} w={} avg_bl={:.2} atk={} cd={}",
                t,
                dist,
                summary_a.alive,
                summary_a.total,
                summary_a.wounds,
                summary_a.avg_blood,
                summary_a.attacking,
                summary_a.cooling_down,
                summary_b.alive,
                summary_b.total,
                summary_b.wounds,
                summary_b.avg_blood,
                summary_b.attacking,
                summary_b.cooling_down,
            );
        }

        if summary_a.alive == 0 || summary_b.alive == 0 {
            break;
        }

        let outputs: Vec<AgentOutput> = agents.iter_mut().map(|a| a.tick(&state)).collect();
        for (pi, po) in outputs.iter().enumerate() {
            if !po.operational_commands.is_empty() {
                eprintln!(
                    "  [agent] P{} ops: {} commands",
                    pi,
                    po.operational_commands.len()
                );
            }
            if !po.tactical_commands.is_empty() {
                eprintln!(
                    "  [agent] P{} tactical: {} commands ({} stacks engaged)",
                    pi,
                    po.tactical_commands.len(),
                    po.tactical_stacks
                );
            }
        }

        sim::apply_agent_outputs(&mut state, &outputs);

        let result = sim::tick(&mut state, 1.0);

        // Write replay frame
        if let Some(ref mut f) = replay_file {
            if t == 0 {
                let snap = v3_protocol::build_snapshot(&state, 1.0);
                let msg = v3_protocol::V3ServerToSpectator::Snapshot { snapshot: snap };
                writeln!(f, "{}", serde_json::to_string(&msg).unwrap()).unwrap();
            } else {
                let delta = delta_tracker.build_delta(&mut state, 1.0);
                let msg = v3_protocol::V3ServerToSpectator::SnapshotDelta { delta };
                writeln!(f, "{}", serde_json::to_string(&msg).unwrap()).unwrap();
            }
        }

        if result.impacts > 0 {
            eprintln!("  [combat] {} impacts this tick", result.impacts);
        }
        if result.deaths > 0 {
            eprintln!("  [combat] {} deaths this tick", result.deaths);
        }
    }

    let summary_a = summarize_arena_side(&state, &side_a.members);
    let summary_b = summarize_arena_side(&state, &side_b.members);
    eprintln!("\n=== Arena complete at tick {} ===", state.tick);
    eprintln!(
        "  Side A ({}): alive={}/{}, wounds={}, avg_blood={:.2}",
        side_a.agent, summary_a.alive, summary_a.total, summary_a.wounds, summary_a.avg_blood,
    );
    eprintln!(
        "  Side B ({}): alive={}/{}, wounds={}, avg_blood={:.2}",
        side_b.agent, summary_b.alive, summary_b.total, summary_b.wounds, summary_b.avg_blood,
    );

    match (summary_a.alive, summary_b.alive) {
        (0, 0) => eprintln!("  Result: MUTUAL KILL"),
        (0, _) => eprintln!("  Winner: Side B"),
        (_, 0) => eprintln!("  Winner: Side A"),
        _ => eprintln!(
            "  Result: TIME OUT — no winner after {} ticks",
            scenario.max_ticks
        ),
    }
}

fn spawn_arena_side(
    state: &mut GameState,
    side: &ArenaSideScenario,
    cluster_radius_m: f32,
    rng: &mut StdRng,
) -> ArenaSideState {
    let mut members = Vec::with_capacity(side.soldiers);
    let bow_count = match side.weapon_preset {
        ArenaWeaponPreset::Swords => 0,
        ArenaWeaponPreset::Mixed => ((side.soldiers as f32) * 0.4).round() as usize,
    };
    let armor_count = ((side.soldiers as f32) * side.armor_ratio)
        .round()
        .clamp(0.0, side.soldiers as f32) as usize;

    for i in 0..side.soldiers {
        let pos = arena_spawn_position(side.center, cluster_radius_m, rng);
        let soldier = spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(side.owner)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                    task: None,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals()
                .equipment(Equipment::empty()),
        );

        let weapon_props = if i < bow_count {
            weapon::wooden_bow()
        } else {
            weapon::iron_sword()
        };
        let weapon_key = spawn_entity(
            state,
            EntityBuilder::new()
                .owner(side.owner)
                .weapon_props(weapon_props),
        );
        contain(state, soldier, weapon_key);
        state.entities[soldier].equipment.as_mut().unwrap().weapon = Some(weapon_key);

        if i < armor_count {
            let armor_props: Option<ArmorProperties> = match side.armor_preset {
                ArenaArmorPreset::None => None,
                ArenaArmorPreset::LeatherCuirass => Some(armor::leather_cuirass()),
                ArenaArmorPreset::BronzeBreastplate => Some(armor::bronze_breastplate()),
            };
            if let Some(armor_props) = armor_props {
                let armor_key = spawn_entity(
                    state,
                    EntityBuilder::new()
                        .owner(side.owner)
                        .armor_props(armor_props.clone()),
                );
                contain(state, soldier, armor_key);
                equipment::equip_armor(
                    state.entities[soldier].equipment.as_mut().unwrap(),
                    armor_key,
                    &armor_props,
                );
            }
        }

        members.push(soldier);
    }

    let leader = members[0];
    let stack_id = state.alloc_stack_id();
    state.stacks.push(Stack {
        id: stack_id,
        owner: side.owner,
        members: members.iter().copied().collect(),
        formation: side.formation,
        leader,
    });

    ArenaSideState {
        agent: side.agent.clone(),
        members,
    }
}

fn arena_spawn_position(center: Vec3, radius: f32, rng: &mut StdRng) -> Vec3 {
    if radius <= 0.0 {
        return center;
    }
    let theta = rng.gen_range(0.0..std::f32::consts::TAU);
    let dist = rng.gen_range(0.0..radius);
    Vec3::new(
        center.x + theta.cos() * dist,
        center.y + theta.sin() * dist,
        center.z,
    )
}

fn summarize_arena_side(state: &GameState, members: &[EntityKey]) -> ArenaSideSummary {
    let mut alive = 0usize;
    let mut wounds = 0usize;
    let mut total_blood = 0.0f32;
    let mut attacking = 0usize;
    let mut cooling_down = 0usize;

    for &member_key in members {
        let Some(entity) = state.entities.get(member_key) else {
            continue;
        };
        let is_dead = entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(true);
        if !is_dead {
            alive += 1;
        }
        wounds += entity.wounds.as_ref().map(|w| w.len()).unwrap_or(0);
        total_blood += entity.vitals.as_ref().map(|v| v.blood).unwrap_or(0.0);
        if entity
            .combatant
            .as_ref()
            .and_then(|c| c.attack.as_ref())
            .is_some()
        {
            attacking += 1;
        }
        if entity
            .combatant
            .as_ref()
            .and_then(|c| c.cooldown.as_ref())
            .is_some()
        {
            cooling_down += 1;
        }
    }

    ArenaSideSummary {
        alive,
        total: members.len(),
        wounds,
        avg_blood: if members.is_empty() {
            0.0
        } else {
            total_blood / members.len() as f32
        },
        attacking,
        cooling_down,
    }
}

fn average_inter_side_distance(
    state: &GameState,
    side_a: &[EntityKey],
    side_b: &[EntityKey],
) -> f32 {
    let living_a: Vec<_> = side_a
        .iter()
        .filter_map(|&key| {
            let entity = state.entities.get(key)?;
            if entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(true) {
                None
            } else {
                entity.pos
            }
        })
        .collect();
    let living_b: Vec<_> = side_b
        .iter()
        .filter_map(|&key| {
            let entity = state.entities.get(key)?;
            if entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(true) {
                None
            } else {
                entity.pos
            }
        })
        .collect();

    if living_a.is_empty() || living_b.is_empty() {
        return 0.0;
    }

    let mut total = 0.0f32;
    let mut count = 0usize;
    for a in &living_a {
        for b in &living_b {
            total += (*b - *a).length();
            count += 1;
        }
    }
    total / count as f32
}

// ---------------------------------------------------------------------------
// Mechanics suite
// ---------------------------------------------------------------------------

fn run_mechanics_suite(args: &[String]) {
    let strict = args.iter().any(|a| a == "--strict");
    let artifacts_dir = flag_value(args, "--artifacts-dir").map(PathBuf::from);
    let filter = flag_value(args, "--mechanics-filter");
    let trials = flag_value(args, "--trials")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(24)
        .max(1);

    let mut scenarios = Vec::new();

    if mechanics_enabled(filter, "micro.high_ground_block_cost") {
        scenarios.push(mechanics_high_ground_block_cost());
    }
    if mechanics_enabled(filter, "micro.high_ground_head_bias") {
        scenarios.push(mechanics_high_ground_head_bias());
    }
    if mechanics_enabled(filter, "micro.wound_reduces_stamina_recovery") {
        scenarios.push(mechanics_wound_reduces_stamina_recovery());
    }
    if mechanics_enabled(filter, "micro.leg_wounds_reduce_speed") {
        scenarios.push(mechanics_leg_wounds_reduce_speed());
    }
    if mechanics_enabled(filter, "micro.projectile_skill_leads_target") {
        scenarios.push(mechanics_projectile_skill_leads_target());
    }
    if mechanics_enabled(filter, "micro.low_stamina_increases_cooldown") {
        scenarios.push(mechanics_low_stamina_increases_cooldown());
    }
    if mechanics_enabled(filter, "arena.high_ground") {
        scenarios.push(mechanics_arena_high_ground(
            artifacts_dir.as_deref(),
            trials,
        ));
    }
    if mechanics_enabled(filter, "arena.armor") {
        scenarios.push(mechanics_arena_armor(artifacts_dir.as_deref(), trials));
    }
    if mechanics_enabled(filter, "arena.injured") {
        scenarios.push(mechanics_arena_injured(artifacts_dir.as_deref(), trials));
    }
    if mechanics_enabled(filter, "arena.training_melee") {
        scenarios.push(mechanics_arena_training_melee(
            artifacts_dir.as_deref(),
            trials,
        ));
    }

    let failed = scenarios.iter().filter(|s| !s.meets_intended).count();
    let passed = scenarios.len().saturating_sub(failed);
    let suite = MechanicsSuiteResult {
        suite: "v3_mechanics",
        strict,
        passed,
        failed,
        scenarios,
        implementation_gaps: vec![
            "Arena ranged combat is not yet wired end-to-end. Bows exist and projectile math exists, but arena attacks still resolve through the melee path only.".to_string(),
            "Live movement benchmarks cannot yet validate slope, surface, or encumbrance because `sim::compute_steering_and_move` hardcodes those factors to `1.0`.".to_string(),
            "Terrain-driven high-ground advantage is still incomplete in full arena play because movement speed and positioning do not yet consume slope or dedicated arena terrain generation.".to_string(),
        ],
    };

    println!("{}", serde_json::to_string_pretty(&suite).unwrap());
    if strict && failed > 0 {
        std::process::exit(1);
    }
}

fn mechanics_enabled(filter: Option<&str>, id: &str) -> bool {
    match filter {
        Some(filter) => id.contains(filter),
        None => true,
    }
}

fn mechanics_high_ground_block_cost() -> MechanicsScenarioResult {
    let (attacker, defender) = mechanics_entity_keys();
    let vitals = Vitals::new();
    let defender_state = DefenderState {
        entity_id: defender,
        facing: 0.0,
        vitals: &vitals,
        block: Some(BlockCapability {
            arc: std::f32::consts::PI,
            efficiency: 0.3,
            maneuver: BlockManeuver::HighGuard,
            read_skill: 1.0,
        }),
        armor_at_zone: [None, None, None, None, None],
    };

    let flat = Impact {
        kinetic_energy: 1.0,
        sharpness: 0.8,
        cross_section: 0.5,
        damage_type: DamageType::Slash,
        attack_motion: AttackMotion::Overhead,
        attack_direction: std::f32::consts::PI,
        attacker_id: attacker,
        height_diff: 0.0,
        tick: 1,
    };
    let high = Impact {
        height_diff: 2.0,
        ..flat.clone()
    };

    let flat_cost = match damage::resolve_impact(&flat, &defender_state) {
        ImpactResult::Blocked { stamina_cost, .. } => stamina_cost,
        other => panic!("expected blocked result for flat impact, got {:?}", other),
    };
    let high_cost = match damage::resolve_impact(&high, &defender_state) {
        ImpactResult::Blocked { stamina_cost, .. } => stamina_cost,
        other => panic!("expected blocked result for high impact, got {:?}", other),
    };

    let mut metrics = BTreeMap::new();
    metrics.insert("flat_block_cost".to_string(), flat_cost as f64);
    metrics.insert("high_ground_block_cost".to_string(), high_cost as f64);
    metrics.insert(
        "cost_ratio".to_string(),
        (high_cost / flat_cost.max(f32::EPSILON)) as f64,
    );

    MechanicsScenarioResult {
        id: "micro.high_ground_block_cost".to_string(),
        level: "micro",
        description: "Direct damage-pipeline check for downhill block pressure.".to_string(),
        intended_effect: "Defending against a higher attacker should cost more stamina to block."
            .to_string(),
        observed_effect: format!(
            "Block cost rises from {:.3} on flat ground to {:.3} with +2.0m height advantage.",
            flat_cost, high_cost
        ),
        meets_intended: high_cost > flat_cost,
        metrics,
        artifact_paths: Vec::new(),
        notes: vec![
            "This is wired today through `damage::resolve_impact` height-modified block cost."
                .to_string(),
        ],
    }
}

fn mechanics_high_ground_head_bias() -> MechanicsScenarioResult {
    let (attacker, defender) = mechanics_entity_keys();
    let vitals = Vitals::new();
    let defender_state = DefenderState {
        entity_id: defender,
        facing: 0.0,
        vitals: &vitals,
        block: None,
        armor_at_zone: [None, None, None, None, None],
    };

    let mut high_head_hits = 0u32;
    let mut low_head_hits = 0u32;
    let samples = 256u32;

    for tick in 0..samples {
        let uphill = Impact {
            kinetic_energy: 12.0,
            sharpness: 0.8,
            cross_section: 0.5,
            damage_type: DamageType::Slash,
            attack_motion: AttackMotion::Overhead,
            attack_direction: 0.0,
            attacker_id: attacker,
            height_diff: 2.0,
            tick: tick as u64,
        };
        let downhill = Impact {
            height_diff: -2.0,
            ..uphill.clone()
        };

        if let ImpactResult::Wounded { wound, .. } =
            damage::resolve_impact(&uphill, &defender_state)
            && wound.zone == BodyZone::Head
        {
            high_head_hits += 1;
        }
        if let ImpactResult::Wounded { wound, .. } =
            damage::resolve_impact(&downhill, &defender_state)
            && wound.zone == BodyZone::Head
        {
            low_head_hits += 1;
        }
    }

    let high_rate = high_head_hits as f64 / samples as f64;
    let low_rate = low_head_hits as f64 / samples as f64;
    let mut metrics = BTreeMap::new();
    metrics.insert("high_ground_head_rate".to_string(), high_rate);
    metrics.insert("low_ground_head_rate".to_string(), low_rate);
    metrics.insert("rate_delta".to_string(), high_rate - low_rate);

    MechanicsScenarioResult {
        id: "micro.high_ground_head_bias".to_string(),
        level: "micro",
        description:
            "Repeated deterministic impacts to measure height-biased hit-location distribution."
                .to_string(),
        intended_effect: "Higher attackers should bias hit location upward.".to_string(),
        observed_effect: format!(
            "Head-hit rate is {:.1}% with +2.0m advantage vs {:.1}% with -2.0m.",
            high_rate * 100.0,
            low_rate * 100.0
        ),
        meets_intended: high_rate > low_rate,
        metrics,
        artifact_paths: Vec::new(),
        notes: vec![
            "This uses the same hash-deterministic roll path as live damage resolution."
                .to_string(),
        ],
    }
}

fn mechanics_wound_reduces_stamina_recovery() -> MechanicsScenarioResult {
    let attacker = mechanics_entity_keys().0;
    let wound = Wound {
        zone: BodyZone::Torso,
        severity: Severity::Laceration,
        bleed_rate: 0.005,
        damage_type: DamageType::Slash,
        attacker_id: attacker,
        created_at: 0,
    };
    let mut fresh = Vitals::new();
    let mut wounded = Vitals::new();
    fresh.stamina = 0.0;
    wounded.stamina = 0.0;

    fresh.tick_stamina_recovery(&[], 1.0);
    wounded.tick_stamina_recovery(&[wound], 1.0);

    let mut metrics = BTreeMap::new();
    metrics.insert("fresh_recovery".to_string(), fresh.stamina as f64);
    metrics.insert("wounded_recovery".to_string(), wounded.stamina as f64);

    MechanicsScenarioResult {
        id: "micro.wound_reduces_stamina_recovery".to_string(),
        level: "micro",
        description: "Vitals recovery check for wounded vs unwounded soldier.".to_string(),
        intended_effect: "A wounded entity should recover stamina more slowly.".to_string(),
        observed_effect: format!(
            "Unwounded recovery is {:.3} stamina/tick vs {:.3} with a torso laceration.",
            fresh.stamina, wounded.stamina
        ),
        meets_intended: wounded.stamina < fresh.stamina,
        metrics,
        artifact_paths: Vec::new(),
        notes: vec![
            "Torso wounds carry the heaviest stamina-recovery penalty in current V3.".to_string(),
        ],
    }
}

fn mechanics_leg_wounds_reduce_speed() -> MechanicsScenarioResult {
    let healthy = simulate_everything_engine::v3::movement::SpeedFactors {
        base_capability: 3.0,
        slope_factor: 1.0,
        surface_factor: 1.0,
        encumbrance_factor: 1.0,
        wound_factor: simulate_everything_engine::v3::movement::wound_factor(0.0),
        stamina_factor: simulate_everything_engine::v3::movement::stamina_factor(1.0),
    }
    .derived_speed();
    let wounded = simulate_everything_engine::v3::movement::SpeedFactors {
        base_capability: 3.0,
        slope_factor: 1.0,
        surface_factor: 1.0,
        encumbrance_factor: 1.0,
        wound_factor: simulate_everything_engine::v3::movement::wound_factor(0.6),
        stamina_factor: simulate_everything_engine::v3::movement::stamina_factor(1.0),
    }
    .derived_speed();

    let mut metrics = BTreeMap::new();
    metrics.insert("healthy_speed".to_string(), healthy as f64);
    metrics.insert("leg_wounded_speed".to_string(), wounded as f64);
    metrics.insert(
        "speed_ratio".to_string(),
        (wounded / healthy.max(f32::EPSILON)) as f64,
    );

    MechanicsScenarioResult {
        id: "micro.leg_wounds_reduce_speed".to_string(),
        level: "micro",
        description: "Movement speed derivation under leg-wound penalty.".to_string(),
        intended_effect: "Leg wounds should reduce derived movement speed.".to_string(),
        observed_effect: format!(
            "Derived speed drops from {:.2} to {:.2} with leg wound weight 0.6.",
            healthy, wounded
        ),
        meets_intended: wounded < healthy,
        metrics,
        artifact_paths: Vec::new(),
        notes: vec!["This is active in live movement because `sim::compute_steering_and_move` already feeds leg wound weight into `wound_factor`.".to_string()],
    }
}

fn mechanics_projectile_skill_leads_target() -> MechanicsScenarioResult {
    let target_pos = Vec3::new(30.0, 0.0, 1.0);
    let target_vel = Vec3::new(1.0, 0.5, 0.0);
    let predicted = target_pos + target_vel * (30.0 / 50.0);
    let no_skill = projectile::compute_aim_pos(target_pos, target_vel, 30.0, 50.0, 0.0);
    let high_skill = projectile::compute_aim_pos(target_pos, target_vel, 30.0, 50.0, 1.0);
    let no_skill_err = (predicted - no_skill).length() as f64;
    let high_skill_err = (predicted - high_skill).length() as f64;

    let mut metrics = BTreeMap::new();
    metrics.insert("no_skill_error".to_string(), no_skill_err);
    metrics.insert("high_skill_error".to_string(), high_skill_err);

    MechanicsScenarioResult {
        id: "micro.projectile_skill_leads_target".to_string(),
        level: "micro",
        description: "Projectile aiming interpolation check.".to_string(),
        intended_effect: "Higher combat skill should lead moving targets more accurately.".to_string(),
        observed_effect: format!(
            "Predicted-target error falls from {:.3} at skill 0.0 to {:.3} at skill 1.0.",
            no_skill_err, high_skill_err
        ),
        meets_intended: high_skill_err < no_skill_err,
        metrics,
        artifact_paths: Vec::new(),
        notes: vec!["This is implemented in projectile aim math, but not yet exercised by live arena combat because ranged attack resolution is incomplete.".to_string()],
    }
}

fn mechanics_low_stamina_increases_cooldown() -> MechanicsScenarioResult {
    let sword = weapon::iron_sword();
    let full = weapon::compute_cooldown(&sword, 1.0) as f64;
    let exhausted = weapon::compute_cooldown(&sword, 0.2) as f64;
    let mut metrics = BTreeMap::new();
    metrics.insert("full_stamina_cooldown".to_string(), full);
    metrics.insert("low_stamina_cooldown".to_string(), exhausted);

    MechanicsScenarioResult {
        id: "micro.low_stamina_increases_cooldown".to_string(),
        level: "micro",
        description: "Weapon recovery timing under stamina loss.".to_string(),
        intended_effect: "Lower stamina should lengthen recovery after an attack.".to_string(),
        observed_effect: format!(
            "Sword cooldown rises from {:.0} ticks at full stamina to {:.0} ticks at 0.2 stamina.",
            full, exhausted
        ),
        meets_intended: exhausted > full,
        metrics,
        artifact_paths: Vec::new(),
        notes: vec!["This effect is live today because melee cooldown reads current stamina after each attack.".to_string()],
    }
}

fn mechanics_arena_high_ground(
    artifacts_dir: Option<&Path>,
    trials: usize,
) -> MechanicsScenarioResult {
    let base = mechanics_base_arena("arena.high_ground", 1);
    paired_arena_result(
        "arena.high_ground",
        "Mirrored arena duel with only starting elevation swapped.",
        "The side on higher ground should perform better in mirrored melee matchups.",
        ArenaMechanicVariant {
            id: "high_ground_a".to_string(),
            description: "Side A starts 4m above Side B.".to_string(),
            scenario: base.clone(),
            post_setup: ArenaPostSetup {
                side_a_z_offset: 4.0,
                ..Default::default()
            },
        },
        ArenaMechanicVariant {
            id: "high_ground_b".to_string(),
            description: "Side B starts 4m above Side A.".to_string(),
            scenario: base,
            post_setup: ArenaPostSetup {
                side_b_z_offset: 4.0,
                ..Default::default()
            },
        },
        artifacts_dir,
        ArenaExpectation::AdvantagedWins,
        trials,
        vec!["Current V3 uses `pos.z` as the live melee height-difference source, so this benchmark is already meaningful even though terrain slope is not yet fed into movement speed.".to_string()],
    )
}

fn mechanics_arena_armor(artifacts_dir: Option<&Path>, trials: usize) -> MechanicsScenarioResult {
    let mut armored_a = mechanics_base_arena("arena.armor", 1);
    armored_a.side_a.armor_preset = ArenaArmorPreset::BronzeBreastplate;
    armored_a.side_a.armor_ratio = 1.0;

    let mut armored_b = mechanics_base_arena("arena.armor", 1);
    armored_b.side_b.armor_preset = ArenaArmorPreset::BronzeBreastplate;
    armored_b.side_b.armor_ratio = 1.0;

    paired_arena_result(
        "arena.armor",
        "Mirrored arena duel with one side fully armored.",
        "Armored soldiers should survive better than otherwise identical unarmored soldiers.",
        ArenaMechanicVariant {
            id: "armor_a".to_string(),
            description: "Side A has bronze breastplates.".to_string(),
            scenario: armored_a,
            post_setup: ArenaPostSetup::default(),
        },
        ArenaMechanicVariant {
            id: "armor_b".to_string(),
            description: "Side B has bronze breastplates.".to_string(),
            scenario: armored_b,
            post_setup: ArenaPostSetup::default(),
        },
        artifacts_dir,
        ArenaExpectation::AdvantagedWins,
        trials,
        vec!["This validates end-to-end armor effects through the live damage pipeline, not just the theoretical damage table.".to_string()],
    )
}

fn mechanics_arena_injured(artifacts_dir: Option<&Path>, trials: usize) -> MechanicsScenarioResult {
    let attacker = mechanics_entity_keys().0;
    let injury = Wound {
        zone: BodyZone::Legs,
        severity: Severity::Laceration,
        bleed_rate: 0.003,
        damage_type: DamageType::Slash,
        attacker_id: attacker,
        created_at: 0,
    };

    paired_arena_result(
        "arena.injured",
        "Mirrored arena duel with one side pre-injured.",
        "The injured side should lose ground against a fresh side.",
        ArenaMechanicVariant {
            id: "injured_a".to_string(),
            description: "Side A starts wounded and bloodied.".to_string(),
            scenario: mechanics_base_arena("arena.injured", 1),
            post_setup: ArenaPostSetup {
                side_a_blood: Some(0.55),
                side_a_stamina: Some(0.35),
                side_a_start_wounds: vec![injury.clone()],
                ..Default::default()
            },
        },
        ArenaMechanicVariant {
            id: "injured_b".to_string(),
            description: "Side B starts wounded and bloodied.".to_string(),
            scenario: mechanics_base_arena("arena.injured", 1),
            post_setup: ArenaPostSetup {
                side_b_blood: Some(0.55),
                side_b_stamina: Some(0.35),
                side_b_start_wounds: vec![injury],
                ..Default::default()
            },
        },
        artifacts_dir,
        ArenaExpectation::AdvantagedLoses,
        trials,
        vec!["This exercises live bleed, stamina recovery, and leg-wound movement penalties together.".to_string()],
    )
}

fn mechanics_arena_training_melee(
    artifacts_dir: Option<&Path>,
    trials: usize,
) -> MechanicsScenarioResult {
    paired_arena_result(
        "arena.training_melee",
        "Mirrored melee arena duel with only combat_skill swapped.",
        "Better-trained melee soldiers should outperform worse-trained ones.",
        ArenaMechanicVariant {
            id: "training_a".to_string(),
            description: "Side A combat_skill 0.9, Side B combat_skill 0.1.".to_string(),
            scenario: mechanics_base_arena("arena.training_melee", 1),
            post_setup: ArenaPostSetup {
                side_a_skill: Some(0.9),
                side_b_skill: Some(0.1),
                ..Default::default()
            },
        },
        ArenaMechanicVariant {
            id: "training_b".to_string(),
            description: "Side B combat_skill 0.9, Side A combat_skill 0.1.".to_string(),
            scenario: mechanics_base_arena("arena.training_melee", 1),
            post_setup: ArenaPostSetup {
                side_a_skill: Some(0.1),
                side_b_skill: Some(0.9),
                ..Default::default()
            },
        },
        artifacts_dir,
        ArenaExpectation::AdvantagedWins,
        trials,
        vec![
            "Sword training now changes move selection, windup, and reactive blocking rather than sitting idle on `Person`.".to_string(),
            "Use `--trials N` here for stable tuning; single-duel results are intentionally replaced by aggregated win-rate and score outputs.".to_string(),
        ],
    )
}

fn paired_arena_result(
    id: &str,
    description: &str,
    intended_effect: &str,
    variant_a: ArenaMechanicVariant,
    variant_b: ArenaMechanicVariant,
    artifacts_dir: Option<&Path>,
    expectation: ArenaExpectation,
    trials: usize,
    notes: Vec<String>,
) -> MechanicsScenarioResult {
    let aggregate_a = aggregate_arena_trials(&variant_a, 0, expectation, trials);
    let aggregate_b = aggregate_arena_trials(&variant_b, 1, expectation, trials);
    let score_a = aggregate_a.mean_advantaged_score;
    let score_b = aggregate_b.mean_advantaged_score;
    let avg_score = (score_a + score_b) / 2.0;
    let meets = match expectation {
        ArenaExpectation::AdvantagedWins => {
            score_a > 0.0
                && score_b > 0.0
                && aggregate_a.success_rate > 0.5
                && aggregate_b.success_rate > 0.5
        }
        ArenaExpectation::AdvantagedLoses => {
            score_a < 0.0
                && score_b < 0.0
                && aggregate_a.success_rate > 0.5
                && aggregate_b.success_rate > 0.5
        }
        ArenaExpectation::SpecGap => false,
    };

    let mut artifact_paths = Vec::new();
    if let Some(dir) = artifacts_dir {
        if let Ok(path) = write_arena_artifact(dir, id, &variant_a.id, &aggregate_a.sample) {
            artifact_paths.push(path.display().to_string());
        }
        if let Ok(path) = write_arena_artifact(dir, id, &variant_b.id, &aggregate_b.sample) {
            artifact_paths.push(path.display().to_string());
        }
    }

    let mut metrics = BTreeMap::new();
    metrics.insert("trials".to_string(), trials as f64);
    metrics.insert("variant_a_advantaged_score".to_string(), score_a);
    metrics.insert("variant_b_advantaged_score".to_string(), score_b);
    metrics.insert("avg_advantaged_score".to_string(), avg_score);
    metrics.insert(
        "variant_a_success_rate".to_string(),
        aggregate_a.success_rate,
    );
    metrics.insert(
        "variant_b_success_rate".to_string(),
        aggregate_b.success_rate,
    );
    metrics.insert("variant_a_draw_rate".to_string(), aggregate_a.draw_rate);
    metrics.insert("variant_b_draw_rate".to_string(), aggregate_b.draw_rate);

    MechanicsScenarioResult {
        id: id.to_string(),
        level: "arena",
        description: description.to_string(),
        intended_effect: intended_effect.to_string(),
        observed_effect: format!(
            "Across {} trials per mirror, advantaged-side mean scores are {:.2} and {:.2}; success rates are {:.1}% and {:.1}%.",
            trials,
            score_a,
            score_b,
            aggregate_a.success_rate * 100.0,
            aggregate_b.success_rate * 100.0,
        ),
        meets_intended: meets,
        metrics,
        artifact_paths,
        notes,
    }
}

fn aggregate_arena_trials(
    variant: &ArenaMechanicVariant,
    advantaged_owner: u8,
    expectation: ArenaExpectation,
    trials: usize,
) -> ArenaAggregate {
    let results: Vec<ArenaArtifact> = (0..trials.max(1))
        .into_par_iter()
        .map(|trial| {
            let mirror_geometry = trial % 2 == 1;
            let run_variant = if mirror_geometry {
                mirrored_geometry_variant(variant)
            } else {
                variant.clone()
            };
            simulate_arena_variant(
                &run_variant,
                false,
                arena_trial_seed(&variant.id, trial as u64),
            )
        })
        .collect();
    let sample = simulate_arena_variant(variant, true, arena_trial_seed(&variant.id, 0));

    let mut total_score = 0.0;
    let mut successes = 0usize;
    let mut draws = 0usize;
    for result in &results {
        let advantaged_score = if advantaged_owner == 0 {
            arena_strength(result.side_a) - arena_strength(result.side_b)
        } else {
            arena_strength(result.side_b) - arena_strength(result.side_a)
        };
        total_score += advantaged_score;
        match result.winner {
            Some(winner) => {
                if arena_winner_matches_expectation(winner, advantaged_owner, expectation) {
                    successes += 1;
                }
            }
            None => draws += 1,
        }
    }

    let denom = results.len().max(1) as f64;
    ArenaAggregate {
        mean_advantaged_score: total_score / denom,
        success_rate: successes as f64 / denom,
        draw_rate: draws as f64 / denom,
        sample,
    }
}

fn arena_winner_matches_expectation(
    winner: u8,
    advantaged_owner: u8,
    expectation: ArenaExpectation,
) -> bool {
    match expectation {
        ArenaExpectation::AdvantagedWins => winner == advantaged_owner,
        ArenaExpectation::AdvantagedLoses => winner != advantaged_owner,
        ArenaExpectation::SpecGap => false,
    }
}

fn mirrored_geometry_variant(variant: &ArenaMechanicVariant) -> ArenaMechanicVariant {
    let mut mirrored = variant.clone();
    std::mem::swap(
        &mut mirrored.scenario.side_a.center,
        &mut mirrored.scenario.side_b.center,
    );
    mirrored
}

fn arena_trial_seed(id: &str, trial: u64) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    trial.hash(&mut hasher);
    hasher.finish()
}

fn simulate_arena_variant(
    variant: &ArenaMechanicVariant,
    capture_timeline: bool,
    seed: u64,
) -> ArenaArtifact {
    let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
    let mut state = GameState::new(20, 20, 2, hf);
    let mut rng = StdRng::seed_from_u64(seed);

    let side_a = spawn_arena_side(
        &mut state,
        &variant.scenario.side_a,
        variant.scenario.cluster_radius_m,
        &mut rng,
    );
    let side_b = spawn_arena_side(
        &mut state,
        &variant.scenario.side_b,
        variant.scenario.cluster_radius_m,
        &mut rng,
    );

    apply_arena_post_setup(
        &mut state,
        &side_a.members,
        &side_b.members,
        &variant.post_setup,
    );

    let mut agents = vec![
        make_agent(&variant.scenario.side_a.agent, 0),
        make_agent(&variant.scenario.side_b.agent, 1),
    ];
    let mut timeline = Vec::new();

    if capture_timeline {
        timeline.push(record_arena_frame(
            &mut state,
            &side_a.members,
            &side_b.members,
        ));
    }

    for _ in 0..variant.scenario.max_ticks {
        let summary_a = summarize_arena_side(&state, &side_a.members);
        let summary_b = summarize_arena_side(&state, &side_b.members);
        if summary_a.alive == 0 || summary_b.alive == 0 {
            break;
        }

        let _phase = sim::run_agent_phase(&mut state, &mut agents);

        let _ = sim::tick(&mut state, 1.0);
        if capture_timeline {
            timeline.push(record_arena_frame(
                &mut state,
                &side_a.members,
                &side_b.members,
            ));
        } else {
            let _ = state.combat_log.drain();
        }
    }

    let side_a_summary = summarize_arena_side(&state, &side_a.members);
    let side_b_summary = summarize_arena_side(&state, &side_b.members);
    ArenaArtifact {
        id: variant.id.clone(),
        description: variant.description.clone(),
        winner: arena_winner(side_a_summary, side_b_summary),
        final_tick: state.tick,
        side_a: side_a_summary,
        side_b: side_b_summary,
        timeline,
    }
}

fn apply_arena_post_setup(
    state: &mut GameState,
    side_a_members: &[EntityKey],
    side_b_members: &[EntityKey],
    post_setup: &ArenaPostSetup,
) {
    apply_side_post_setup(
        state,
        side_a_members,
        post_setup.side_a_z_offset,
        post_setup.side_a_skill,
        post_setup.side_a_blood,
        post_setup.side_a_stamina,
        post_setup.side_a_movement_mode,
        &post_setup.side_a_start_wounds,
    );
    apply_side_post_setup(
        state,
        side_b_members,
        post_setup.side_b_z_offset,
        post_setup.side_b_skill,
        post_setup.side_b_blood,
        post_setup.side_b_stamina,
        post_setup.side_b_movement_mode,
        &post_setup.side_b_start_wounds,
    );
}

fn apply_side_post_setup(
    state: &mut GameState,
    members: &[EntityKey],
    z_offset: f32,
    skill: Option<f32>,
    blood: Option<f32>,
    stamina: Option<f32>,
    movement_mode: Option<MovementMode>,
    start_wounds: &[Wound],
) {
    for &member in members {
        let Some(entity) = state.entities.get_mut(member) else {
            continue;
        };
        if let Some(pos) = entity.pos.as_mut() {
            pos.z += z_offset;
        }
        if let Some(skill) = skill
            && let Some(person) = entity.person.as_mut()
        {
            person.combat_skill = skill;
        }
        if let Some(vitals) = entity.vitals.as_mut() {
            if let Some(blood) = blood {
                vitals.blood = blood;
            }
            if let Some(stamina) = stamina {
                vitals.stamina = stamina;
            }
            if let Some(mode) = movement_mode {
                vitals.movement_mode = mode;
            }
        }
        if !start_wounds.is_empty() {
            let wounds = entity.wounds.get_or_insert_with(Default::default);
            wounds.extend(start_wounds.iter().cloned());
        }
    }
}

fn record_arena_frame(
    state: &mut GameState,
    side_a_members: &[EntityKey],
    side_b_members: &[EntityKey],
) -> ArenaTimelineFrame {
    ArenaTimelineFrame {
        tick: state.tick,
        avg_distance: average_inter_side_distance(state, side_a_members, side_b_members),
        side_a: summarize_arena_side(state, side_a_members),
        side_b: summarize_arena_side(state, side_b_members),
        soldiers: collect_arena_units(state, side_a_members, side_b_members),
        combat_log: state.combat_log.drain(),
    }
}

fn collect_arena_units(
    state: &GameState,
    side_a_members: &[EntityKey],
    side_b_members: &[EntityKey],
) -> Vec<ArenaUnitFrame> {
    side_a_members
        .iter()
        .chain(side_b_members.iter())
        .filter_map(|&key| {
            let entity = state.entities.get(key)?;
            let pos = entity.pos?;
            Some(ArenaUnitFrame {
                id: entity.id,
                owner: entity.owner.unwrap_or(255),
                x: pos.x,
                y: pos.y,
                z: pos.z,
                blood: entity.vitals.as_ref().map(|v| v.blood).unwrap_or(0.0),
                stamina: entity.vitals.as_ref().map(|v| v.stamina).unwrap_or(0.0),
                alive: entity
                    .vitals
                    .as_ref()
                    .map(|v| !v.is_dead())
                    .unwrap_or(false),
                wounds: entity.wounds.as_ref().map(|w| w.len()).unwrap_or(0),
                combat_skill: entity
                    .person
                    .as_ref()
                    .map(|p| p.combat_skill)
                    .unwrap_or(0.0),
            })
        })
        .collect()
}

fn mechanics_base_arena(id: &str, soldiers: usize) -> ArenaScenario {
    ArenaScenario {
        title: format!("=== V3 Mechanics Arena: {} ===", id),
        max_ticks: 160,
        cluster_radius_m: 0.0,
        side_a: ArenaSideScenario {
            owner: 0,
            agent: "striker".to_string(),
            soldiers,
            weapon_preset: ArenaWeaponPreset::Swords,
            armor_preset: ArenaArmorPreset::None,
            armor_ratio: 0.0,
            formation: FormationType::Line,
            center: Vec3::new(50.0, 50.0, 0.0),
        },
        side_b: ArenaSideScenario {
            owner: 1,
            agent: "striker".to_string(),
            soldiers,
            weapon_preset: ArenaWeaponPreset::Swords,
            armor_preset: ArenaArmorPreset::None,
            armor_ratio: 0.0,
            formation: FormationType::Line,
            center: Vec3::new(64.0, 50.0, 0.0),
        },
    }
}

fn mechanics_entity_keys() -> (EntityKey, EntityKey) {
    let mut state = GameState::new(2, 2, 2, Heightfield::new(2, 2, 0.0, GeoMaterial::Soil));
    let attacker = spawn_entity(&mut state, EntityBuilder::new().owner(0));
    let defender = spawn_entity(&mut state, EntityBuilder::new().owner(1));
    (attacker, defender)
}

fn arena_strength(summary: ArenaSideSummary) -> f64 {
    summary.alive as f64 * 100.0 + summary.avg_blood as f64 * 10.0 - summary.wounds as f64
}

fn arena_winner(side_a: ArenaSideSummary, side_b: ArenaSideSummary) -> Option<u8> {
    match (side_a.alive, side_b.alive) {
        (0, 0) => None,
        (0, _) => Some(1),
        (_, 0) => Some(0),
        _ => None,
    }
}

fn write_arena_artifact(
    root: &Path,
    scenario_id: &str,
    variant_id: &str,
    artifact: &ArenaArtifact,
) -> std::io::Result<PathBuf> {
    let dir = root.join(scenario_id);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", variant_id));
    fs::write(&path, serde_json::to_vec_pretty(artifact).unwrap())?;
    Ok(path)
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
        if let Some(owner) = entity.owner
            && (owner as usize) < alive.len()
        {
            alive[owner as usize] = true;
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
        if let Some(owner) = entity.owner
            && (owner as usize) < counts.len()
        {
            counts[owner as usize] += 1;
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
        if let Some(owner) = entity.owner
            && (owner as usize) < counts.len()
        {
            counts[owner as usize] += 1;
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
        if let Some(owner) = entity.owner
            && let Some(hex) = entity.hex
        {
            hex_owners.entry((hex.q, hex.r)).or_default().push(owner);
        }
    }
    let mut territory = vec![0usize; num_players as usize];
    for owners in hex_owners.values() {
        let mut counts = vec![0u32; num_players as usize];
        for &o in owners {
            if (o as usize) < counts.len() {
                counts[o as usize] += 1;
            }
        }
        if let Some((player, &count)) = counts.iter().enumerate().max_by_key(|(_, c)| **c)
            && count > 0
        {
            territory[player] += 1;
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
    if let Some(wi) = winner_idx
        && winner_was_behind_late(wi, snapshots, max_ticks / 4)
    {
        score += 30.0;
        tags.push("comeback".into());
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
    if let Some(wi) = winner_idx
        && wi != 0
    {
        score += 5.0;
        tags.push("upset".into());
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

        if snap.tick > after_tick
            && let (Some(prev), Some(curr)) = (prev_leader, leader)
            && prev != curr
        {
            changes += 1;
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
// Personality report generation
// ---------------------------------------------------------------------------

fn build_personality_report_metadata(
    args: &[String],
    seeds: &[u64],
    max_ticks: u64,
    (w, h): (usize, usize),
    snapshot_interval: u64,
    games: usize,
) -> PersonalityReportMetadata {
    PersonalityReportMetadata {
        generated_at_epoch_s: unix_now(),
        git_head: git_head_short(),
        git_dirty: git_is_dirty(),
        command: format_cli_command(args),
        seeds: format_seed_list(seeds),
        ticks: max_ticks,
        size: format!("{}x{}", w, h),
        snapshot_interval,
        games,
    }
}

fn build_matchup_report_summary(stats: &V3MatchupStats) -> MatchupReportSummary {
    let games = stats.results.len();
    MatchupReportSummary {
        matchup: stats.agents.join("-vs-"),
        agents: stats.agents.clone(),
        games,
        wins: stats.wins.clone(),
        draws: stats.draws,
        win_rates: stats
            .wins
            .iter()
            .map(|&wins| ratio(wins as usize, games))
            .collect(),
        draw_rate: ratio(stats.draws as usize, games),
        avg_ticks: average_u64(stats.results.iter().map(|r| r.ticks)),
        avg_deaths: average_usize(stats.results.iter().map(|r| r.total_deaths)),
        avg_final_entities: average_per_slot(&stats.results, |r| &r.final_entities),
        avg_final_soldiers: average_per_slot(&stats.results, |r| &r.final_soldiers),
        avg_final_territory: average_per_slot(&stats.results, |r| &r.final_territory),
        diagnosis: diagnose_matchup(&stats.results),
    }
}

fn build_personality_summaries(results: &[V3GameResult]) -> Vec<PersonalityReportSummary> {
    #[derive(Default)]
    struct PersonalityAccumulator {
        games: usize,
        wins: u32,
        draws: u32,
        losses: u32,
        ticks: u64,
        deaths: usize,
        final_entities: f64,
        final_soldiers: f64,
        final_territory: f64,
    }

    let mut accs: BTreeMap<String, PersonalityAccumulator> = BTreeMap::new();
    for result in results {
        for (idx, personality) in result.agents.iter().enumerate() {
            let acc = accs.entry(personality.clone()).or_default();
            acc.games += 1;
            acc.ticks += result.ticks;
            acc.deaths += result.total_deaths;
            acc.final_entities += result.final_entities[idx] as f64;
            acc.final_soldiers += result.final_soldiers[idx] as f64;
            acc.final_territory += result.final_territory[idx] as f64;

            if result.draw {
                acc.draws += 1;
            } else if result.winner_idx == Some(idx as u8) {
                acc.wins += 1;
            } else {
                acc.losses += 1;
            }
        }
    }

    accs.into_iter()
        .map(|(personality, acc)| PersonalityReportSummary {
            personality,
            games: acc.games,
            wins: acc.wins,
            draws: acc.draws,
            losses: acc.losses,
            win_rate: ratio(acc.wins as usize, acc.games),
            draw_rate: ratio(acc.draws as usize, acc.games),
            avg_ticks: ratio_u64(acc.ticks, acc.games),
            avg_deaths: ratio(acc.deaths, acc.games),
            avg_final_entities: ratio_f64(acc.final_entities, acc.games),
            avg_final_soldiers: ratio_f64(acc.final_soldiers, acc.games),
            avg_final_territory: ratio_f64(acc.final_territory, acc.games),
        })
        .collect()
}

fn diagnose_matchup(results: &[V3GameResult]) -> MatchupDiagnosis {
    let zero_deaths = results.iter().all(|r| r.total_deaths == 0);
    let flat_entities = results
        .iter()
        .all(|r| series_is_flat(&r.snapshots, |s| &s.entities));
    let flat_soldiers = results
        .iter()
        .all(|r| series_is_flat(&r.snapshots, |s| &s.soldiers));
    let flat_territory = results
        .iter()
        .all(|r| series_is_flat(&r.snapshots, |s| &s.territory));
    let attrition_without_resolution = results
        .iter()
        .any(|r| r.draw && r.total_deaths > 0 && final_counts_changed(r));

    let mut notes = Vec::new();
    if zero_deaths {
        notes.push("No combat deaths recorded across sampled games.".to_string());
    }
    if flat_entities {
        notes.push("Entity counts stayed flat across snapshots.".to_string());
    }
    if flat_soldiers {
        notes.push("Soldier counts stayed flat across snapshots.".to_string());
    }
    if flat_territory {
        notes.push("Territory estimates stayed flat across snapshots.".to_string());
    }
    if attrition_without_resolution {
        notes.push("Combat causes attrition but the matchup still times out as draws.".to_string());
    }
    if notes.is_empty() {
        notes.push(
            "Matchup shows measurable differentiation without obvious stall signals.".to_string(),
        );
    }

    MatchupDiagnosis {
        zero_deaths,
        flat_entities,
        flat_soldiers,
        flat_territory,
        attrition_without_resolution,
        notes,
    }
}

fn persist_personality_report(
    report_out: &Path,
    report_data_dir: &Path,
    results: &[V3GameResult],
    report: &PersonalityReportData,
) {
    fs::create_dir_all(report_data_dir)
        .unwrap_or_else(|err| panic!("failed to create {}: {}", report_data_dir.display(), err));
    if let Some(parent) = report_out.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|err| panic!("failed to create {}: {}", parent.display(), err));
    }

    let games_path = report_data_dir.join("games.jsonl");
    let mut games_file = fs::File::create(&games_path)
        .unwrap_or_else(|err| panic!("failed to create {}: {}", games_path.display(), err));
    for result in results {
        writeln!(games_file, "{}", serde_json::to_string(result).unwrap()).unwrap();
    }

    let summary_path = report_data_dir.join("summary.json");
    fs::write(&summary_path, serde_json::to_vec_pretty(report).unwrap())
        .unwrap_or_else(|err| panic!("failed to write {}: {}", summary_path.display(), err));

    fs::write(report_out, render_personality_report_markdown(report))
        .unwrap_or_else(|err| panic!("failed to write {}: {}", report_out.display(), err));
}

fn render_personality_report_markdown(report: &PersonalityReportData) -> String {
    let mut out = String::new();
    out.push_str("# V3 Personality Report\n\n");
    out.push_str("## Run metadata\n\n");
    out.push_str(&format!(
        "- Generated at Unix time `{}` from git `{}`{}\n",
        report.metadata.generated_at_epoch_s,
        report.metadata.git_head,
        if report.metadata.git_dirty {
            " (`dirty` worktree)"
        } else {
            ""
        }
    ));
    out.push_str(&format!(
        "- Command: `{}`\n- Seeds: `{}`\n- Max ticks: `{}`\n- Map size: `{}`\n- Snapshot interval: `{}` ticks\n- Games: `{}`\n\n",
        report.metadata.command,
        report.metadata.seeds,
        report.metadata.ticks,
        report.metadata.size,
        report.metadata.snapshot_interval,
        report.metadata.games
    ));

    out.push_str("## Personality summary\n\n");
    out.push_str("| Personality | Games | Wins | Draws | Losses | Win rate | Draw rate | Avg ticks | Avg deaths | Avg final entities | Avg final soldiers | Avg final territory |\n");
    out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n");
    for summary in &report.personality_summaries {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.1}% | {:.1}% | {:.1} | {:.2} | {:.2} | {:.2} | {:.2} |\n",
            summary.personality,
            summary.games,
            summary.wins,
            summary.draws,
            summary.losses,
            summary.win_rate * 100.0,
            summary.draw_rate * 100.0,
            summary.avg_ticks,
            summary.avg_deaths,
            summary.avg_final_entities,
            summary.avg_final_soldiers,
            summary.avg_final_territory,
        ));
    }
    out.push('\n');

    out.push_str("## Matchup summary\n\n");
    out.push_str("| Matchup | Games | Wins | Draws | Avg ticks | Avg deaths | Avg final entities | Avg final soldiers | Avg final territory |\n");
    out.push_str("|---|---:|---|---:|---:|---:|---|---|---|\n");
    for summary in &report.matchup_summaries {
        out.push_str(&format!(
            "| {} | {} | {} / {} | {} | {:.1} | {:.2} | {} / {} | {} / {} | {} / {} |\n",
            summary.matchup,
            summary.games,
            summary.wins.first().copied().unwrap_or(0),
            summary.wins.get(1).copied().unwrap_or(0),
            summary.draws,
            summary.avg_ticks,
            summary.avg_deaths,
            fmt_pair(&summary.avg_final_entities),
            fmt_pair_tail(&summary.avg_final_entities),
            fmt_pair(&summary.avg_final_soldiers),
            fmt_pair_tail(&summary.avg_final_soldiers),
            fmt_pair(&summary.avg_final_territory),
            fmt_pair_tail(&summary.avg_final_territory),
        ));
    }
    out.push('\n');

    out.push_str("## Findings\n\n");
    for finding in report_findings(report) {
        out.push_str(&format!("- {}\n", finding));
    }
    out.push('\n');

    out.push_str("## Diagnosis by matchup\n\n");
    for summary in &report.matchup_summaries {
        out.push_str(&format!("### {}\n\n", summary.matchup));
        out.push_str(&format!(
            "- Flags: zero_deaths=`{}`, flat_entities=`{}`, flat_soldiers=`{}`, flat_territory=`{}`, attrition_without_resolution=`{}`\n",
            summary.diagnosis.zero_deaths,
            summary.diagnosis.flat_entities,
            summary.diagnosis.flat_soldiers,
            summary.diagnosis.flat_territory,
            summary.diagnosis.attrition_without_resolution,
        ));
        for note in &summary.diagnosis.notes {
            out.push_str(&format!("- {}\n", note));
        }
        out.push('\n');
    }

    out
}

fn report_findings(report: &PersonalityReportData) -> Vec<String> {
    let mut findings = Vec::new();
    if let Some(best) = report
        .personality_summaries
        .iter()
        .max_by(|a, b| a.avg_deaths.partial_cmp(&b.avg_deaths).unwrap())
    {
        findings.push(format!(
            "{} produces the highest average deaths across the matrix ({:.2}).",
            best.personality, best.avg_deaths
        ));
    }
    if let Some(best) = report.personality_summaries.iter().max_by(|a, b| {
        a.avg_final_soldiers
            .partial_cmp(&b.avg_final_soldiers)
            .unwrap()
    }) {
        findings.push(format!(
            "{} ends with the largest average surviving soldier count ({:.2}).",
            best.personality, best.avg_final_soldiers
        ));
    }

    let stalled: Vec<&str> = report
        .matchup_summaries
        .iter()
        .filter(|summary| summary.diagnosis.zero_deaths)
        .map(|summary| summary.matchup.as_str())
        .collect();
    if stalled.is_empty() {
        findings.push(
            "No matchup is a complete zero-death stalemate across the sampled seeds.".to_string(),
        );
    } else {
        findings.push(format!(
            "Zero-death stalemates persist in: {}.",
            stalled.join(", ")
        ));
    }

    findings
}

fn average_per_slot<F>(results: &[V3GameResult], values: F) -> Vec<f64>
where
    F: Fn(&V3GameResult) -> &[usize],
{
    if results.is_empty() {
        return Vec::new();
    }
    let width = values(&results[0]).len();
    let mut totals = vec![0.0; width];
    for result in results {
        for (idx, value) in values(result).iter().enumerate() {
            totals[idx] += *value as f64;
        }
    }
    totals
        .into_iter()
        .map(|total| total / results.len() as f64)
        .collect()
}

fn series_is_flat<F>(snapshots: &[V3Snapshot], values: F) -> bool
where
    F: Fn(&V3Snapshot) -> &[usize],
{
    let Some(first) = snapshots.first() else {
        return true;
    };
    let first_values = values(first);
    snapshots
        .iter()
        .all(|snapshot| values(snapshot) == first_values)
}

fn final_counts_changed(result: &V3GameResult) -> bool {
    let Some(first) = result.snapshots.first() else {
        return false;
    };
    result.final_entities != first.entities
        || result.final_soldiers != first.soldiers
        || result.final_territory != first.territory
}

fn format_seed_list(seeds: &[u64]) -> String {
    match (seeds.first(), seeds.last()) {
        (Some(first), Some(last)) if seeds.len() > 1 => format!("{}-{}", first, last),
        (Some(first), _) => first.to_string(),
        _ => "empty".to_string(),
    }
}

fn format_cli_command(args: &[String]) -> String {
    let trimmed: Vec<&str> = args
        .iter()
        .map(|s| s.as_str())
        .skip_while(|arg| arg.contains('/') || arg.ends_with("simulate_everything_cli"))
        .collect();
    if trimmed.is_empty() {
        "simulate_everything_cli v3bench --personality-report".to_string()
    } else {
        format!("simulate_everything_cli {}", trimmed.join(" "))
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn git_head_short() -> String {
    Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn git_is_dirty() -> bool {
    Command::new("git")
        .args(["status", "--short"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(true)
}

fn average_u64<I>(iter: I) -> f64
where
    I: Iterator<Item = u64>,
{
    let mut total = 0u64;
    let mut count = 0usize;
    for value in iter {
        total += value;
        count += 1;
    }
    ratio_u64(total, count)
}

fn average_usize<I>(iter: I) -> f64
where
    I: Iterator<Item = usize>,
{
    let mut total = 0usize;
    let mut count = 0usize;
    for value in iter {
        total += value;
        count += 1;
    }
    ratio(total, count)
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn ratio_u64(numerator: u64, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn ratio_f64(numerator: f64, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator / denominator as f64
    }
}

fn fmt_pair(values: &[f64]) -> String {
    format!("{:.2}", values.first().copied().unwrap_or(0.0))
}

fn fmt_pair_tail(values: &[f64]) -> String {
    format!("{:.2}", values.get(1).copied().unwrap_or(0.0))
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
        "  {:>6} {:>6} {:>10} {:>12} {:>8}  tags",
        "seed", "ticks", "winner", "final_ents", "score"
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
                    if let Some(o) = e.owner
                        && (o as usize) < owner_counts.len()
                    {
                        owner_counts[o as usize] += 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(
        tick: u64,
        entities: [usize; 2],
        soldiers: [usize; 2],
        territory: [usize; 2],
    ) -> V3Snapshot {
        V3Snapshot {
            tick,
            entities: entities.into(),
            soldiers: soldiers.into(),
            territory: territory.into(),
            alive: vec![true, true],
        }
    }

    fn game_result(
        matchup: &str,
        agents: [&str; 2],
        winner_idx: Option<u8>,
        total_deaths: usize,
        final_entities: [usize; 2],
        final_soldiers: [usize; 2],
        final_territory: [usize; 2],
        snapshots: Vec<V3Snapshot>,
    ) -> V3GameResult {
        V3GameResult {
            seed: 0,
            matchup: matchup.to_string(),
            agents: agents.into_iter().map(str::to_string).collect(),
            winner: winner_idx.map(|idx| agents[idx as usize].to_string()),
            winner_idx,
            ticks: 2000,
            draw: winner_idx.is_none(),
            compute_total_us: vec![0, 0],
            compute_mean_us: vec![0.0, 0.0],
            compute_max_us: vec![0, 0],
            final_entities: final_entities.into(),
            final_soldiers: final_soldiers.into(),
            final_territory: final_territory.into(),
            total_deaths,
            interest_score: 0.0,
            interest_tags: Vec::new(),
            snapshots,
        }
    }

    #[test]
    fn personality_report_matchups_cover_full_matrix_without_null() {
        let matchups = personality_report_matchups();
        assert_eq!(matchups.len(), 9);
        assert!(matchups.contains(&vec!["spread", "spread"]));
        assert!(matchups.contains(&vec!["spread", "striker"]));
        assert!(matchups.contains(&vec!["turtle", "striker"]));
        assert!(!matchups.iter().flatten().any(|name| *name == "null"));
    }

    #[test]
    fn diagnose_matchup_flags_stalemate_signals() {
        let result = game_result(
            "spread-vs-turtle",
            ["spread", "turtle"],
            None,
            0,
            [35, 35],
            [5, 6],
            [2, 2],
            vec![
                snapshot(100, [35, 35], [5, 6], [2, 2]),
                snapshot(200, [35, 35], [5, 6], [2, 2]),
            ],
        );

        let diagnosis = diagnose_matchup(&[result]);
        assert!(diagnosis.zero_deaths);
        assert!(diagnosis.flat_entities);
        assert!(diagnosis.flat_soldiers);
        assert!(diagnosis.flat_territory);
        assert!(!diagnosis.attrition_without_resolution);
    }

    #[test]
    fn diagnose_matchup_flags_attrition_without_resolution() {
        let result = game_result(
            "spread-vs-striker",
            ["spread", "striker"],
            None,
            8,
            [31, 29],
            [1, 9],
            [2, 4],
            vec![
                snapshot(100, [35, 35], [5, 15], [2, 4]),
                snapshot(2000, [31, 29], [1, 9], [2, 4]),
            ],
        );

        let diagnosis = diagnose_matchup(&[result]);
        assert!(!diagnosis.zero_deaths);
        assert!(diagnosis.attrition_without_resolution);
    }

    #[test]
    fn personality_summary_aggregates_results_by_agent_slot() {
        let results = vec![
            game_result(
                "spread-vs-striker",
                ["spread", "striker"],
                Some(0),
                4,
                [30, 20],
                [8, 4],
                [3, 2],
                vec![snapshot(100, [35, 35], [5, 15], [2, 4])],
            ),
            game_result(
                "striker-vs-spread",
                ["striker", "spread"],
                None,
                6,
                [28, 32],
                [6, 3],
                [4, 1],
                vec![snapshot(100, [35, 35], [15, 5], [4, 2])],
            ),
        ];

        let summaries = build_personality_summaries(&results);
        let spread = summaries
            .iter()
            .find(|s| s.personality == "spread")
            .unwrap();
        let striker = summaries
            .iter()
            .find(|s| s.personality == "striker")
            .unwrap();

        assert_eq!(spread.games, 2);
        assert_eq!(spread.wins, 1);
        assert_eq!(spread.draws, 1);
        assert!((spread.avg_final_entities - 31.0).abs() < 0.001);

        assert_eq!(striker.games, 2);
        assert_eq!(striker.losses, 1);
        assert_eq!(striker.draws, 1);
        assert!((striker.avg_deaths - 5.0).abs() < 0.001);
    }
}
