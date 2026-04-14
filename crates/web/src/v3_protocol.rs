use std::collections::HashMap;

use serde::Serialize;
use simulate_everything_engine::v3::{
    armor::{BodyZone, DamageType},
    formation::FormationType,
    spatial::Vec3,
    state::{GameState, ResourceType, Role, StackId, StructureType},
    wound::Severity,
};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Entity kind discriminator for the wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EntityKind {
    Person,
    Structure,
}

/// Simulation time resolution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TimeMode {
    /// dt = 3600.0 — economy and strategic movement.
    Strategic,
    /// dt = 1.0 — default, full combat fidelity.
    Tactical,
    /// dt = 0.01 — frame-by-frame combat.
    Cinematic,
}

impl TimeMode {
    pub fn dt(&self) -> f32 {
        match self {
            TimeMode::Strategic => 3600.0,
            TimeMode::Tactical => 1.0,
            TimeMode::Cinematic => 0.01,
        }
    }
}

/// 2-bit wound severity for spectator wire protocol.
/// Coarser than engine's `Severity` — maps Scratch→Light, Laceration→Light,
/// Puncture→Serious, Fracture→Critical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WoundSeverity {
    Light,
    Serious,
    Critical,
}

impl From<Severity> for WoundSeverity {
    fn from(s: Severity) -> Self {
        match s {
            Severity::Scratch | Severity::Laceration => WoundSeverity::Light,
            Severity::Puncture => WoundSeverity::Serious,
            Severity::Fracture => WoundSeverity::Critical,
        }
    }
}

// ---------------------------------------------------------------------------
// Wire types — server to spectator messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum V3ServerToSpectator {
    #[serde(rename = "v3_init")]
    Init {
        #[serde(flatten)]
        init: V3Init,
        game_number: u64,
    },
    #[serde(rename = "v3_snapshot")]
    Snapshot {
        #[serde(flatten)]
        snapshot: V3Snapshot,
    },
    #[serde(rename = "v3_snapshot_delta")]
    SnapshotDelta {
        #[serde(flatten)]
        delta: V3SnapshotDelta,
    },
    #[serde(rename = "v3_game_end")]
    GameEnd {
        winner: Option<u8>,
        tick: u64,
        timed_out: bool,
        scores: Vec<u32>,
    },
    #[serde(rename = "v3_config")]
    Config {
        #[serde(skip_serializing_if = "Option::is_none")]
        tick_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mode: Option<TimeMode>,
        #[serde(skip_serializing_if = "Option::is_none")]
        autoplay: Option<bool>,
    },
    #[serde(rename = "v3_rr_status")]
    RrStatus(V3RrStatus),
}

// ---------------------------------------------------------------------------
// Init — sent once on spectator connect
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct V3Init {
    pub width: u32,
    pub height: u32,
    pub terrain: Vec<f32>,
    pub height_map: Vec<f32>,
    pub material_map: Vec<f32>,
    pub region_ids: Vec<u16>,
    pub player_count: u8,
    pub agent_names: Vec<String>,
    pub agent_versions: Vec<String>,
    pub game_number: u64,
}

// ---------------------------------------------------------------------------
// Snapshot — full state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct V3Snapshot {
    pub tick: u64,
    pub dt: f32,
    pub full_state: bool,
    pub entities: Vec<SpectatorEntityInfo>,
    pub projectiles: Vec<ProjectileInfo>,
    pub stacks: Vec<StackInfo>,
    pub hex_ownership: Vec<Option<u8>>,
    pub hex_roads: Vec<u8>,
    pub hex_structures: Vec<Option<u32>>,
    pub players: Vec<PlayerInfo>,
}

// ---------------------------------------------------------------------------
// SnapshotDelta — per-tick update
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct V3SnapshotDelta {
    pub tick: u64,
    pub dt: f32,
    pub full_state: bool,
    pub entities_appeared: Vec<SpectatorEntityInfo>,
    pub entities_updated: Vec<EntityUpdate>,
    pub entities_removed: Vec<u32>,
    pub projectiles_spawned: Vec<ProjectileInfo>,
    pub projectiles_removed: Vec<u32>,
    pub stacks_created: Vec<StackInfo>,
    pub stacks_updated: Vec<StackUpdate>,
    pub stacks_dissolved: Vec<u32>,
    pub hex_changes: Vec<HexDelta>,
    pub players: Vec<PlayerInfo>,
}

