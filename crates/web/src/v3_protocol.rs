use std::collections::HashMap;

use simulate_everything_engine::v3::{
    body_model::BodyPointId,
    derived::{derive_hex_control, derive_hex_structures, derive_player_stats, stockpile_level},
    spatial::Vec3,
    state::{GameState, TaskAssignment},
    wound::Severity,
};

// Re-export all wire types from the protocol crate.
pub use simulate_everything_protocol::{
    BodyPointWire, BodyRenderInfo, BodyZone, CapsuleWire, DiscWire, EntityKind, EntityUpdate,
    PlayerInfo, ProjectileInfo, Role, SpectatorEntityInfo, StackInfo, StackUpdate, TimeMode,
    V3Init, V3RrStatus, V3ServerToSpectator, V3Snapshot, V3SnapshotDelta, WoundSeverity,
};

/// Convert engine wound severity to wire wound severity.
fn map_severity(s: Severity) -> WoundSeverity {
    match s {
        Severity::Scratch | Severity::Laceration => WoundSeverity::Light,
        Severity::Puncture => WoundSeverity::Serious,
        Severity::Fracture => WoundSeverity::Critical,
    }
}

// ---------------------------------------------------------------------------
// Snapshot builders — GameState → wire types
// ---------------------------------------------------------------------------

/// Build a full V3Snapshot from engine state.
pub fn build_snapshot(state: &GameState, dt: f32) -> V3Snapshot {
    let entities = build_entity_list(state);
    let body_models = build_body_render_list(state);
    let projectiles = build_projectile_list(state);
    let stacks = build_stack_list(state);
    let hex_ownership = derive_hex_control(state)
        .into_iter()
        .map(|hex| hex.owner)
        .collect();
    let hex_roads = vec![0u8; state.map_width * state.map_height];
    let hex_structures = derive_hex_structures(state);

    let players = build_player_list(state);

    V3Snapshot {
        tick: state.tick,
        dt,
        full_state: true,
        entities,
        body_models,
        projectiles,
        stacks,
        hex_ownership,
        hex_roads,
        hex_structures,
        players,
    }
}

fn build_body_render_list(state: &GameState) -> Vec<BodyRenderInfo> {
    let mut body_models = Vec::new();

    for (_key, entity) in &state.entities {
        let Some(body) = entity.body.as_ref() else {
            continue;
        };

        let points = std::array::from_fn(|i| {
            let p = body.points[i].pos;
            BodyPointWire {
                x: p.x,
                y: p.y,
                z: p.z,
            }
        });

        let facing = entity.combatant.as_ref().map(|c| c.facing).unwrap_or(0.0);
        let weapon = build_weapon_capsule(state, entity, body.as_ref(), facing);
        let shield = build_shield_disc(state, entity, body.as_ref(), facing);

        body_models.push(BodyRenderInfo {
            entity_id: entity.id,
            points,
            weapon,
            shield,
        });
    }

    body_models
}

fn build_weapon_capsule(
    state: &GameState,
    entity: &simulate_everything_engine::v3::state::Entity,
    body: &simulate_everything_engine::v3::body_model::BodyModel,
    facing: f32,
) -> Option<CapsuleWire> {
    let weapon_key = entity.equipment.as_ref()?.weapon?;
    let weapon = state.entities.get(weapon_key)?.weapon_props.as_ref()?;
    if weapon.reach <= 0.0 {
        return None;
    }

    let hand = body.point(BodyPointId::RightHand).pos;
    let elbow = body.point(BodyPointId::RightElbow).pos;
    let hand_dir = safe_normalize(hand - elbow);
    let facing_dir = Vec3::new(facing.cos(), facing.sin(), 0.0);
    let dir = if hand_dir.length_squared() > 1e-4 {
        hand_dir
    } else {
        facing_dir
    };
    let tip = hand + dir * weapon.reach.max(0.4);

    Some(CapsuleWire {
        a: [hand.x, hand.y, hand.z],
        b: [tip.x, tip.y, tip.z],
        radius: (weapon.weight * 0.02).clamp(0.025, 0.08),
    })
}

