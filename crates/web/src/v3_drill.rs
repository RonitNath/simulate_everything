//! V3 Drill Pad — a tiny sandbox for driving individual entity behavior via REST.
//!
//! Spawns 1–2 soldiers on a flat 5×3 hex map. No agents run — all commands come
//! from the API. The pad streams snapshots to a WS spectator for real-time
//! rendering, and returns JSON state + ASCII art from curl.

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast};
use tracing::info;

use simulate_everything_engine::v2::hex::Axial;
use simulate_everything_engine::v2::state::EntityKey;
use simulate_everything_engine::v3::{
    armor::{BodyZone, DamageType},
    equipment::Equipment,
    hex::hex_to_world,
    lifecycle::{contain, spawn_entity},
    martial::AttackMotion,
    movement::Mobile,
    sim,
    spatial::{GeoMaterial, Heightfield, Vec3},
    state::{Combatant, EntityBuilder, GameState, Person, Role},
    weapon,
    wound::{Severity, Wound},
};

use crate::v3_protocol::{self, DeltaTracker, V3ServerToSpectator};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const DRILL_WIDTH: usize = 5;
const DRILL_HEIGHT: usize = 3;
const DRILL_PLAYERS: u8 = 2;
const BROADCAST_CAPACITY: usize = 64;

/// Person collision radius (meters).
const PERSON_RADIUS: f32 = 10.0;
/// Person steering force.
const PERSON_STEERING: f32 = 2.0;

// ---------------------------------------------------------------------------
// Drill state
// ---------------------------------------------------------------------------

struct DrillInner {
    state: GameState,
    delta_tracker: DeltaTracker,
    /// Map from public entity ID → slotmap key.
    entity_keys: Vec<(u32, EntityKey)>,
}

pub struct V3Drill {
    inner: Mutex<DrillInner>,
    spectator_tx: broadcast::Sender<V3ServerToSpectator>,
}

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DrillCommand {
    /// Target entity by public ID (0-indexed shorthand or actual ID).
    pub entity: u32,
    /// Set facing angle in radians.
    #[serde(default)]
    pub facing: Option<f32>,
    /// Move to world position [x, y].
    #[serde(default)]
    pub move_to: Option<[f32; 2]>,
    /// Attack target entity by public ID.
    #[serde(default)]
    pub attack: Option<u32>,
    /// Set position directly (teleport) [x, y].
    #[serde(default)]
    pub set_pos: Option<[f32; 2]>,
}

#[derive(Debug, Deserialize)]
pub struct ExecRequest {
    /// Commands to apply before advancing.
    #[serde(default)]
    pub commands: Vec<DrillCommand>,
    /// Number of ticks to advance after applying commands.
    #[serde(default = "default_settle_ticks")]
    pub settle_ticks: u64,
}

fn default_settle_ticks() -> u64 {
    1
}