// ---------------------------------------------------------------------------
// Entity info — flat struct with optional fields
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SpectatorEntityInfo {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<u8>,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub hex_q: i32,
    pub hex_r: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facing: Option<f32>,
    pub entity_kind: EntityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blood: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamina: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wounds: Vec<(BodyZone, WoundSeverity)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<ResourceType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_amount: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_type: Option<StructureType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_progress: Option<f32>,
    pub contains_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,
}

// ---------------------------------------------------------------------------
// Entity update — changed fields only (for delta snapshots)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct EntityUpdate {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex_q: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex_r: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facing: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blood: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamina: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wounds: Option<Vec<(BodyZone, WoundSeverity)>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contains_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_id: Option<Option<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<Option<String>>,
}

// ---------------------------------------------------------------------------
// Projectile info
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ProjectileInfo {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    pub damage_type: DamageType,
}

// ---------------------------------------------------------------------------
// Stack info
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct StackInfo {
    pub id: u32,
    pub owner: u8,
    pub members: Vec<u32>,
    pub formation: FormationType,
    pub center_x: f32,
    pub center_y: f32,
    pub facing: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackUpdate {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formation: Option<FormationType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facing: Option<f32>,
}

// ---------------------------------------------------------------------------
// Player info — faction-level aggregates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct PlayerInfo {
    pub id: u8,
    pub population: u32,
    pub territory: u32,
    pub food_level: u8,
    pub material_level: u8,
    pub alive: bool,
    pub score: u32,
}

// ---------------------------------------------------------------------------
// Hex delta
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct HexDelta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub road_level: Option<u8>,
    /// `Some(None)` = structure removed, `Some(Some(id))` = structure placed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_id: Option<Option<u32>>,
}

// ---------------------------------------------------------------------------
// RR status — broadcast on state change
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct V3RrStatus {
    pub game_number: u64,
    pub current_tick: u64,
    pub dt: f32,
    pub mode: TimeMode,
    pub paused: bool,
    pub tick_ms: u64,
    pub autoplay: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capturable_start_tick: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capturable_end_tick: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_capture: Option<String>,
}

// ---------------------------------------------------------------------------
// Snapshot builders — GameState → wire types
// ---------------------------------------------------------------------------

/// Build a full V3Snapshot from engine state.
pub fn build_snapshot(state: &GameState, dt: f32) -> V3Snapshot {
    let entities = build_entity_list(state);
    let projectiles = build_projectile_list(state);
    let stacks = build_stack_list(state);

    let hex_count = state.map_width * state.map_height;

    // Territory, roads, structures — V3 engine doesn't have per-hex ownership yet,
    // so initialize empty. The RR loop will populate these as the engine evolves.
    let hex_ownership = vec![None; hex_count];
    let hex_roads = vec![0u8; hex_count];
    let hex_structures = vec![None; hex_count];

    let players = build_player_list(state);

    V3Snapshot {
        tick: state.tick,
        dt,
        full_state: true,
        entities,
        projectiles,
        stacks,
        hex_ownership,
        hex_roads,
        hex_structures,
        players,
    }
}