fn build_shield_disc(
    state: &GameState,
    entity: &simulate_everything_engine::v3::state::Entity,
    body: &simulate_everything_engine::v3::body_model::BodyModel,
    facing: f32,
) -> Option<DiscWire> {
    let shield_key = entity.equipment.as_ref()?.shield?;
    let shield = state.entities.get(shield_key)?.weapon_props.as_ref()?;
    let left_hand = body.point(BodyPointId::LeftHand).pos;
    let left_elbow = body.point(BodyPointId::LeftElbow).pos;
    let guard_dir = safe_normalize(left_hand - left_elbow);
    let normal = if guard_dir.length_squared() > 1e-4 {
        guard_dir
    } else {
        Vec3::new(facing.cos(), facing.sin(), 0.0)
    };
    let center = left_hand + normal * 0.15;

    Some(DiscWire {
        center: [center.x, center.y, center.z],
        normal: [normal.x, normal.y, normal.z],
        radius: shield.block_arc.clamp(0.45, 1.5) * 0.35,
    })
}

fn safe_normalize(v: Vec3) -> Vec3 {
    if v.length() > 1e-4 {
        v.normalize()
    } else {
        Vec3::ZERO
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

        let hex = entity
            .hex
            .unwrap_or_else(|| simulate_everything_engine::v2::hex::Axial::new(0, 0));

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
                    .map(|w| (w.zone, map_severity(w.severity)))
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
        let current_task = derive_current_task(entity);

        // Swordplay visual state — derive from combatant attack/cooldown.
        let (attack_phase, attack_motion, weapon_angle, attack_progress) =
            if let Some(c) = entity.combatant.as_ref() {
                if let Some(atk) = &c.attack {
                    let weapon_props = state
                        .entities
                        .get(atk.weapon)
                        .and_then(|e| e.weapon_props.as_ref());
                    let windup = weapon_props.map(|w| w.windup_ticks).unwrap_or(4) as f32;
                    let progress = (atk.progress / windup).clamp(0.0, 1.0);
                    let phase = if atk.committed { "committed" } else { "windup" };
                    // Weapon angle: offset from facing based on motion.
                    let base_facing = c.facing;
                    let motion_offset = match atk.motion {
                        simulate_everything_engine::v3::martial::AttackMotion::Overhead => {
                            -std::f32::consts::FRAC_PI_2
                        }
                        simulate_everything_engine::v3::martial::AttackMotion::Forehand => {
                            -std::f32::consts::FRAC_PI_4
                        }
                        simulate_everything_engine::v3::martial::AttackMotion::Backhand => {
                            std::f32::consts::FRAC_PI_4
                        }
                        simulate_everything_engine::v3::martial::AttackMotion::Thrust => 0.0,
                        simulate_everything_engine::v3::martial::AttackMotion::Generic => 0.0,
                    };
                    // Animate: in windup, weapon goes to ready position; in committed, swings through.
                    let anim_t = if atk.committed {
                        1.0 - progress
                    } else {
                        progress
                    };
                    let w_angle = base_facing + motion_offset * anim_t;
                    (
                        Some(phase.to_string()),
                        Some(format!("{:?}", atk.motion).to_lowercase()),
                        Some(w_angle),
                        Some(progress),
                    )
                } else if c.cooldown.is_some() {
                    (Some("recovery".to_string()), None, None, None)
                } else {
                    (Some("idle".to_string()), None, None, None)
                }
            } else {
                (None, None, None, None)
            };

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
            current_task,
            attack_phase,
            attack_motion,
            weapon_angle,
            attack_progress,
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
            let (cx, cy, count) =
                stack
                    .members
                    .iter()
                    .fold((0.0f32, 0.0f32, 0u32), |(sx, sy, n), &key| {
                        if let Some(entity) = state.entities.get(key)
                            && let Some(pos) = entity.pos
                        {
                            return (sx + pos.x, sy + pos.y, n + 1);
                        }
                        (sx, sy, n)
                    });
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
                members: stack
                    .members
                    .iter()
                    .filter_map(|&k| state.entities.get(k).map(|e| e.id))
                    .collect(),
                formation: stack.formation,
                center_x: cx,
                center_y: cy,
                facing,
            }
        })
        .collect()
}