#[derive(Debug, Deserialize)]
pub struct ViewParams {
    /// Center hex column for ASCII viewport.
    #[serde(default)]
    pub center_q: Option<i32>,
    /// Center hex row for ASCII viewport.
    #[serde(default)]
    pub center_r: Option<i32>,
    /// Zoom level for browser (sent via WS config).
    #[serde(default)]
    pub zoom: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct DrillEntity {
    pub id: u32,
    pub owner: Option<u8>,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub hex_q: i32,
    pub hex_r: i32,
    pub facing: Option<f32>,
    pub blood: Option<f32>,
    pub stamina: Option<f32>,
    pub attack_phase: Option<String>,
    pub attack_motion: Option<String>,
    pub weapon_angle: Option<f32>,
    pub attack_progress: Option<f32>,
    pub weapon_type: Option<String>,
    pub wounds: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ExecResponse {
    pub tick: u64,
    pub settled: bool,
    pub entities: Vec<DrillEntity>,
    pub ascii: String,
}

#[derive(Debug, Serialize)]
pub struct DrillStatus {
    pub tick: u64,
    pub entities: Vec<DrillEntity>,
    pub ascii: String,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl V3Drill {
    pub fn new() -> Self {
        let (spectator_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let state = create_drill_state();
        let entity_keys = state
            .entities
            .iter()
            .filter(|(_, e)| e.person.is_some())
            .map(|(k, e)| (e.id, k))
            .collect();

        let mut delta_tracker = DeltaTracker::new();
        let snapshot = v3_protocol::build_snapshot(&state, 0.05);
        delta_tracker.seed_from_snapshot(&snapshot);

        Self {
            inner: Mutex::new(DrillInner {
                state,
                delta_tracker,
                entity_keys,
            }),
            spectator_tx,
        }
    }

    pub fn spectator_subscribe(&self) -> broadcast::Receiver<V3ServerToSpectator> {
        self.spectator_tx.subscribe()
    }

    /// Send init + full snapshot to a newly connected spectator.
    pub async fn spectator_catchup(&self) -> Vec<V3ServerToSpectator> {
        let inner = self.inner.lock().await;
        let init = v3_protocol::build_init(
            &inner.state,
            &["drill-a".to_string(), "drill-b".to_string()],
            &["0".to_string(), "0".to_string()],
            0,
        );
        let snapshot = v3_protocol::build_snapshot(&inner.state, 0.05);
        vec![
            V3ServerToSpectator::Init { init },
            V3ServerToSpectator::Snapshot { snapshot },
        ]
    }

    /// Execute commands, advance ticks, return state.
    pub async fn exec(&self, req: ExecRequest) -> ExecResponse {
        let mut inner = self.inner.lock().await;

        // Apply commands.
        for cmd in &req.commands {
            apply_drill_command(&mut inner, cmd);
        }

        // Advance simulation.
        let dt = 0.05_f32;
        let DrillInner {
            ref mut state,
            ref mut delta_tracker,
            ..
        } = *inner;
        for _ in 0..req.settle_ticks {
            sim::tick(state, dt as f64);
            state.combat_log.drain();

            let delta = delta_tracker.build_delta(state, dt);
            let _ = self
                .spectator_tx
                .send(V3ServerToSpectator::SnapshotDelta { delta });
        }

        let entities = build_drill_entities(&inner.state, &inner.entity_keys);
        let ascii = render_ascii(&inner.state, &inner.entity_keys, None);

        ExecResponse {
            tick: inner.state.tick,
            settled: true,
            entities,
            ascii,
        }
    }

    /// Get current state without advancing.
    pub async fn status(&self, view: Option<ViewParams>) -> DrillStatus {
        let inner = self.inner.lock().await;
        let center = view.as_ref().and_then(|v| match (v.center_q, v.center_r) {
            (Some(q), Some(r)) => Some(Axial::new(q, r)),
            _ => None,
        });
        let entities = build_drill_entities(&inner.state, &inner.entity_keys);
        let ascii = render_ascii(&inner.state, &inner.entity_keys, center);
        DrillStatus {
            tick: inner.state.tick,
            entities,
            ascii,
        }
    }

    /// Reset the drill pad to initial state.
    pub async fn reset(&self) {
        let mut inner = self.inner.lock().await;
        let state = create_drill_state();
        let entity_keys = state
            .entities
            .iter()
            .filter(|(_, e)| e.person.is_some())
            .map(|(k, e)| (e.id, k))
            .collect();
        let snapshot = v3_protocol::build_snapshot(&state, 0.05);
        inner.delta_tracker = DeltaTracker::new();
        inner.delta_tracker.seed_from_snapshot(&snapshot);
        inner.state = state;
        inner.entity_keys = entity_keys;

        // Broadcast new init + snapshot.
        let init = v3_protocol::build_init(
            &inner.state,
            &["drill-a".to_string(), "drill-b".to_string()],
            &["0".to_string(), "0".to_string()],
            0,
        );
        let _ = self
            .spectator_tx
            .send(V3ServerToSpectator::Init { init });
        let _ = self
            .spectator_tx
            .send(V3ServerToSpectator::Snapshot { snapshot });
        info!("V3 drill pad reset");
    }

    /// Replace the drill pad with a zoo — a wide map showing one entity per
    /// column, each in a different visual state: idle, windup (×4 motions),
    /// committed (×4 motions), recovery, plus wound variations.
    pub async fn zoo(&self) {
        let mut inner = self.inner.lock().await;
        let state = create_zoo_state();
        let entity_keys = state
            .entities
            .iter()
            .filter(|(_, e)| e.person.is_some())
            .map(|(k, e)| (e.id, k))
            .collect();
        let snapshot = v3_protocol::build_snapshot(&state, 0.05);
        inner.delta_tracker = DeltaTracker::new();
        inner.delta_tracker.seed_from_snapshot(&snapshot);
        inner.state = state;
        inner.entity_keys = entity_keys;

        let agent_names: Vec<String> = (0..ZOO_PLAYERS).map(|i| format!("zoo-{}", i)).collect();
        let agent_versions: Vec<String> = (0..ZOO_PLAYERS).map(|_| "0".to_string()).collect();

        let init = v3_protocol::build_init(&inner.state, &agent_names, &agent_versions, 0);
        let _ = self
            .spectator_tx
            .send(V3ServerToSpectator::Init { init });
        let _ = self
            .spectator_tx
            .send(V3ServerToSpectator::Snapshot { snapshot });
        info!("V3 drill pad zoo loaded");
    }
}

// ---------------------------------------------------------------------------
// Drill map generation
// ---------------------------------------------------------------------------

/// Create a flat 5×3 hex map with 2 soldiers facing each other.
fn create_drill_state() -> GameState {
    let vertex_cols = DRILL_WIDTH * 2 + 1;
    let vertex_rows = DRILL_HEIGHT * 2 + 1;

    // Flat terrain — all heights 0.
    let heightfield = Heightfield::new(vertex_cols, vertex_rows, 0.0, GeoMaterial::Soil);

    let mut state = GameState::new(DRILL_WIDTH, DRILL_HEIGHT, DRILL_PLAYERS, heightfield);

    // Spawn soldier A at hex (1,1), facing east.
    let hex_a = Axial::new(1, 1);
    let pos_a = hex_to_world(hex_a);
    let soldier_a = spawn_entity(
        &mut state,
        EntityBuilder::new()
            .pos(pos_a)
            .owner(0)
            .person(Person {
                role: Role::Soldier,
                combat_skill: 0.5,
                    task: None,
            })
            .mobile(Mobile::new(PERSON_STEERING, PERSON_RADIUS))
            .combatant(Combatant::new())
            .vitals()
            .equipment(Equipment::empty()),
    );

    // Give soldier A an iron sword.
    let sword_a = spawn_entity(
        &mut state,
        EntityBuilder::new()
            .owner(0)
            .weapon_props(weapon::iron_sword()),
    );
    contain(&mut state, soldier_a, sword_a);
    if let Some(e) = state.entities.get_mut(soldier_a)
        && let Some(eq) = &mut e.equipment
    {
        eq.weapon = Some(sword_a);
    }

    // Spawn soldier B at hex (3,1), facing west.
    let hex_b = Axial::new(3, 1);
    let pos_b = hex_to_world(hex_b);
    let soldier_b = spawn_entity(
        &mut state,
        EntityBuilder::new()
            .pos(pos_b)
            .owner(1)
            .person(Person {
                role: Role::Soldier,
                combat_skill: 0.5,
                    task: None,
            })
            .mobile(Mobile::new(PERSON_STEERING, PERSON_RADIUS))
            .combatant(Combatant {
                facing: std::f32::consts::PI, // facing west
                ..Combatant::new()
            })
            .vitals()
            .equipment(Equipment::empty()),
    );

    // Give soldier B an iron sword.
    let sword_b = spawn_entity(
        &mut state,
        EntityBuilder::new()
            .owner(1)
            .weapon_props(weapon::iron_sword()),
    );
    contain(&mut state, soldier_b, sword_b);
    if let Some(e) = state.entities.get_mut(soldier_b)
        && let Some(eq) = &mut e.equipment
    {
        eq.weapon = Some(sword_b);
    }

    state
}

// ---------------------------------------------------------------------------
// Zoo — visual state showcase
// ---------------------------------------------------------------------------

const ZOO_WIDTH: usize = 5;
const ZOO_HEIGHT: usize = 3;
const ZOO_PLAYERS: u8 = 2;

/// Spawn a soldier at a world position with a weapon, return the soldier key.
fn zoo_spawn_soldier(state: &mut GameState, pos: Vec3, owner: u8, facing: f32) -> EntityKey {
    let soldier = spawn_entity(
        state,
        EntityBuilder::new()
            .pos(pos)
            .owner(owner)
            .person(Person {
                role: Role::Soldier,
                combat_skill: 0.5,
                    task: None,
            })
            .mobile(Mobile::new(PERSON_STEERING, PERSON_RADIUS))
            .combatant(Combatant {
                facing,
                ..Combatant::new()
            })
            .vitals()
            .equipment(Equipment::empty()),
    );
    let sword = spawn_entity(
        state,
        EntityBuilder::new()
            .owner(owner)
            .weapon_props(weapon::iron_sword()),
    );
    contain(state, soldier, sword);
    if let Some(e) = state.entities.get_mut(soldier)
        && let Some(eq) = &mut e.equipment
    {
        eq.weapon = Some(sword);
    }
    soldier
}

fn create_zoo_state() -> GameState {
    let vertex_cols = ZOO_WIDTH * 2 + 1;
    let vertex_rows = ZOO_HEIGHT * 2 + 1;
    let heightfield = Heightfield::new(vertex_cols, vertex_rows, 0.0, GeoMaterial::Soil);
    let mut state = GameState::new(ZOO_WIDTH, ZOO_HEIGHT, ZOO_PLAYERS, heightfield);

    let east = 0.0_f32;
    let dummy_key = EntityKey::default();

    // All entities packed into hex(2,1) area.  Spacing ~25 world units
    // (~17m) between each — enough to see at close zoom, fits within
    // a single hex diameter (150 world units).
    let center = hex_to_world(Axial::new(2, 1));
    let spacing = 25.0_f32;

    // --- Top row: combat phases, y offset -40 from hex center ---
    let row_y = center.y - 40.0;
    let row_start_x = center.x - 2.5 * spacing;

    // 0: Idle
    zoo_spawn_soldier(&mut state, Vec3::new(row_start_x, row_y, 0.0), 0, east);

    // 1: Windup — generic
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + spacing, row_y, 0.0),
        0,
        east,
    );
    if let Some(e) = state.entities.get_mut(s)
        && let Some(c) = &mut e.combatant
    {
        let wk = e.equipment.as_ref().and_then(|eq| eq.weapon).unwrap();
        let mut atk = weapon::AttackState::new(s, wk);
        atk.progress = 1.0;
        atk.motion = AttackMotion::Generic;
        c.attack = Some(atk);
    }

    // 2: Windup — overhead
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + 2.0 * spacing, row_y, 0.0),
        0,
        east,
    );
    if let Some(e) = state.entities.get_mut(s)
        && let Some(c) = &mut e.combatant
    {
        let wk = e.equipment.as_ref().and_then(|eq| eq.weapon).unwrap();
        let mut atk = weapon::AttackState::new(s, wk);
        atk.progress = 2.0;
        atk.motion = AttackMotion::Overhead;
        c.attack = Some(atk);
    }

