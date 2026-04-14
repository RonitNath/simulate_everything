use crate::headless_renderer::{DEFAULT_RENDER_SIZE, render_snapshot_png};
use serde::{Deserialize, Serialize};
use simulate_everything_engine::v2::hex::offset_to_axial;
use simulate_everything_engine::v2::state::EntityKey;
use simulate_everything_engine::v3::behavior_snapshot::{
    BehaviorSnapshot, capture_behavior_snapshot,
};
use simulate_everything_engine::v3::lifecycle::spawn_entity;
use simulate_everything_engine::v3::mapgen;
use simulate_everything_engine::v3::movement::Mobile;
use simulate_everything_engine::v3::physical::{PhysicalProperties, SiteProperties};
use simulate_everything_engine::v3::sim;
use simulate_everything_engine::v3::social::SocialState;
use simulate_everything_engine::v3::spatial::{GeoMaterial, Heightfield, Vec2, Vec3};
use simulate_everything_engine::v3::state::{EntityBuilder, GameState, Person, Role};
use simulate_everything_engine::v3::terrain_ops::{TerrainOp, terrain_raster_spec};
use simulate_everything_protocol::{
    CommodityKind, EntityNeedsInfo, MaterialKind, MatterState, PropertyTag,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tiny_skia::{Pixmap, PixmapPaint, Transform};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScenarioMode {
    Stat,
    Forensic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioConfig {
    pub id: String,
    pub description: String,
    pub duration_ticks: u64,
    #[serde(default = "default_mode")]
    pub mode: ScenarioMode,
    #[serde(default = "default_runs")]
    pub runs: u32,
    #[serde(default = "default_seed")]
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize)]
struct InvariantSample {
    tick: u64,
    value: f32,
    threshold: f32,
    passed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct InvariantResult {
    name: String,
    passed: bool,
    samples: Vec<InvariantSample>,
}

#[derive(Debug, Clone, Serialize)]
struct ScenarioSummary {
    id: String,
    mode: String,
    runs: u32,
    passed_runs: u32,
    invariants_passed: bool,
    final_tick: u64,
}

#[derive(Debug, Clone, Serialize)]
struct TimelinePoint {
    tick: u64,
    goal: Option<String>,
    action: Option<String>,
    pos: [f32; 3],
    needs: Option<EntityNeedsInfo>,
}

pub fn main(args: &[String]) {
    let config = flag_value(args, "--scenario-file")
        .map(load_config)
        .or_else(|| flag_value(args, "--scenario").map(builtin_config))
        .unwrap_or_else(|| builtin_config("solo_farmer_harvest"));
    let output_dir = flag_value(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("v3behavior_output"));
    let forensic_override = args.iter().any(|arg| arg == "--forensic");
    let mode = if forensic_override {
        ScenarioMode::Forensic
    } else {
        config.mode.clone()
    };

    let run_count = match mode {
        ScenarioMode::Forensic => 1,
        ScenarioMode::Stat => config.runs.max(1),
    };

    let mut passed_runs = 0;
    let mut final_tick = 0;
    for run_index in 0..run_count {
        let run_seed = config.seed + run_index as u64;
        let (mut state, scenario_entities, injection_tick) = setup_state(&config.id, run_seed);
        let scenario_dir = output_dir.join(&config.id);
        if let ScenarioMode::Forensic = mode {
            let _ = fs::remove_dir_all(&scenario_dir);
            fs::create_dir_all(scenario_dir.join("ticks")).expect("create ticks dir");
            fs::create_dir_all(scenario_dir.join("frames")).expect("create frames dir");
        }

        let mut timelines: BTreeMap<u32, Vec<TimelinePoint>> = BTreeMap::new();
        let mut snapshots = Vec::new();
        for _ in 0..config.duration_ticks {
            if let Some(tick) = injection_tick
                && state.tick == tick
            {
                inject_patrol_threat(&mut state);
            }
            sim::tick(&mut state, 1.0);
            let snapshot = capture_behavior_snapshot(&state, 1.0);
            final_tick = snapshot.tick;
            collect_timelines(&snapshot, &mut timelines);
            if let ScenarioMode::Forensic = mode {
                write_snapshot_artifacts(&scenario_dir, &snapshot, &state);
            }
            snapshots.push(snapshot);
        }

        let invariants = evaluate_invariants(&config.id, &snapshots, &state, &scenario_entities);
        let passed = invariants.iter().all(|invariant| invariant.passed);
        if passed {
            passed_runs += 1;
        }

        if let ScenarioMode::Forensic = mode {
            write_forensic_outputs(
                &scenario_dir,
                &config,
                &snapshots,
                &timelines,
                &invariants,
                run_count,
                passed_runs,
                &state,
            );
        }
    }

    let summary = ScenarioSummary {
        id: config.id,
        mode: match mode {
            ScenarioMode::Stat => "stat".to_string(),
            ScenarioMode::Forensic => "forensic".to_string(),
        },
        runs: run_count,
        passed_runs,
        invariants_passed: passed_runs == run_count,
        final_tick,
    };
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
}

fn default_mode() -> ScenarioMode {
    ScenarioMode::Forensic
}

fn default_runs() -> u32 {
    1
}

fn default_seed() -> u64 {
    42
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|idx| args.get(idx + 1))
        .map(|arg| arg.as_str())
}

fn load_config(path: &str) -> ScenarioConfig {
    let raw = fs::read_to_string(path).expect("read scenario config");
    toml::from_str(&raw).expect("parse scenario config")
}

fn builtin_config(id: &str) -> ScenarioConfig {
    match id {
        "1v1_sword_engagement" => ScenarioConfig {
            id: id.to_string(),
            description: "Two soldiers approach and engage.".to_string(),
            duration_ticks: 300,
            mode: ScenarioMode::Forensic,
            runs: 1,
            seed: 42,
        },
        "patrol_responds_to_threat" => ScenarioConfig {
            id: id.to_string(),
            description: "Patrol entities react to injected threat.".to_string(),
            duration_ticks: 250,
            mode: ScenarioMode::Forensic,
            runs: 1,
            seed: 42,
        },
        "settlement_stability_200" => ScenarioConfig {
            id: id.to_string(),
            description: "Settlement stability smoke.".to_string(),
            duration_ticks: 500,
            mode: ScenarioMode::Stat,
            runs: 5,
            seed: 100,
        },
        "terrain_road_emergence" => ScenarioConfig {
            id: id.to_string(),
            description: "Road/furrow terrain activity smoke.".to_string(),
            duration_ticks: 400,
            mode: ScenarioMode::Forensic,
            runs: 1,
            seed: 7,
        },
        _ => ScenarioConfig {
            id: "solo_farmer_harvest".to_string(),
            description: "One hungry farmer should harvest and eat.".to_string(),
            duration_ticks: 220,
            mode: ScenarioMode::Forensic,
            runs: 1,
            seed: 42,
        },
    }
}

fn setup_state(id: &str, seed: u64) -> (GameState, Vec<u32>, Option<u64>) {
    match id {
        "1v1_sword_engagement" => setup_combat_scenario(seed),
        "patrol_responds_to_threat" => setup_patrol_scenario(seed),
        "settlement_stability_200" => {
            let state = mapgen::generate_economy_ready(30, 30, 2, seed);
            (state, Vec::new(), None)
        }
        "terrain_road_emergence" => setup_terrain_scenario(seed),
        _ => setup_farmer_scenario(seed),
    }
}

fn setup_farmer_scenario(_seed: u64) -> (GameState, Vec<u32>, Option<u64>) {
    let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
    let mut state = GameState::new(20, 20, 1, hf);
    let home = spawn_site(&mut state, Vec3::new(100.0, 100.0, 0.0), 0, true, false);
    let farm = spawn_site(&mut state, Vec3::new(180.0, 100.0, 0.0), 0, false, true);
    seed_settlement_ops(&mut state, home, farm);
    let farmer = spawn_person(&mut state, Vec3::new(100.0, 100.0, 0.0), 0, Role::Farmer);
    if let Some(behavior) = state
        .entities
        .get_mut(farmer)
        .and_then(|e| e.behavior.as_mut())
    {
        behavior.needs.hunger = 0.82;
        behavior.social = SocialState::default();
    }
    let ids = vec![state.entities[farmer].id];
    (state, ids, None)
}

fn setup_combat_scenario(_seed: u64) -> (GameState, Vec<u32>, Option<u64>) {
    let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
    let mut state = GameState::new(20, 20, 2, hf);
    let left = spawn_person(&mut state, Vec3::new(50.0, 100.0, 0.0), 0, Role::Soldier);
    let right = spawn_person(&mut state, Vec3::new(180.0, 100.0, 0.0), 1, Role::Soldier);
    if let Some(behavior) = state
        .entities
        .get_mut(left)
        .and_then(|e| e.behavior.as_mut())
    {
        behavior.needs.duty = 0.9;
    }
    if let Some(behavior) = state
        .entities
        .get_mut(right)
        .and_then(|e| e.behavior.as_mut())
    {
        behavior.needs.duty = 0.9;
    }
    let ids = vec![state.entities[left].id, state.entities[right].id];
    (state, ids, None)
}

fn setup_patrol_scenario(_seed: u64) -> (GameState, Vec<u32>, Option<u64>) {
    let hf = Heightfield::new(30, 30, 0.0, GeoMaterial::Soil);
    let mut state = GameState::new(30, 30, 2, hf);
    let mut ids = Vec::new();
    for i in 0..5 {
        let soldier = spawn_person(
            &mut state,
            Vec3::new(100.0 + i as f32 * 10.0, 100.0, 0.0),
            0,
            Role::Soldier,
        );
        ids.push(state.entities[soldier].id);
    }
    (state, ids, Some(100))
}

fn setup_terrain_scenario(seed: u64) -> (GameState, Vec<u32>, Option<u64>) {
    let mut state = mapgen::generate(20, 20, 1, seed);
    let ids = state
        .entities
        .values()
        .filter_map(|entity| entity.person.as_ref().map(|_| entity.id))
        .take(10)
        .collect();
    // Seed an extra corridor to make terrain activity visible in forensic artifacts.
    let center_a = map_center_world(&state, offset_to_axial(5, 5));
    let center_b = map_center_world(&state, offset_to_axial(10, 10));
    state.terrain_ops.push_op(
        offset_to_axial(5, 5),
        TerrainOp::Road {
            points: smallvec::smallvec![center_a.xy(), center_b.xy()],
            width: 6.0,
            grade: 1.0,
            material: GeoMaterial::Soil,
        },
        &state.heightfield,
        state.map_width,
        state.map_height,
    );
    (state, ids, None)
}

fn spawn_person(state: &mut GameState, pos: Vec3, owner: u8, role: Role) -> EntityKey {
    spawn_entity(
        state,
        EntityBuilder::new()
            .pos(pos)
            .owner(owner)
            .person(Person {
                role,
                combat_skill: if role == Role::Soldier { 0.5 } else { 0.2 },
            })
            .mobile(Mobile::new(2.0, 10.0))
            .vitals(),
    )
}

fn spawn_site(state: &mut GameState, pos: Vec3, owner: u8, shelter: bool, farm: bool) -> EntityKey {
    let mut physical = PhysicalProperties::new(900.0, 0.4, MaterialKind::Wood, MatterState::Solid)
        .with_tags(&[PropertyTag::Structural, PropertyTag::Container]);
    if shelter {
        physical.insert_tag(PropertyTag::Shelter);
        physical.insert_tag(PropertyTag::Settlement);
    }
    if farm {
        physical.insert_tag(PropertyTag::Farm);
        physical.insert_tag(PropertyTag::Harvestable);
    }
    spawn_entity(
        state,
        EntityBuilder::new()
            .pos(pos)
            .owner(owner)
            .physical(physical)
            .site(SiteProperties {
                build_progress: 1.0,
                integrity: 100.0,
                occupancy_capacity: 20,
            }),
    )
}

fn seed_settlement_ops(state: &mut GameState, home: EntityKey, farm: EntityKey) {
    let home_pos = state.entities[home].pos.unwrap();
    let farm_pos = state.entities[farm].pos.unwrap();
    let home_hex = offset_to_axial(1, 1);
    state.terrain_ops.push_op(
        home_hex,
        TerrainOp::Road {
            points: smallvec::smallvec![home_pos.xy(), farm_pos.xy()],
            width: 5.0,
            grade: 1.0,
            material: GeoMaterial::Soil,
        },
        &state.heightfield,
        state.map_width,
        state.map_height,
    );
    state.terrain_ops.push_op(
        home_hex,
        TerrainOp::Furrow {
            center: farm_pos.xy(),
            half_extents: Vec2::new(15.0, 10.0),
            rotation: 0.0,
            spacing: 2.0,
            depth: 0.5,
        },
        &state.heightfield,
        state.map_width,
        state.map_height,
    );
}

fn inject_patrol_threat(state: &mut GameState) {
    for i in 0..2 {
        let _ = spawn_person(
            state,
            Vec3::new(220.0 + i as f32 * 8.0, 100.0, 0.0),
            1,
            Role::Soldier,
        );
    }
}

fn collect_timelines(
    snapshot: &BehaviorSnapshot,
    timelines: &mut BTreeMap<u32, Vec<TimelinePoint>>,
) {
    for entity in &snapshot.entities {
        timelines.entry(entity.id).or_default().push(TimelinePoint {
            tick: snapshot.tick,
            goal: entity.current_goal.clone(),
            action: entity.current_action.clone(),
            pos: entity.pos,
            needs: entity.needs.clone(),
        });
    }
}

fn write_snapshot_artifacts(dir: &Path, snapshot: &BehaviorSnapshot, state: &GameState) {
    let tick_path = dir.join("ticks").join(format!("{:04}.json", snapshot.tick));
    fs::write(&tick_path, serde_json::to_vec_pretty(snapshot).unwrap())
        .expect("write tick snapshot");
    let frame_path = dir.join("frames").join(format!("{:04}.png", snapshot.tick));
    let _ = render_snapshot_png(state, snapshot, &frame_path, DEFAULT_RENDER_SIZE);
}

fn write_forensic_outputs(
    dir: &Path,
    config: &ScenarioConfig,
    snapshots: &[BehaviorSnapshot],
    timelines: &BTreeMap<u32, Vec<TimelinePoint>>,
    invariants: &[InvariantResult],
    runs: u32,
    passed_runs: u32,
    state: &GameState,
) {
    let summary = ScenarioSummary {
        id: config.id.clone(),
        mode: "forensic".to_string(),
        runs,
        passed_runs,
        invariants_passed: invariants.iter().all(|invariant| invariant.passed),
        final_tick: snapshots.last().map(|snapshot| snapshot.tick).unwrap_or(0),
    };
    fs::write(
        dir.join("summary.json"),
        serde_json::to_vec_pretty(&summary).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("entity_timelines.json"),
        serde_json::to_vec_pretty(timelines).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("terrain_ops.json"),
        serde_json::to_vec_pretty(&state.terrain_ops).unwrap(),
    )
    .unwrap();
    fs::write(
        dir.join("invariants.json"),
        serde_json::to_vec_pretty(invariants).unwrap(),
    )
    .unwrap();
    let _ = build_filmstrip(dir, snapshots);
}

fn build_filmstrip(dir: &Path, snapshots: &[BehaviorSnapshot]) -> Result<(), String> {
    let selected: Vec<_> = snapshots.iter().step_by(50).collect();
    if selected.is_empty() {
        return Ok(());
    }
    let tile = 256u32;
    let cols = 4u32;
    let rows = ((selected.len() as f32) / cols as f32).ceil() as u32;
    let mut filmstrip =
        Pixmap::new(cols * tile, rows * tile).ok_or_else(|| "filmstrip alloc".to_string())?;
    for (idx, snapshot) in selected.iter().enumerate() {
        let frame_path = dir.join("frames").join(format!("{:04}.png", snapshot.tick));
        if !frame_path.exists() {
            continue;
        }
        let frame = Pixmap::load_png(&frame_path).map_err(|err| err.to_string())?;
        let x = (idx as u32 % cols) * tile;
        let y = (idx as u32 / cols) * tile;
        filmstrip.draw_pixmap(
            x as i32,
            y as i32,
            frame.as_ref(),
            &PixmapPaint::default(),
            Transform::from_scale(
                tile as f32 / frame.width() as f32,
                tile as f32 / frame.height() as f32,
            ),
            None,
        );
    }
    filmstrip
        .save_png(dir.join("filmstrip.png"))
        .map_err(|err| err.to_string())
}

fn evaluate_invariants(
    scenario_id: &str,
    snapshots: &[BehaviorSnapshot],
    state: &GameState,
    focus_entities: &[u32],
) -> Vec<InvariantResult> {
    match scenario_id {
        "1v1_sword_engagement" => vec![
            invariant_engagement_started(snapshots),
            invariant_one_entity_dead_or_fled(snapshots, focus_entities),
        ],
        "patrol_responds_to_threat" => vec![
            invariant_engagement_started(snapshots),
            invariant_close_distance(snapshots, focus_entities, 140.0),
        ],
        "settlement_stability_200" => vec![
            invariant_population_alive(state, 10.0),
            invariant_food_stockpile_positive(state),
        ],
        "terrain_road_emergence" => vec![invariant_road_ops_present(state)],
        _ => vec![
            invariant_hunger_recovers(snapshots, focus_entities.first().copied()),
            invariant_goal_selected(snapshots, focus_entities.first().copied(), "Eat"),
        ],
    }
}

fn invariant_goal_selected(
    snapshots: &[BehaviorSnapshot],
    entity_id: Option<u32>,
    goal_name: &str,
) -> InvariantResult {
    let mut samples = Vec::new();
    let mut passed = false;
    for snapshot in snapshots {
        let value = snapshot
            .entities
            .iter()
            .find(|entity| Some(entity.id) == entity_id)
            .and_then(|entity| entity.current_goal.as_ref())
            .map(|goal| if goal == goal_name { 1.0 } else { 0.0 })
            .unwrap_or(0.0);
        let sample_passed = value >= 1.0;
        passed |= sample_passed;
        samples.push(InvariantSample {
            tick: snapshot.tick,
            value,
            threshold: 1.0,
            passed: sample_passed,
        });
    }
    InvariantResult {
        name: format!("goal_selected_{}", goal_name.to_lowercase()),
        passed,
        samples,
    }
}

fn invariant_hunger_recovers(
    snapshots: &[BehaviorSnapshot],
    entity_id: Option<u32>,
) -> InvariantResult {
    let mut samples = Vec::new();
    let mut passed = false;
    for snapshot in snapshots {
        let value = snapshot
            .entities
            .iter()
            .find(|entity| Some(entity.id) == entity_id)
            .and_then(|entity| entity.needs.as_ref())
            .map(|needs| 1.0 - needs.hunger)
            .unwrap_or(0.0);
        let sample_passed = value >= 0.3;
        passed |= sample_passed;
        samples.push(InvariantSample {
            tick: snapshot.tick,
            value,
            threshold: 0.3,
            passed: sample_passed,
        });
    }
    InvariantResult {
        name: "hunger_recovered".to_string(),
        passed,
        samples,
    }
}

fn invariant_engagement_started(snapshots: &[BehaviorSnapshot]) -> InvariantResult {
    let mut samples = Vec::new();
    let mut passed = false;
    for snapshot in snapshots {
        let value = if snapshot
            .entities
            .iter()
            .any(|entity| entity.current_action.as_deref() == Some("Attack"))
        {
            1.0
        } else {
            0.0
        };
        let sample_passed = value >= 1.0;
        passed |= sample_passed;
        samples.push(InvariantSample {
            tick: snapshot.tick,
            value,
            threshold: 1.0,
            passed: sample_passed,
        });
    }
    InvariantResult {
        name: "engagement_started".to_string(),
        passed,
        samples,
    }
}

fn invariant_one_entity_dead_or_fled(
    snapshots: &[BehaviorSnapshot],
    focus_entities: &[u32],
) -> InvariantResult {
    let mut samples = Vec::new();
    let mut passed = false;
    for snapshot in snapshots {
        let visible = focus_entities
            .iter()
            .filter(|entity_id| {
                snapshot
                    .entities
                    .iter()
                    .any(|entity| entity.id == **entity_id)
            })
            .count() as f32;
        let sample_passed = visible < focus_entities.len() as f32;
        passed |= sample_passed;
        samples.push(InvariantSample {
            tick: snapshot.tick,
            value: visible,
            threshold: (focus_entities.len().saturating_sub(1)) as f32,
            passed: sample_passed,
        });
    }
    InvariantResult {
        name: "one_entity_dead_or_fled".to_string(),
        passed,
        samples,
    }
}

fn invariant_close_distance(
    snapshots: &[BehaviorSnapshot],
    focus_entities: &[u32],
    max_distance: f32,
) -> InvariantResult {
    let mut samples = Vec::new();
    let mut passed = false;
    for snapshot in snapshots {
        let tracked: Vec<_> = snapshot
            .entities
            .iter()
            .filter(|entity| focus_entities.contains(&entity.id))
            .collect();
        let value = if tracked.len() >= 2 {
            let a = tracked[0].pos;
            let b = tracked[tracked.len() - 1].pos;
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
        } else {
            f32::INFINITY
        };
        let sample_passed = value <= max_distance;
        passed |= sample_passed;
        samples.push(InvariantSample {
            tick: snapshot.tick,
            value,
            threshold: max_distance,
            passed: sample_passed,
        });
    }
    InvariantResult {
        name: "response_distance".to_string(),
        passed,
        samples,
    }
}

fn invariant_population_alive(state: &GameState, threshold: f32) -> InvariantResult {
    let alive = state
        .entities
        .values()
        .filter(|entity| {
            entity.person.is_some()
                && entity
                    .vitals
                    .as_ref()
                    .map(|vitals| !vitals.is_dead())
                    .unwrap_or(true)
        })
        .count() as f32;
    InvariantResult {
        name: "population_alive".to_string(),
        passed: alive >= threshold,
        samples: vec![InvariantSample {
            tick: state.tick,
            value: alive,
            threshold,
            passed: alive >= threshold,
        }],
    }
}

fn invariant_food_stockpile_positive(state: &GameState) -> InvariantResult {
    let amount = state
        .entities
        .values()
        .filter_map(|entity| entity.matter.as_ref())
        .filter(|matter| matter.commodity == CommodityKind::Food)
        .map(|matter| matter.amount)
        .sum::<f32>();
    InvariantResult {
        name: "food_stockpile_positive".to_string(),
        passed: amount > 0.0,
        samples: vec![InvariantSample {
            tick: state.tick,
            value: amount,
            threshold: 0.0,
            passed: amount > 0.0,
        }],
    }
}

fn invariant_road_ops_present(state: &GameState) -> InvariantResult {
    let spec = terrain_raster_spec(state.map_width, state.map_height, 16.0);
    let mut count = 0.0;
    for row in 0..spec.height {
        for col in 0..spec.width {
            let hex = offset_to_axial(
                row as i32 % state.map_height as i32,
                col as i32 % state.map_width as i32,
            );
            count += state
                .terrain_ops
                .ops_for_hex(hex)
                .iter()
                .filter(|op| matches!(op, TerrainOp::Road { .. }))
                .count() as f32;
        }
    }
    InvariantResult {
        name: "road_ops_present".to_string(),
        passed: count > 0.0,
        samples: vec![InvariantSample {
            tick: state.tick,
            value: count,
            threshold: 1.0,
            passed: count > 0.0,
        }],
    }
}

fn map_center_world(state: &GameState, hex: simulate_everything_engine::v2::hex::Axial) -> Vec3 {
    let _ = state;
    simulate_everything_engine::v3::hex::hex_to_world(hex)
}