fn build_player_list(state: &GameState) -> Vec<PlayerInfo> {
    derive_player_stats(state)
        .into_iter()
        .map(|player| PlayerInfo {
            id: player.id,
            population: player.population,
            territory: player.territory,
            food_level: stockpile_level(player.food_stockpile),
            material_level: stockpile_level(player.material_stockpile),
            alive: player.alive || player.population > 0,
            score: player.population + player.territory,
        })
        .collect()
}

fn derive_current_task(entity: &simulate_everything_engine::v3::state::Entity) -> Option<String> {
    if entity
        .combatant
        .as_ref()
        .and_then(|combatant| combatant.attack.as_ref())
        .is_some()
    {
        return Some("Attack".to_string());
    }
    if entity
        .combatant
        .as_ref()
        .and_then(|combatant| combatant.cooldown.as_ref())
        .is_some()
    {
        return Some("Recover".to_string());
    }
    if let Some(task) = entity
        .person
        .as_ref()
        .and_then(|person| person.task.as_ref())
    {
        return Some(
            match task {
                TaskAssignment::Farm { .. } => "Farm",
                TaskAssignment::Workshop { .. } => "Work",
                TaskAssignment::Patrol => "Patrol",
                TaskAssignment::Garrison => "Garrison",
                TaskAssignment::Train => "Train",
                TaskAssignment::Idle => "Idle",
            }
            .to_string(),
        );
    }

    if entity
        .mobile
        .as_ref()
        .map(|mobile| !mobile.waypoints.is_empty())
        .unwrap_or(false)
    {
        return Some(
            match entity.person.as_ref().map(|person| person.role) {
                Some(Role::Soldier) => "Move",
                Some(Role::Farmer) => "Travel",
                Some(Role::Worker | Role::Builder) => "Work",
                Some(Role::Idle) => "Move",
                None => "Move",
            }
            .to_string(),
        );
    }

    entity.person.as_ref().map(|person| {
        match person.role {
            Role::Farmer => "Farm",
            Role::Worker | Role::Builder => "Work",
            Role::Soldier => "Hold",
            Role::Idle => "Idle",
        }
        .to_string()
    })
}

// ---------------------------------------------------------------------------
// Delta tracker — compares consecutive snapshots to produce deltas
// ---------------------------------------------------------------------------

/// Tracks previous tick's state to compute deltas for spectator streaming.
pub struct DeltaTracker {
    prev_entities: HashMap<u32, SpectatorEntityInfo>,
    prev_body_models: HashMap<u32, BodyRenderInfo>,
    prev_projectiles: HashMap<u32, ProjectileInfo>,
    prev_stacks: HashMap<u32, StackInfo>,
}

impl Default for DeltaTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl DeltaTracker {
    pub fn new() -> Self {
        Self {
            prev_entities: HashMap::new(),
            prev_body_models: HashMap::new(),
            prev_projectiles: HashMap::new(),
            prev_stacks: HashMap::new(),
        }
    }

    /// Reset tracker state (e.g., on new game).
    pub fn reset(&mut self) {
        self.prev_entities.clear();
        self.prev_body_models.clear();
        self.prev_projectiles.clear();
        self.prev_stacks.clear();
    }

    /// Build a delta from the current game state compared to the previous tick.
    /// Updates internal state for the next comparison.
    pub fn build_delta(&mut self, state: &GameState, dt: f32) -> V3SnapshotDelta {
        let cur_entities = build_entity_list(state);
        let cur_body_models = build_body_render_list(state);
        let cur_projectiles = build_projectile_list(state);
        let cur_stacks = build_stack_list(state);
        let players = build_player_list(state);

        // --- Entities ---
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
            if let Some(prev) = self.prev_entities.get(&e.id)
                && let Some(update) = diff_entity(prev, e)
            {
                entities_updated.push(update);
            }
        }