    // 3: Committed — forehand
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + 3.0 * spacing, row_y, 0.0),
        0,
        east,
    );
    if let Some(e) = state.entities.get_mut(s)
        && let Some(c) = &mut e.combatant
    {
        let wk = e.equipment.as_ref().and_then(|eq| eq.weapon).unwrap();
        let mut atk = weapon::AttackState::new(s, wk);
        atk.progress = 3.0;
        atk.committed = true;
        atk.motion = AttackMotion::Forehand;
        c.attack = Some(atk);
    }

    // 4: Committed — backhand
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + 4.0 * spacing, row_y, 0.0),
        0,
        east,
    );
    if let Some(e) = state.entities.get_mut(s)
        && let Some(c) = &mut e.combatant
    {
        let wk = e.equipment.as_ref().and_then(|eq| eq.weapon).unwrap();
        let mut atk = weapon::AttackState::new(s, wk);
        atk.progress = 3.0;
        atk.committed = true;
        atk.motion = AttackMotion::Backhand;
        c.attack = Some(atk);
    }

    // 5: Recovery
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + 5.0 * spacing, row_y, 0.0),
        0,
        east,
    );
    if let Some(e) = state.entities.get_mut(s)
        && let Some(c) = &mut e.combatant
    {
        c.cooldown = Some(weapon::CooldownState::new(3));
    }

    // --- Bottom row: wound variations, y offset +40 from hex center ---
    let row_y = center.y + 40.0;

    // 0: Head scratch
    let s = zoo_spawn_soldier(&mut state, Vec3::new(row_start_x, row_y, 0.0), 1, east);
    if let Some(e) = state.entities.get_mut(s) {
        let wounds = e.wounds.get_or_insert_with(Default::default);
        wounds.push(Wound {
            zone: BodyZone::Head,
            severity: Severity::Scratch,
            bleed_rate: 0.01,
            damage_type: DamageType::Slash,
            attacker_id: dummy_key,
            created_at: 0,
        });
    }

    // 1: Torso laceration, 70% blood
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + spacing, row_y, 0.0),
        1,
        east,
    );
    if let Some(e) = state.entities.get_mut(s) {
        let wounds = e.wounds.get_or_insert_with(Default::default);
        wounds.push(Wound {
            zone: BodyZone::Torso,
            severity: Severity::Laceration,
            bleed_rate: 0.03,
            damage_type: DamageType::Slash,
            attacker_id: dummy_key,
            created_at: 0,
        });
        if let Some(v) = &mut e.vitals {
            v.blood = 0.7;
        }
    }

    // 2: Left arm puncture, 40% blood
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + 2.0 * spacing, row_y, 0.0),
        1,
        east,
    );
    if let Some(e) = state.entities.get_mut(s) {
        let wounds = e.wounds.get_or_insert_with(Default::default);
        wounds.push(Wound {
            zone: BodyZone::LeftArm,
            severity: Severity::Puncture,
            bleed_rate: 0.05,
            damage_type: DamageType::Slash,
            attacker_id: dummy_key,
            created_at: 0,
        });
        if let Some(v) = &mut e.vitals {
            v.blood = 0.4;
        }
    }

    // 3: Multi-wound (head + torso + right arm), 25% blood, 30% stamina
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + 3.0 * spacing, row_y, 0.0),
        1,
        east,
    );
    if let Some(e) = state.entities.get_mut(s) {
        let wounds = e.wounds.get_or_insert_with(Default::default);
        wounds.push(Wound {
            zone: BodyZone::Head,
            severity: Severity::Scratch,
            bleed_rate: 0.01,
            damage_type: DamageType::Slash,
            attacker_id: dummy_key,
            created_at: 0,
        });
        wounds.push(Wound {
            zone: BodyZone::Torso,
            severity: Severity::Laceration,
            bleed_rate: 0.03,
            damage_type: DamageType::Pierce,
            attacker_id: dummy_key,
            created_at: 0,
        });
        wounds.push(Wound {
            zone: BodyZone::RightArm,
            severity: Severity::Puncture,
            bleed_rate: 0.04,
            damage_type: DamageType::Slash,
            attacker_id: dummy_key,
            created_at: 0,
        });
        if let Some(v) = &mut e.vitals {
            v.blood = 0.25;
            v.stamina = 0.3;
        }
    }

    // 4: Low stamina only
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + 4.0 * spacing, row_y, 0.0),
        1,
        east,
    );
    if let Some(e) = state.entities.get_mut(s)
        && let Some(v) = &mut e.vitals
    {
        v.stamina = 0.15;
    }

    // 5: Near death — legs + torso punctures, 10% blood, 5% stamina
    let s = zoo_spawn_soldier(
        &mut state,
        Vec3::new(row_start_x + 5.0 * spacing, row_y, 0.0),
        1,
        east,
    );
    if let Some(e) = state.entities.get_mut(s) {
        let wounds = e.wounds.get_or_insert_with(Default::default);
        wounds.push(Wound {
            zone: BodyZone::Legs,
            severity: Severity::Puncture,
            bleed_rate: 0.06,
            damage_type: DamageType::Slash,
            attacker_id: dummy_key,
            created_at: 0,
        });
        wounds.push(Wound {
            zone: BodyZone::Torso,
            severity: Severity::Puncture,
            bleed_rate: 0.05,
            damage_type: DamageType::Pierce,
            attacker_id: dummy_key,
            created_at: 0,
        });
        if let Some(v) = &mut e.vitals {
            v.blood = 0.1;
            v.stamina = 0.05;
        }
    }

    state
}