/// Build a V3Init from engine state and game metadata.
pub fn build_init(
    state: &GameState,
    agent_names: &[String],
    agent_versions: &[String],
    game_number: u64,
) -> V3Init {
    let hex_count = state.map_width * state.map_height;

    // Extract terrain data from the heightfield vertex grid.
    // The heightfield has its own cols×rows grid — sample one vertex per hex.
    let hf = &state.heightfield;
    let height_map: Vec<f32> = (0..hex_count)
        .map(|i| {
            let col = i % state.map_width;
            let row = i / state.map_width;
            // Clamp to heightfield bounds.
            let hf_col = col.min(hf.cols.saturating_sub(1));
            let hf_row = row.min(hf.rows.saturating_sub(1));
            hf.vertex_at(hf_col, hf_row)
                .map(|v| v.height)
                .unwrap_or(0.0)
        })
        .collect();

    // Material richness — use material enum ordinal as a proxy.
    let material_map: Vec<f32> = (0..hex_count)
        .map(|i| {
            let col = i % state.map_width;
            let row = i / state.map_width;
            let hf_col = col.min(hf.cols.saturating_sub(1));
            let hf_row = row.min(hf.rows.saturating_sub(1));
            hf.vertex_at(hf_col, hf_row)
                .map(|v| v.material as u8 as f32 / 4.0)
                .unwrap_or(0.0)
        })
        .collect();

    let terrain = height_map.clone();
    let region_ids = vec![0u16; hex_count];

    V3Init {
        width: state.map_width as u32,
        height: state.map_height as u32,
        terrain,
        height_map,
        material_map,
        region_ids,
        player_count: state.num_players,
        agent_names: agent_names.to_vec(),
        agent_versions: agent_versions.to_vec(),
        game_number,
    }
}

// ---------------------------------------------------------------------------
// Internal builders
// ---------------------------------------------------------------------------

fn build_entity_list(state: &GameState) -> Vec<SpectatorEntityInfo> {
    let mut entities = Vec::new();

    for (_key, entity) in &state.entities {
        // Skip entities without a position (contained in another entity).
        let pos = match entity.pos {
            Some(p) => p,
            None => continue,
        };

        // Skip projectile entities — they go in the projectile list.
        if entity.projectile.is_some() {
            continue;
        }

        let hex = entity.hex.unwrap_or_else(|| {
            simulate_everything_engine::v2::hex::Axial::new(0, 0)
        });

        let entity_kind = if entity.structure.is_some() {
            EntityKind::Structure
        } else {
            EntityKind::Person
        };

        let role = entity.person.as_ref().map(|p| p.role);
        let facing = entity.combatant.as_ref().map(|c| c.facing);
        let blood = entity.vitals.as_ref().map(|v| v.blood);
        let stamina = entity.vitals.as_ref().map(|v| v.stamina);

        // Wound approximation: zone + 2-bit severity for spectators.
        let wounds: Vec<(BodyZone, WoundSeverity)> = entity
            .wounds
            .as_ref()
            .map(|wl| {
                wl.iter()
                    .map(|w| (w.zone, WoundSeverity::from(w.severity)))
                    .collect()
            })
            .unwrap_or_default();

        // Equipment names: derive from material + damage type.
        let weapon_type = entity.equipment.as_ref().and_then(|eq| {
            eq.weapon.and_then(|wk| {
                state
                    .entities
                    .get(wk)
                    .and_then(|we| we.weapon_props.as_ref())
                    .map(|wp| format!("{:?} {:?}", wp.material, wp.damage_type))
            })
        });

        let armor_type = entity.equipment.as_ref().and_then(|eq| {
            eq.armor_slots.iter().find_map(|slot| {
                slot.and_then(|ak| {
                    state
                        .entities
                        .get(ak)
                        .and_then(|ae| ae.armor_props.as_ref())
                        .map(|ap| format!("{:?} {:?}", ap.material, ap.construction))
                })
            })
        });

        let resource_type = entity.resource.as_ref().map(|r| r.resource_type);
        let resource_amount = entity.resource.as_ref().map(|r| r.amount);

        let structure_type = entity.structure.as_ref().map(|s| s.structure_type);
        let build_progress = entity.structure.as_ref().map(|s| s.build_progress);

        // Stack membership: find which stack this entity belongs to.
        let stack_id = state
            .stacks
            .iter()
            .find(|s| s.members.contains(&_key))
            .map(|s| s.id.0);

        entities.push(SpectatorEntityInfo {
            id: entity.id,
            owner: entity.owner,
            x: pos.x,
            y: pos.y,
            z: pos.z,
            hex_q: hex.q,
            hex_r: hex.r,
            facing,
            entity_kind,
            role,
            blood,
            stamina,
            wounds,
            weapon_type,
            armor_type,
            resource_type,
            resource_amount,
            structure_type,
            build_progress,
            contains_count: entity.contains.len(),
            stack_id,
            current_task: None, // Agent layer not yet integrated.
        });
    }

    entities
}