        // --- Body models ---
        let cur_body_ids: std::collections::HashSet<u32> =
            cur_body_models.iter().map(|b| b.entity_id).collect();
        let prev_body_ids: std::collections::HashSet<u32> =
            self.prev_body_models.keys().copied().collect();

        let body_models_appeared: Vec<BodyRenderInfo> = cur_body_models
            .iter()
            .filter(|b| !prev_body_ids.contains(&b.entity_id))
            .cloned()
            .collect();
        let body_models_removed: Vec<u32> =
            prev_body_ids.difference(&cur_body_ids).copied().collect();

        let mut body_models_updated = Vec::new();
        for body in &cur_body_models {
            if let Some(prev) = self.prev_body_models.get(&body.entity_id)
                && prev != body
            {
                body_models_updated.push(body.clone());
            }
        }

        // --- Projectiles ---
        let cur_proj_ids: std::collections::HashSet<u32> =
            cur_projectiles.iter().map(|p| p.id).collect();
        let prev_proj_ids: std::collections::HashSet<u32> =
            self.prev_projectiles.keys().copied().collect();

        let projectiles_spawned: Vec<ProjectileInfo> = cur_projectiles
            .iter()
            .filter(|p| !prev_proj_ids.contains(&p.id))
            .cloned()
            .collect();
        let projectiles_removed: Vec<u32> =
            prev_proj_ids.difference(&cur_proj_ids).copied().collect();

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
        let stacks_dissolved: Vec<u32> =
            prev_stack_ids.difference(&cur_stack_ids).copied().collect();

        let mut stacks_updated: Vec<StackUpdate> = Vec::new();
        for s in &cur_stacks {
            if let Some(prev) = self.prev_stacks.get(&s.id)
                && let Some(update) = diff_stack(prev, s)
            {
                stacks_updated.push(update);
            }
        }

        // Update previous state for next tick.
        self.prev_entities = cur_entities.into_iter().map(|e| (e.id, e)).collect();
        self.prev_body_models = cur_body_models
            .into_iter()
            .map(|b| (b.entity_id, b))
            .collect();
        self.prev_projectiles = cur_projectiles.into_iter().map(|p| (p.id, p)).collect();
        self.prev_stacks = cur_stacks.into_iter().map(|s| (s.id, s)).collect();