// ---------------------------------------------------------------------------
// Command application
// ---------------------------------------------------------------------------

fn apply_drill_command(inner: &mut DrillInner, cmd: &DrillCommand) {
    let key = match resolve_entity_key(inner, cmd.entity) {
        Some(k) => k,
        None => return,
    };

    if let Some(facing) = cmd.facing
        && let Some(e) = inner.state.entities.get_mut(key)
        && let Some(c) = &mut e.combatant
    {
        c.facing = facing;
    }

    if let Some([x, y]) = cmd.set_pos
        && let Some(e) = inner.state.entities.get_mut(key)
    {
        let z = e.pos.map(|p| p.z).unwrap_or(0.0);
        e.pos = Some(Vec3 { x, y, z });
        // Update hex.
        e.hex = Some(simulate_everything_engine::v3::hex::world_to_hex(Vec3 {
            x,
            y,
            z,
        }));
    }

    if let Some([x, y]) = cmd.move_to
        && let Some(e) = inner.state.entities.get_mut(key)
        && let Some(m) = &mut e.mobile
    {
        m.waypoints = vec![Vec3 { x, y, z: 0.0 }];
    }

    if let Some(target_id) = cmd.attack {
        let target_key = resolve_entity_key(inner, target_id);
        if let Some(tk) = target_key {
            // Find entity's weapon key from equipment.
            let weapon_key = inner
                .state
                .entities
                .get(key)
                .and_then(|e| e.equipment.as_ref())
                .and_then(|eq| eq.weapon);
            if let Some(wk) = weapon_key
                && let Some(e) = inner.state.entities.get_mut(key)
                && let Some(c) = &mut e.combatant
            {
                c.target = Some(tk);
                // Directly initiate attack — bypasses agent/tactical layer.
                c.attack = Some(simulate_everything_engine::v3::weapon::AttackState::new(
                    tk, wk,
                ));
            }
        }
    }
}

