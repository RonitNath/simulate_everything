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