        V3SnapshotDelta {
            tick: state.tick,
            dt,
            full_state: false,
            entities_appeared,
            entities_updated,
            entities_removed,
            body_models_appeared,
            body_models_updated,
            body_models_removed,
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
        self.prev_body_models = snapshot
            .body_models
            .iter()
            .map(|b| (b.entity_id, b.clone()))
            .collect();
        self.prev_projectiles = snapshot
            .projectiles
            .iter()
            .map(|p| (p.id, p.clone()))
            .collect();
        self.prev_stacks = snapshot.stacks.iter().map(|s| (s.id, s.clone())).collect();
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
        attack_phase: None,
        attack_motion: None,
        weapon_angle: None,
        attack_progress: None,
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

    if let (Some(cb), Some(pb)) = (cur.blood, prev.blood)
        && (cb - pb).abs() > VITAL_EPSILON
    {
        update.blood = Some(cb);
        changed = true;
    }

    if let (Some(cs), Some(ps)) = (cur.stamina, prev.stamina)
        && (cs - ps).abs() > VITAL_EPSILON
    {
        update.stamina = Some(cs);
        changed = true;
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

    if cur.attack_phase != prev.attack_phase {
        update.attack_phase = Some(cur.attack_phase.clone());
        changed = true;
    }

    if cur.attack_motion != prev.attack_motion {
        update.attack_motion = Some(cur.attack_motion.clone());
        changed = true;
    }

    if cur.weapon_angle != prev.weapon_angle {
        update.weapon_angle = Some(cur.weapon_angle);
        changed = true;
    }

    if cur.attack_progress != prev.attack_progress {
        update.attack_progress = Some(cur.attack_progress);
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

#[cfg(test)]
mod tests {
    use super::*;
    use simulate_everything_engine::v3::{
        formation::FormationType, mapgen, movement::Mobile, spatial::Vec3, state::Stack,
    };

    #[test]
    fn delta_tracker_reports_stack_creation_update_and_dissolve() {
        let mut state = mapgen::generate(15, 15, 2, 42);
        let dt = 1.0;

        let initial = build_snapshot(&state, dt);
        let mut tracker = DeltaTracker::new();
        tracker.seed_from_snapshot(&initial);

        let member = state
            .entities
            .iter()
            .find_map(|(key, entity)| (entity.owner == Some(0)).then_some(key))
            .expect("player 0 should own at least one entity");
        let mut members = state
            .stacks
            .first()
            .map(|stack| stack.members.clone())
            .unwrap_or_default();
        if members.is_empty() {
            members.push(member);
        }
        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members,
            formation: FormationType::Line,
            leader: member,
        });

        let created = tracker.build_delta(&state, dt);
        assert_eq!(created.stacks_created.len(), 1);
        assert_eq!(created.stacks_created[0].id, stack_id.0);
        assert!(created.stacks_updated.is_empty());
        assert!(created.stacks_dissolved.is_empty());

        state
            .stacks
            .iter_mut()
            .find(|stack| stack.id == stack_id)
            .expect("new stack should exist")
            .formation = FormationType::Wedge;
        let updated = tracker.build_delta(&state, dt);
        assert_eq!(updated.stacks_created.len(), 0);
        assert_eq!(updated.stacks_updated.len(), 1);
        assert_eq!(updated.stacks_updated[0].id, stack_id.0);
        assert_eq!(
            updated.stacks_updated[0].formation,
            Some(FormationType::Wedge)
        );

        state.stacks.retain(|stack| stack.id != stack_id);
        let dissolved = tracker.build_delta(&state, dt);
        assert!(dissolved.stacks_created.is_empty());
        assert!(dissolved.stacks_updated.is_empty());
        assert_eq!(dissolved.stacks_dissolved, vec![stack_id.0]);
    }

    #[test]
    fn snapshot_derives_player_hex_and_task_state() {
        let mut state = mapgen::generate(15, 15, 2, 42);
        let mover = state
            .entities
            .iter()
            .find_map(|(key, entity)| {
                (entity.owner == Some(0)
                    && entity.person.is_some()
                    && entity.mobile.is_some()
                    && entity.pos.is_some())
                .then_some(key)
            })
            .expect("player 0 should have a mobile entity");
        state.entities[mover].mobile = Some(Mobile::new(2.0, 10.0));
        state.entities[mover]
            .mobile
            .as_mut()
            .unwrap()
            .waypoints
            .push(Vec3::new(100.0, 100.0, 0.0));

        let snapshot = build_snapshot(&state, 1.0);
        assert!(
            snapshot.hex_ownership.iter().any(|owner| owner.is_some()),
            "snapshot should derive non-empty hex ownership"
        );
        assert!(
            snapshot
                .hex_structures
                .iter()
                .any(|structure| structure.is_some()),
            "snapshot should derive visible structure ids"
        );
        assert!(
            snapshot.players.iter().any(|player| player.territory > 0),
            "player aggregates should include derived territory"
        );
        assert!(
            snapshot.players.iter().any(|player| player.food_level > 0),
            "player aggregates should include derived stockpile levels"
        );
        let entity_id = state.entities[mover].id;
        let task = snapshot
            .entities
            .iter()
            .find(|entity| entity.id == entity_id)
            .and_then(|entity| entity.current_task.as_deref());
        assert_eq!(task, Some("Move"));
    }
}