fn build_projectile_list(state: &GameState) -> Vec<ProjectileInfo> {
    let mut projectiles = Vec::new();

    for (_key, entity) in &state.entities {
        if let (Some(pos), Some(proj)) = (entity.pos, entity.projectile.as_ref()) {
            let vel = entity.mobile.as_ref().map(|m| m.vel).unwrap_or(Vec3::ZERO);
            projectiles.push(ProjectileInfo {
                id: entity.id,
                x: pos.x,
                y: pos.y,
                z: pos.z,
                vx: vel.x,
                vy: vel.y,
                vz: vel.z,
                damage_type: proj.damage_type,
            });
        }
    }

    projectiles
}

fn build_stack_list(state: &GameState) -> Vec<StackInfo> {
    state
        .stacks
        .iter()
        .map(|stack| {
            // Compute stack center from member positions.
            let (cx, cy, count) = stack.members.iter().fold(
                (0.0f32, 0.0f32, 0u32),
                |(sx, sy, n), &key| {
                    if let Some(entity) = state.entities.get(key) {
                        if let Some(pos) = entity.pos {
                            return (sx + pos.x, sy + pos.y, n + 1);
                        }
                    }
                    (sx, sy, n)
                },
            );
            let (cx, cy) = if count > 0 {
                (cx / count as f32, cy / count as f32)
            } else {
                (0.0, 0.0)
            };

            // Stack facing from leader's combatant component.
            let facing = state
                .entities
                .get(stack.leader)
                .and_then(|e| e.combatant.as_ref())
                .map(|c| c.facing)
                .unwrap_or(0.0);

            StackInfo {
                id: stack.id.0,
                owner: stack.owner,
                members: stack.members.iter().filter_map(|&k| {
                    state.entities.get(k).map(|e| e.id)
                }).collect(),
                formation: stack.formation,
                center_x: cx,
                center_y: cy,
                facing,
            }
        })
        .collect()
}