fn resolve_entity_key(inner: &DrillInner, id: u32) -> Option<EntityKey> {
    // Try exact ID match first.
    if let Some(&(_, key)) = inner.entity_keys.iter().find(|(eid, _)| *eid == id) {
        return Some(key);
    }
    // Try 0-indexed shorthand.
    if (id as usize) < inner.entity_keys.len() {
        return Some(inner.entity_keys[id as usize].1);
    }
    None
}

// ---------------------------------------------------------------------------
// State extraction
// ---------------------------------------------------------------------------

fn build_drill_entities(state: &GameState, keys: &[(u32, EntityKey)]) -> Vec<DrillEntity> {
    let snapshot_entities = v3_protocol::build_snapshot(state, 0.05).entities;
    keys.iter()
        .filter_map(|(id, _)| {
            snapshot_entities
                .iter()
                .find(|e| e.id == *id)
                .map(|info| DrillEntity {
                    id: info.id,
                    owner: info.owner,
                    x: info.x,
                    y: info.y,
                    z: info.z,
                    hex_q: info.hex_q,
                    hex_r: info.hex_r,
                    facing: info.facing,
                    blood: info.blood,
                    stamina: info.stamina,
                    attack_phase: info.attack_phase.clone(),
                    attack_motion: info.attack_motion.clone(),
                    weapon_angle: info.weapon_angle,
                    attack_progress: info.attack_progress,
                    weapon_type: info.weapon_type.clone(),
                    wounds: info
                        .wounds
                        .iter()
                        .map(|(z, s)| format!("{:?}:{:?}", z, s))
                        .collect(),
                })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// ASCII renderer
// ---------------------------------------------------------------------------

/// Render a focused ASCII view of the drill pad.
/// Shows hex grid with entity positions marked.
fn render_ascii(state: &GameState, keys: &[(u32, EntityKey)], _center: Option<Axial>) -> String {
    let mut lines = Vec::new();

    // Header with entity positions.
    lines.push(format!(
        "Tick {:>5}  |  {} entities",
        state.tick,
        keys.len()
    ));
    lines.push(String::new());

    // Entity detail lines.
    for (id, key) in keys {
        if let Some(e) = state.entities.get(*key) {
            let pos = e.pos.unwrap_or(Vec3::ZERO);
            let hex = e.hex.unwrap_or(Axial::new(0, 0));
            let facing = e
                .combatant
                .as_ref()
                .map(|c| format!("{:.1}°", c.facing.to_degrees()))
                .unwrap_or_default();
            let blood = e
                .vitals
                .as_ref()
                .map(|v| format!("{:.0}%", v.blood * 100.0))
                .unwrap_or_default();
            let stamina = e
                .vitals
                .as_ref()
                .map(|v| format!("{:.0}%", v.stamina * 100.0))
                .unwrap_or_default();

            let phase = e
                .combatant
                .as_ref()
                .map(|c| {
                    if c.attack.is_some() {
                        let atk = c.attack.as_ref().unwrap();
                        if atk.committed {
                            format!("COMMIT {:?}", atk.motion)
                        } else {
                            format!("WINDUP {:?}", atk.motion)
                        }
                    } else if c.cooldown.is_some() {
                        "RECOVERY".to_string()
                    } else {
                        "idle".to_string()
                    }
                })
                .unwrap_or_default();

            let owner_char = e.owner.map(|o| (b'A' + o) as char).unwrap_or('?');

            lines.push(format!(
                "  [{}] P{} ({:>6.1},{:>6.1}) hex({},{}) face={} blood={} stam={} {}",
                id, owner_char, pos.x, pos.y, hex.q, hex.r, facing, blood, stamina, phase
            ));
        }
    }

    lines.push(String::new());

    // Simple hex grid.
    // Show each hex as a cell, entities marked by owner letter.
    let mut grid: Vec<Vec<char>> = vec![vec!['.'; DRILL_WIDTH]; DRILL_HEIGHT];
    for (_, key) in keys {
        if let Some(e) = state.entities.get(*key) {
            let hex = e.hex.unwrap_or(Axial::new(0, 0));
            // Convert axial to offset for grid display.
            let col = hex.q as usize;
            let row = hex.r as usize;
            if row < DRILL_HEIGHT && col < DRILL_WIDTH {
                let ch = e.owner.map(|o| (b'A' + o) as char).unwrap_or('?');
                grid[row][col] = ch;
            }
        }
    }

    lines.push("  q→ 0 1 2 3 4".to_string());
    for (r, row) in grid.iter().enumerate() {
        let indent = if r % 2 == 1 { " " } else { "" };
        let cells: String = row.iter().map(|c| format!("{} ", c)).collect();
        lines.push(format!("r={} {}{}", r, indent, cells.trim_end()));
    }

    lines.join("\n")
}