fn build_player_list(state: &GameState) -> Vec<PlayerInfo> {
    (0..state.num_players)
        .map(|player_id| {
            let mut population = 0u32;
            let mut alive_entities = false;

            for (_key, entity) in &state.entities {
                if entity.owner == Some(player_id) && entity.person.is_some() {
                    population += 1;
                    if entity
                        .vitals
                        .as_ref()
                        .map(|v| v.blood > 0.0)
                        .unwrap_or(true)
                    {
                        alive_entities = true;
                    }
                }
            }

            PlayerInfo {
                id: player_id,
                population,
                territory: 0, // Hex ownership not yet tracked by engine.
                food_level: 0,
                material_level: 0,
                alive: alive_entities || population > 0,
                score: population, // Simple score = population for now.
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Delta tracker — compares consecutive snapshots to produce deltas
// ---------------------------------------------------------------------------

/// Tracks previous tick's state to compute deltas for spectator streaming.
pub struct DeltaTracker {
    prev_entities: HashMap<u32, SpectatorEntityInfo>,
    prev_projectiles: HashMap<u32, ProjectileInfo>,
    prev_stacks: HashMap<u32, StackInfo>,
}

impl DeltaTracker {
    pub fn new() -> Self {
        Self {
            prev_entities: HashMap::new(),
            prev_projectiles: HashMap::new(),
            prev_stacks: HashMap::new(),
        }
    }

    /// Reset tracker state (e.g., on new game).
    pub fn reset(&mut self) {
        self.prev_entities.clear();
        self.prev_projectiles.clear();
        self.prev_stacks.clear();
    }

    /// Build a delta from the current game state compared to the previous tick.
    /// Updates internal state for the next comparison.
    pub fn build_delta(&mut self, state: &GameState, dt: f32) -> V3SnapshotDelta {
        let cur_entities = build_entity_list(state);
        let cur_projectiles = build_projectile_list(state);
        let cur_stacks = build_stack_list(state);
        let players = build_player_list(state);

        // --- Entities ---
        let cur_entity_map: HashMap<u32, &SpectatorEntityInfo> =
            cur_entities.iter().map(|e| (e.id, e)).collect();
        let cur_entity_ids: std::collections::HashSet<u32> =
            cur_entities.iter().map(|e| e.id).collect();
        let prev_entity_ids: std::collections::HashSet<u32> =
            self.prev_entities.keys().copied().collect();

        // Appeared: in current but not in previous.
        let entities_appeared: Vec<SpectatorEntityInfo> = cur_entities
            .iter()
            .filter(|e| !prev_entity_ids.contains(&e.id))
            .cloned()
            .collect();

        // Removed: in previous but not in current.
        let entities_removed: Vec<u32> = prev_entity_ids
            .difference(&cur_entity_ids)
            .copied()
            .collect();

        // Updated: in both, with changed fields.
        let mut entities_updated: Vec<EntityUpdate> = Vec::new();
        for e in &cur_entities {
            if let Some(prev) = self.prev_entities.get(&e.id) {
                if let Some(update) = diff_entity(prev, e) {
                    entities_updated.push(update);
                }
            }
        }

        // --- Projectiles ---
        let cur_proj_map: HashMap<u32, &ProjectileInfo> =
            cur_projectiles.iter().map(|p| (p.id, p)).collect();
        let cur_proj_ids: std::collections::HashSet<u32> =
            cur_projectiles.iter().map(|p| p.id).collect();
        let prev_proj_ids: std::collections::HashSet<u32> =
            self.prev_projectiles.keys().copied().collect();

        let projectiles_spawned: Vec<ProjectileInfo> = cur_projectiles
            .iter()
            .filter(|p| !prev_proj_ids.contains(&p.id))
            .cloned()
            .collect();
        let projectiles_removed: Vec<u32> = prev_proj_ids
            .difference(&cur_proj_ids)
            .copied()
            .collect();

        // --- Stacks ---
        let cur_stack_ids: std::collections::HashSet<u32> =
            cur_stacks.iter().map(|s| s.id).collect();
        let prev_stack_ids: std::collections::HashSet<u32> =
            self.prev_stacks.keys().copied().collect();

        let stacks_created: Vec<StackInfo> = cur_stacks
            .iter()
            .filter(|s| !prev_stack_ids.contains(&s.id))
            .cloned()
            .collect();
        let stacks_dissolved: Vec<u32> = prev_stack_ids
            .difference(&cur_stack_ids)
            .copied()
            .collect();

        let mut stacks_updated: Vec<StackUpdate> = Vec::new();
        for s in &cur_stacks {
            if let Some(prev) = self.prev_stacks.get(&s.id) {
                if let Some(update) = diff_stack(prev, s) {
                    stacks_updated.push(update);
                }
            }
        }

        // Update previous state for next tick.
        self.prev_entities = cur_entities.into_iter().map(|e| (e.id, e)).collect();
        self.prev_projectiles = cur_projectiles.into_iter().map(|p| (p.id, p)).collect();
        self.prev_stacks = cur_stacks.into_iter().map(|s| (s.id, s)).collect();

        V3SnapshotDelta {
            tick: state.tick,
            dt,
            full_state: false,
            entities_appeared,
            entities_updated,
            entities_removed,
            projectiles_spawned,
            projectiles_removed,
            stacks_created,
            stacks_updated,
            stacks_dissolved,
            hex_changes: Vec::new(), // Hex ownership not yet tracked.
            players,
        }
    }

    /// Seed the tracker from a full snapshot (e.g., after game init).
    pub fn seed_from_snapshot(&mut self, snapshot: &V3Snapshot) {
        self.prev_entities = snapshot
            .entities
            .iter()
            .map(|e| (e.id, e.clone()))
            .collect();
        self.prev_projectiles = snapshot
            .projectiles
            .iter()
            .map(|p| (p.id, p.clone()))
            .collect();
        self.prev_stacks = snapshot
            .stacks
            .iter()
            .map(|s| (s.id, s.clone()))
            .collect();
    }
}

// ---------------------------------------------------------------------------
// Entity diffing
// ---------------------------------------------------------------------------

/// Position change threshold — skip update if entity moved less than this.
const POS_EPSILON: f32 = 0.01;
/// Facing change threshold (radians).
const FACING_EPSILON: f32 = 0.01;
/// Blood/stamina change threshold.
const VITAL_EPSILON: f32 = 0.001;

/// Compare two entity snapshots and return an EntityUpdate with only changed fields.
/// Returns None if nothing changed.
fn diff_entity(prev: &SpectatorEntityInfo, cur: &SpectatorEntityInfo) -> Option<EntityUpdate> {
    let mut update = EntityUpdate {
        id: cur.id,
        x: None,
        y: None,
        z: None,
        hex_q: None,
        hex_r: None,
        facing: None,
        blood: None,
        stamina: None,
        wounds: None,
        weapon_type: None,
        armor_type: None,
        contains_count: None,
        stack_id: None,
        current_task: None,
    };
    let mut changed = false;

    if (cur.x - prev.x).abs() > POS_EPSILON
        || (cur.y - prev.y).abs() > POS_EPSILON
        || (cur.z - prev.z).abs() > POS_EPSILON
    {
        update.x = Some(cur.x);
        update.y = Some(cur.y);
        update.z = Some(cur.z);
        changed = true;
    }

    if cur.hex_q != prev.hex_q || cur.hex_r != prev.hex_r {
        update.hex_q = Some(cur.hex_q);
        update.hex_r = Some(cur.hex_r);
        changed = true;
    }

    if let (Some(cf), Some(pf)) = (cur.facing, prev.facing) {
        if (cf - pf).abs() > FACING_EPSILON {
            update.facing = Some(cf);
            changed = true;
        }
    } else if cur.facing != prev.facing {
        update.facing = cur.facing;
        changed = true;
    }

    if let (Some(cb), Some(pb)) = (cur.blood, prev.blood) {
        if (cb - pb).abs() > VITAL_EPSILON {
            update.blood = Some(cb);
            changed = true;
        }
    }

    if let (Some(cs), Some(ps)) = (cur.stamina, prev.stamina) {
        if (cs - ps).abs() > VITAL_EPSILON {
            update.stamina = Some(cs);
            changed = true;
        }
    }

    if cur.wounds != prev.wounds {
        update.wounds = Some(cur.wounds.clone());
        changed = true;
    }

    if cur.weapon_type != prev.weapon_type {
        update.weapon_type = cur.weapon_type.clone();
        changed = true;
    }

    if cur.armor_type != prev.armor_type {
        update.armor_type = cur.armor_type.clone();
        changed = true;
    }

    if cur.contains_count != prev.contains_count {
        update.contains_count = Some(cur.contains_count);
        changed = true;
    }

    if cur.stack_id != prev.stack_id {
        update.stack_id = Some(cur.stack_id);
        changed = true;
    }

    if cur.current_task != prev.current_task {
        update.current_task = Some(cur.current_task.clone());
        changed = true;
    }

    if changed { Some(update) } else { None }
}

/// Compare two stack snapshots and return a StackUpdate with only changed fields.
fn diff_stack(prev: &StackInfo, cur: &StackInfo) -> Option<StackUpdate> {
    let mut update = StackUpdate {
        id: cur.id,
        members: None,
        formation: None,
        center_x: None,
        center_y: None,
        facing: None,
    };
    let mut changed = false;

    if cur.members != prev.members {
        update.members = Some(cur.members.clone());
        changed = true;
    }
    if cur.formation != prev.formation {
        update.formation = Some(cur.formation);
        changed = true;
    }
    if (cur.center_x - prev.center_x).abs() > POS_EPSILON
        || (cur.center_y - prev.center_y).abs() > POS_EPSILON
    {
        update.center_x = Some(cur.center_x);
        update.center_y = Some(cur.center_y);
        changed = true;
    }
    if (cur.facing - prev.facing).abs() > FACING_EPSILON {
        update.facing = Some(cur.facing);
        changed = true;
    }

    if changed { Some(update) } else { None }
}
