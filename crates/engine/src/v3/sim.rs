use smallvec::SmallVec;

use super::action_queue::{Action, CurrentAction};
use super::agent::{AgentOutput, LayeredAgent};
use super::body_physics;
use super::combat_log::CombatObservation;
use super::commands::{CommandApplySummary, apply_agent_output};
use super::damage::{self, BlockCapability, DefenderState, Impact, ImpactResult};
use super::economy;
use super::htn;
use super::index::update_hex_membership;
use super::lifecycle::{self, cleanup_dead, cleanup_inert_projectiles};
use super::martial;
use super::movement::{self, Mobile};
use super::needs::{NeedDecayRates, apply_decay};
use super::projectile::{self, ProjectileTick};
use super::resolution::{enemy_nearby, resolution_demand_at};
use super::spatial::{Vec2, Vec3, terrain_height_at, terrain_material_at, terrain_slope_at};
use super::state::{DecisionRecord, GameState, Role, StructureType};
use super::steering;
use super::utility::{Goal, UtilityScorer};
use super::weapon::{self, AttackTick};
use super::{economy::add_player_stockpile, economy::consume_player_stockpile};
use crate::v2::state::EntityKey;

// ---------------------------------------------------------------------------
// Tick result
// ---------------------------------------------------------------------------

/// Information returned from a tick for the caller (protocol, renderer).
#[derive(Debug, Default)]
pub struct TickResult {
    /// Players eliminated this tick.
    pub eliminated: SmallVec<[u8; 4]>,
    /// Number of entities that died this tick.
    pub deaths: usize,
    /// Number of projectile impacts this tick.
    pub impacts: usize,
}

#[derive(Debug, Default)]
pub struct AgentPhaseResult {
    pub outputs: Vec<AgentOutput>,
    pub summaries: Vec<CommandApplySummary>,
}

// ---------------------------------------------------------------------------
// Main tick function
// ---------------------------------------------------------------------------

/// Run the agent phase against the current state and apply resulting commands.
///
/// This is the shared engine-owned path used by the simulation runtime and the
/// bench harness so both exercise the same command-application code.
pub fn run_agent_phase(state: &mut GameState, agents: &mut [LayeredAgent]) -> AgentPhaseResult {
    rebuild_spatial_index(state);

    let outputs: Vec<AgentOutput> = agents.iter_mut().map(|agent| agent.tick(state)).collect();
    let summaries = apply_agent_outputs(state, &outputs);

    AgentPhaseResult { outputs, summaries }
}

/// Apply already-produced agent outputs to the world state in order.
pub fn apply_agent_outputs(
    state: &mut GameState,
    outputs: &[AgentOutput],
) -> Vec<CommandApplySummary> {
    outputs
        .iter()
        .map(|output| apply_agent_output(state, output))
        .collect()
}

/// Advance the simulation by one tick after running the shared agent phase.
pub fn tick_with_agents(state: &mut GameState, agents: &mut [LayeredAgent], dt: f64) -> TickResult {
    let _ = run_agent_phase(state, agents);
    tick(state, dt)
}

/// Advance the simulation by one tick.
///
/// Orchestrates all subsystems in the correct order:
/// 1. Spatial index rebuild
/// 2. Economy
/// 3. Steering → movement
/// 4. Melee combat
/// 5. Projectile advancement
/// 6. Impact resolution
/// 7. Vitals (bleed, stamina, stagger)
/// 8. Cleanup (dead entities, spent projectiles)
/// 9. Elimination check
pub fn tick(state: &mut GameState, dt: f64) -> TickResult {
    let mut result = TickResult::default();
    let dt_f32 = dt as f32;

    // --- Phase 1: Spatial index ---
    rebuild_spatial_index(state);

    // --- Phase 2: Economy ---
    economy::tick_economy(state, dt_f32);

    // --- Phase 3: Autonomous behavior ---
    tick_behaviors(state, dt_f32);

    // --- Phase 4: Movement ---
    compute_steering_and_move(state, dt_f32);

    // --- Phase 4.5: Body physics ---
    body_physics::tick_body_physics(state, dt_f32);

    // --- Phase 5: Melee combat ---
    let melee_impacts = resolve_melee_attacks(state);

    // --- Phase 6: Projectile advancement ---
    let projectile_impacts = advance_projectiles(state);
    result.impacts = melee_impacts.len() + projectile_impacts.len();

    // --- Phase 7: Impact resolution ---
    apply_all_impacts(state, &melee_impacts);
    apply_all_impacts(state, &projectile_impacts);

    // --- Phase 8: Vitals ---
    tick_vitals(state, dt_f32);

    // --- Phase 9: Cleanup ---
    // Count newly dead before cleanup strips their mobile/combatant
    result.deaths = state
        .entities
        .values()
        .filter(|e| {
            e.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false)
                && (e.mobile.is_some() || e.combatant.is_some())
        })
        .count();
    cleanup_dead(state);
    cleanup_inert_projectiles(state);

    // --- Phase 10: Elimination ---
    result.eliminated = lifecycle::check_elimination(state);

    // --- Advance time ---
    state.game_time += dt;
    state.tick += 1;

    result
}

pub fn batch_resolve_entities(
    state: &mut GameState,
    entity_keys: &[EntityKey],
    dt: f32,
    time_budget: f32,
) {
    let mut elapsed = 0.0f32;
    while elapsed < time_budget {
        let before = state.tick;
        for &entity_key in entity_keys {
            tick_entity_behavior(state, entity_key, dt, true);
        }
        if state.tick == before {
            elapsed += dt.max(1.0);
        } else {
            elapsed += dt.max(1.0);
        }
    }
}

fn tick_behaviors(state: &mut GameState, dt: f32) {
    let entity_keys: Vec<EntityKey> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| entity.person.as_ref().map(|_| key))
        .collect();
    for entity_key in entity_keys {
        tick_entity_behavior(state, entity_key, dt, false);
    }
}

fn tick_entity_behavior(state: &mut GameState, entity_key: EntityKey, dt: f32, batch_mode: bool) {
    let Some(entity_snapshot) = state.entities.get(entity_key).cloned() else {
        return;
    };
    let Some(person) = entity_snapshot.person.as_ref() else {
        return;
    };
    if entity_snapshot
        .vitals
        .as_ref()
        .map(|vitals| vitals.is_dead())
        .unwrap_or(false)
    {
        return;
    }

    let owner = entity_snapshot.owner.unwrap_or(0);
    let pos = entity_snapshot.pos.unwrap_or(Vec3::ZERO);
    let enemy_close = enemy_nearby(state, entity_key, 90.0);
    let resolution = resolution_demand_at(
        state,
        entity_snapshot
            .hex
            .unwrap_or_else(|| super::hex::world_to_hex(pos)),
    );
    let weights = state
        .faction_need_weights
        .get(owner as usize)
        .copied()
        .unwrap_or_default();
    let waypoints_empty = state
        .entities
        .get(entity_key)
        .and_then(|entity| entity.mobile.as_ref())
        .map(|mobile| mobile.waypoints.is_empty())
        .unwrap_or(true);

    let mut should_replan = false;
    if let Some(behavior) = state
        .entities
        .get_mut(entity_key)
        .and_then(|entity| entity.behavior.as_mut())
    {
        let ticks_elapsed = state
            .tick
            .saturating_sub(behavior.last_decision_tick)
            .max(1);
        apply_decay(
            &mut behavior.needs,
            NeedDecayRates::default(),
            ticks_elapsed,
            dt,
        );
        if enemy_close {
            behavior.needs.safety = behavior.needs.safety.max(0.7);
        }
        if waypoints_empty {
            behavior.needs.duty = (behavior.needs.duty + 0.02).clamp(0.0, 1.0);
        }
        should_replan =
            state.tick >= behavior.next_decision_tick || behavior.action_queue.is_empty();
    }

    if should_replan {
        let needs = state
            .entities
            .get(entity_key)
            .and_then(|entity| entity.behavior.as_ref())
            .map(|behavior| behavior.needs)
            .unwrap_or_default();
        let choice =
            UtilityScorer::choose_goal(state, entity_key, needs, weights, resolution, enemy_close);
        let plan = htn::decompose_goal(state, entity_key, choice.goal);
        if let Some(behavior) = state
            .entities
            .get_mut(entity_key)
            .and_then(|entity| entity.behavior.as_mut())
        {
            behavior.current_goal = Some(choice.goal);
            behavior.decision_reason = Some(choice.reason.clone());
            behavior.action_queue.clear();
            behavior.action_queue.queued.extend(plan.actions);
            behavior.mtr = plan.traversal;
            if behavior.decision_history.len() == 4 {
                behavior.decision_history.remove(0);
            }
            behavior.decision_history.push(DecisionRecord {
                tick: state.tick,
                goal: choice.goal,
                reason: choice.reason,
            });
            behavior.last_decision_tick = state.tick;
            behavior.next_decision_tick =
                state.tick + decision_interval(choice.goal, enemy_close, resolution, person.role);
        }
    }

    promote_next_action(state, entity_key);
    advance_current_action(state, entity_key, dt, batch_mode);
}

fn decision_interval(goal: Goal, enemy_close: bool, resolution: f32, role: Role) -> u64 {
    if enemy_close || matches!(goal, Goal::Fight | Goal::Flee) || resolution > 0.45 {
        return 1;
    }
    if matches!(goal, Goal::Work | Goal::Build | Goal::Eat) {
        return 20;
    }
    if role == Role::Soldier {
        return 30;
    }
    60
}

fn promote_next_action(state: &mut GameState, entity_key: EntityKey) {
    if let Some(queue) = state
        .entities
        .get_mut(entity_key)
        .and_then(|entity| entity.behavior.as_mut())
        .map(|behavior| &mut behavior.action_queue)
        && queue.current.is_none()
        && let Some(action) = queue.queued.pop_front()
    {
        queue.current = Some(CurrentAction {
            action,
            progress: 0.0,
        });
    }
}

fn advance_current_action(state: &mut GameState, entity_key: EntityKey, dt: f32, batch_mode: bool) {
    let current = state
        .entities
        .get(entity_key)
        .and_then(|entity| entity.behavior.as_ref())
        .and_then(|behavior| behavior.action_queue.current.as_ref())
        .cloned();
    let Some(current) = current else {
        return;
    };

    let complete = match current.action.clone() {
        Action::MoveTo { target } => advance_move_action(state, entity_key, target, batch_mode),
        Action::WorkAt { target, duration } => {
            advance_timed_action(state, entity_key, dt, duration, |state, entity_key| {
                apply_work_effect(state, entity_key, target);
            })
        }
        Action::ConsumeStockpile => {
            apply_consume_effect(state, entity_key);
            true
        }
        Action::AttackTarget { target } => advance_attack_action(state, entity_key, target),
        Action::FleeFrom { threat, distance } => {
            advance_flee_action(state, entity_key, threat, distance, batch_mode)
        }
        Action::Rest { duration } => {
            advance_timed_action(state, entity_key, dt, duration, |state, entity_key| {
                if let Some(behavior) = state
                    .entities
                    .get_mut(entity_key)
                    .and_then(|entity| entity.behavior.as_mut())
                {
                    behavior.needs.rest = 0.05;
                }
                if let Some(vitals) = state
                    .entities
                    .get_mut(entity_key)
                    .and_then(|entity| entity.vitals.as_mut())
                {
                    vitals.stamina = (vitals.stamina + 0.2).clamp(0.0, 1.0);
                }
            })
        }
        Action::SocializeAt { target, duration } => {
            let counterpart_id = state.entities.get(target).map(|entity| entity.id);
            advance_timed_action(state, entity_key, dt, duration, |state, entity_key| {
                if let Some(behavior) = state
                    .entities
                    .get_mut(entity_key)
                    .and_then(|entity| entity.behavior.as_mut())
                {
                    behavior.needs.social = 0.05;
                    if let Some(target_id) = counterpart_id {
                        behavior
                            .social
                            .remember(state.tick, target_id, "shared settlement time");
                    }
                }
            })
        }
        Action::Wait { duration } => {
            advance_timed_action(state, entity_key, dt, duration, |_state, _entity_key| {})
        }
    };

    if complete
        && let Some(queue) = state
            .entities
            .get_mut(entity_key)
            .and_then(|entity| entity.behavior.as_mut())
            .map(|behavior| &mut behavior.action_queue)
    {
        queue.current = None;
    }
}

fn advance_move_action(
    state: &mut GameState,
    entity_key: EntityKey,
    target: Vec3,
    batch_mode: bool,
) -> bool {
    if batch_mode {
        if let Some(entity) = state.entities.get_mut(entity_key) {
            entity.pos = Some(target);
            entity.hex = Some(super::hex::world_to_hex(target));
            if let Some(mobile) = entity.mobile.as_mut() {
                mobile.waypoints.clear();
            }
        }
        return true;
    }
    let Some(entity) = state.entities.get_mut(entity_key) else {
        return true;
    };
    let Some(pos) = entity.pos else {
        return true;
    };
    if let Some(mobile) = entity.mobile.as_mut() {
        mobile.waypoints = vec![target];
    }
    (target.x - pos.x).powi(2) + (target.y - pos.y).powi(2) <= 16.0
}

fn advance_timed_action(
    state: &mut GameState,
    entity_key: EntityKey,
    dt: f32,
    duration: f32,
    on_complete: impl FnOnce(&mut GameState, EntityKey),
) -> bool {
    let mut done = false;
    if let Some(current) = state
        .entities
        .get_mut(entity_key)
        .and_then(|entity| entity.behavior.as_mut())
        .and_then(|behavior| behavior.action_queue.current.as_mut())
    {
        current.progress += dt.max(0.05);
        done = current.progress >= duration;
    }
    if done {
        on_complete(state, entity_key);
    }
    done
}

fn apply_work_effect(state: &mut GameState, entity_key: EntityKey, target: EntityKey) {
    let owner = state
        .entities
        .get(entity_key)
        .and_then(|entity| entity.owner);
    let Some(owner) = owner else {
        return;
    };
    if let Some(entity) = state.entities.get(target) {
        if let Some(physical) = &entity.physical {
            use simulate_everything_protocol::{CommodityKind, PropertyTag};
            if physical.has_tag(PropertyTag::Harvestable) {
                add_player_stockpile(state, owner, CommodityKind::Food, 1.5);
            } else if physical.has_tag(PropertyTag::Tool) {
                add_player_stockpile(state, owner, CommodityKind::Material, 0.75);
            }
        }
    }
    if let Some(behavior) = state
        .entities
        .get_mut(entity_key)
        .and_then(|entity| entity.behavior.as_mut())
    {
        behavior.needs.duty = 0.05;
        behavior.needs.hunger = (behavior.needs.hunger + 0.06).clamp(0.0, 1.0);
    }
}

fn apply_consume_effect(state: &mut GameState, entity_key: EntityKey) {
    let Some(owner) = state
        .entities
        .get(entity_key)
        .and_then(|entity| entity.owner)
    else {
        return;
    };
    use simulate_everything_protocol::CommodityKind;
    if economy::player_stockpile_amount(state, owner, CommodityKind::Food) > 0.0 {
        consume_player_stockpile(state, owner, CommodityKind::Food, 1.0);
        if let Some(behavior) = state
            .entities
            .get_mut(entity_key)
            .and_then(|entity| entity.behavior.as_mut())
        {
            behavior.needs.hunger = 0.05;
        }
    }
}

fn advance_attack_action(state: &mut GameState, entity_key: EntityKey, target: EntityKey) -> bool {
    let target_pos = state.entities.get(target).and_then(|entity| entity.pos);
    let Some(target_pos) = target_pos else {
        return true;
    };
    if let Some(entity) = state.entities.get_mut(entity_key) {
        if let Some(mobile) = entity.mobile.as_mut()
            && let Some(pos) = entity.pos
            && (target_pos.x - pos.x).powi(2) + (target_pos.y - pos.y).powi(2) > 9.0
        {
            mobile.waypoints = vec![target_pos];
            return false;
        }
        if let Some(combatant) = entity.combatant.as_mut() {
            combatant.target = Some(target);
        }
    }
    super::commands::apply_tactical_command(
        state,
        &super::agent::TacticalCommand::Attack {
            attacker: entity_key,
            target,
        },
    );
    true
}

fn advance_flee_action(
    state: &mut GameState,
    entity_key: EntityKey,
    threat: EntityKey,
    distance: f32,
    batch_mode: bool,
) -> bool {
    let Some(entity_pos) = state.entities.get(entity_key).and_then(|entity| entity.pos) else {
        return true;
    };
    let Some(threat_pos) = state.entities.get(threat).and_then(|entity| entity.pos) else {
        return true;
    };
    let dir = (entity_pos - threat_pos).normalize();
    let target = Vec3::new(
        entity_pos.x + dir.x * distance,
        entity_pos.y + dir.y * distance,
        entity_pos.z,
    );
    let done = advance_move_action(state, entity_key, target, batch_mode);
    if done
        && let Some(behavior) = state
            .entities
            .get_mut(entity_key)
            .and_then(|entity| entity.behavior.as_mut())
    {
        behavior.needs.safety = 0.2;
    }
    done
}

// ---------------------------------------------------------------------------
// Phase 1: Spatial index rebuild
// ---------------------------------------------------------------------------

/// Recompute hex membership for all entities with positions.
/// Uses hysteresis to prevent oscillation at hex boundaries.
fn rebuild_spatial_index(state: &mut GameState) {
    // Collect updates to avoid borrow conflicts
    let updates: SmallVec<[(EntityKey, _); 64]> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            let pos = entity.pos?;
            let current_hex = entity.hex?;
            let new_hex = update_hex_membership(current_hex, pos.xy());
            if new_hex != current_hex {
                Some((key, (current_hex, new_hex)))
            } else {
                None
            }
        })
        .collect();

    for (key, (old_hex, new_hex)) in updates {
        state.spatial_index.move_entity(old_hex, new_hex, key);
        if let Some(entity) = state.entities.get_mut(key) {
            entity.hex = Some(new_hex);
        }
    }

    // Rebuild fine index (full rebuild each tick — cheap hash table ops)
    state.fine_index.rebuild(
        state
            .entities
            .iter()
            .filter_map(|(key, e)| e.pos.map(|p| (key, p))),
    );

    // Rebuild coarse index (full rebuild each tick — few cells, cheap)
    state
        .coarse_index
        .rebuild(state.entities.iter().filter_map(|(key, e)| {
            let pos = e.pos?;
            let is_soldier = e
                .person
                .as_ref()
                .map(|p| p.role == super::state::Role::Soldier)
                .unwrap_or(false);
            Some((key, pos, e.owner, is_soldier))
        }));
}

// ---------------------------------------------------------------------------
// Phase 2: Steering + movement
// ---------------------------------------------------------------------------

/// Compute steering forces and integrate movement for all mobile entities.
fn compute_steering_and_move(state: &mut GameState, dt: f32) {
    // Collect mobile entity positions for separation calculation
    let mobile_positions: SmallVec<[(EntityKey, Vec3); 64]> = state
        .entities
        .iter()
        .filter_map(|(key, e)| {
            if e.mobile.is_some() {
                e.pos.map(|p| (key, p))
            } else {
                None
            }
        })
        .collect();

    // Compute derived speed and steering for each mobile entity, then integrate
    let updates: SmallVec<[(EntityKey, Vec3, Mobile); 64]> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            let pos = entity.pos?;
            let mobile = entity.mobile.as_ref()?;

            // Skip staggered entities
            if entity
                .vitals
                .as_ref()
                .map(|v| v.is_staggered() || v.is_collapsed())
                .unwrap_or(false)
            {
                return None;
            }

            // Compute derived speed
            let stamina = entity.vitals.as_ref().map(|v| v.stamina).unwrap_or(1.0);
            let leg_wound_weight = entity
                .wounds
                .as_ref()
                .map(|w| super::wound::zone_wound_weight(w, super::armor::BodyZone::Legs))
                .unwrap_or(0.0);

            let speed_factors = movement::SpeedFactors {
                base_capability: 3.0, // default person speed
                slope_factor: movement::slope_factor(terrain_slope_at(
                    state,
                    pos.xy(),
                    mobile.vel.xy(),
                )),
                surface_factor: movement::surface_factor(
                    terrain_material_at(state, pos.xy()).friction(),
                ),
                encumbrance_factor: 1.0, // TODO: compute from carried weight
                wound_factor: movement::wound_factor(leg_wound_weight),
                stamina_factor: movement::stamina_factor(stamina),
            };
            let derived_speed = speed_factors.derived_speed();

            // Melee approach: if entity has an active attack, steer toward target
            let attack_target_key = entity
                .combatant
                .as_ref()
                .and_then(|c| c.attack.as_ref())
                .map(|a| a.target);

            let has_waypoints = !mobile.waypoints.is_empty();
            let has_intent = has_waypoints || attack_target_key.is_some();

            let mut accel = Vec3::ZERO;

            if let Some(target_key) = attack_target_key {
                // Steer toward attack target for melee approach
                if let Some(target_pos) = state.entities.get(target_key).and_then(|e| e.pos) {
                    let target_radius = state
                        .entities
                        .get(target_key)
                        .and_then(|e| e.mobile.as_ref())
                        .map(|m| m.radius)
                        .unwrap_or(mobile.radius);
                    let strike_distance = mobile.radius + target_radius + 2.0;
                    let center_distance = (target_pos - pos).length();

                    if center_distance > strike_distance {
                        accel = steering::arrive(
                            pos,
                            mobile.vel,
                            target_pos,
                            mobile.steering_force,
                            derived_speed,
                            strike_distance,
                        );
                    }
                }
            } else if let Some(&wp) = mobile.waypoints.first() {
                // Normal waypoint steering
                accel = steering::arrive(
                    pos,
                    mobile.vel,
                    wp,
                    mobile.steering_force,
                    derived_speed,
                    50.0,
                );
            }

            // Separation only for entities with movement intent (prevent idle drift)
            if has_intent {
                let neighbors: SmallVec<[Vec3; 16]> = mobile_positions
                    .iter()
                    .filter(|(k, _)| *k != key)
                    .filter(|(_, p)| (*p - pos).length_squared() < 30.0 * 30.0)
                    .map(|(_, p)| *p)
                    .collect();

                if !neighbors.is_empty() {
                    let sep = steering::separation(pos, &neighbors, mobile.radius * 2.0);
                    accel = accel + sep;
                }
            }

            // Integrate
            let mut mobile_clone = mobile.clone();
            let new_pos = movement::integrate(pos, &mut mobile_clone, accel, derived_speed, dt);

            // Consume waypoints
            movement::consume_waypoint(new_pos, &mut mobile_clone, 2.0);

            Some((key, new_pos, mobile_clone))
        })
        .collect();

    // Apply updates
    for (key, new_pos, new_mobile) in updates {
        if let Some(entity) = state.entities.get_mut(key) {
            entity.pos = Some(new_pos);
            entity.mobile = Some(new_mobile);
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 3: Melee combat
// ---------------------------------------------------------------------------

/// Pending impact from melee or projectile, to be resolved in phase 5.
struct PendingImpact {
    target: EntityKey,
    impact: Impact,
}

/// Tick attack states and resolve ready melee attacks.
fn resolve_melee_attacks(state: &mut GameState) -> Vec<PendingImpact> {
    let mut impacts = Vec::new();

    // Collect attackers that have active attacks
    let attackers: SmallVec<[(EntityKey, _); 32]> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            let combatant = entity.combatant.as_ref()?;
            let attack = combatant.attack.as_ref()?;
            let weapon_key = attack.weapon;
            Some((key, (attack.target, weapon_key)))
        })
        .collect();

    for (attacker_key, (target_key, weapon_key)) in attackers {
        // Get weapon properties
        let weapon_props = match state
            .entities
            .get(weapon_key)
            .and_then(|e| e.weapon_props.as_ref())
        {
            Some(w) => w.clone(),
            None => continue,
        };

        // Tick the attack
        let tick_result = {
            let entity = match state.entities.get_mut(attacker_key) {
                Some(e) => e,
                None => continue,
            };
            let attack = match entity.combatant.as_mut().and_then(|c| c.attack.as_mut()) {
                Some(a) => a,
                None => continue,
            };
            weapon::tick_attack(attack, &weapon_props)
        };

        match tick_result {
            AttackTick::Ready => {
                // Resolve the attack
                let attacker_pos = state
                    .entities
                    .get(attacker_key)
                    .and_then(|e| e.pos)
                    .unwrap_or(Vec3::ZERO);
                let attacker_radius = state
                    .entities
                    .get(attacker_key)
                    .and_then(|e| e.mobile.as_ref())
                    .map(|m| m.radius)
                    .unwrap_or(0.0);
                let target_pos = state
                    .entities
                    .get(target_key)
                    .and_then(|e| e.pos)
                    .unwrap_or(Vec3::ZERO);
                let target_radius = state
                    .entities
                    .get(target_key)
                    .and_then(|e| e.mobile.as_ref())
                    .map(|m| m.radius)
                    .unwrap_or(0.0);

                // Check stagger state
                let stagger = state
                    .entities
                    .get(attacker_key)
                    .and_then(|e| e.combatant.as_ref())
                    .and_then(|c| c.attack.as_ref())
                    .and_then(|a| {
                        if state
                            .entities
                            .get(attacker_key)
                            .and_then(|e| e.vitals.as_ref())
                            .map(|v| v.is_staggered())
                            .unwrap_or(false)
                        {
                            Some(weapon::handle_stagger(a))
                        } else {
                            None
                        }
                    });
                let attack_state = state
                    .entities
                    .get(attacker_key)
                    .and_then(|e| e.combatant.as_ref())
                    .and_then(|c| c.attack.as_ref())
                    .cloned();

                let attacker_body = state
                    .entities
                    .get(attacker_key)
                    .and_then(|e| e.body.as_deref());

                if let Some(impact) = attack_state.as_ref().and_then(|attack| {
                    weapon::resolve_melee(
                        &weapon_props,
                        attacker_key,
                        attacker_pos,
                        attacker_radius,
                        target_pos,
                        target_radius,
                        attack,
                        stagger.as_ref(),
                        attacker_body,
                        state.tick,
                    )
                }) {
                    impacts.push(PendingImpact {
                        target: target_key,
                        impact,
                    });
                }

                // Clear attack, start cooldown
                let cooldown_stamina = state
                    .entities
                    .get(attacker_key)
                    .and_then(|e| e.vitals.as_ref())
                    .map(|v| v.stamina)
                    .unwrap_or(1.0);
                let cd = weapon::compute_cooldown(&weapon_props, cooldown_stamina);

                if let Some(combatant) = state
                    .entities
                    .get_mut(attacker_key)
                    .and_then(|e| e.combatant.as_mut())
                {
                    combatant.attack = None;
                    combatant.cooldown = Some(weapon::CooldownState::new(cd));
                }
            }
            AttackTick::InProgress | AttackTick::Committed => {
                // Attack continues
            }
        }
    }

    // Tick cooldowns
    let cooldown_done: SmallVec<[EntityKey; 16]> = state
        .entities
        .iter_mut()
        .filter_map(|(key, entity)| {
            let cd = entity.combatant.as_mut()?.cooldown.as_mut()?;
            if cd.tick() { Some(key) } else { None }
        })
        .collect();

    for key in cooldown_done {
        if let Some(combatant) = state
            .entities
            .get_mut(key)
            .and_then(|e| e.combatant.as_mut())
        {
            combatant.cooldown = None;
        }
    }

    impacts
}

// ---------------------------------------------------------------------------
// Phase 4: Projectile advancement
// ---------------------------------------------------------------------------

/// Advance all projectile entities by one tick.
fn advance_projectiles(state: &mut GameState) -> Vec<PendingImpact> {
    let mut impacts = Vec::new();

    // Collect projectile entities
    let projectiles: SmallVec<[(EntityKey, Vec3, Vec3, projectile::Projectile, Option<u8>); 16]> =
        state
            .entities
            .iter()
            .filter_map(|(key, entity)| {
                let proj = entity.projectile.as_ref()?;
                let pos = entity.pos?;
                let vel = entity.mobile.as_ref()?.vel;
                Some((key, pos, vel, *proj, entity.owner))
            })
            .collect();

    // Need entity positions for collision checks
    let entity_positions: SmallVec<[(EntityKey, Vec3, f32); 64]> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            if entity.projectile.is_some() {
                return None; // skip projectiles themselves
            }
            let pos = entity.pos?;
            let radius = entity.mobile.as_ref().map(|m| m.radius).unwrap_or(10.0); // default collision radius for non-mobile
            Some((key, pos, radius))
        })
        .collect();

    for (proj_key, pos, vel, proj_component, _owner) in projectiles {
        let attacker_id = proj_key; // projectile entity IS the attacker for attribution

        let result = projectile::tick_projectile(
            pos,
            vel,
            &proj_component,
            attacker_id,
            state.tick,
            |x, y| terrain_height_at(state, Vec2::new(x, y)),
            |check_pos| {
                // Find nearest entity within collision radius
                for &(key, entity_pos, radius) in &entity_positions {
                    let dist = (check_pos - entity_pos).length();
                    if dist < radius * 0.1 {
                        // tight collision
                        return Some(key);
                    }
                }
                None
            },
        );

        match result {
            ProjectileTick::InFlight {
                pos: new_pos,
                vel: new_vel,
            } => {
                if let Some(entity) = state.entities.get_mut(proj_key) {
                    entity.pos = Some(new_pos);
                    if let Some(mobile) = &mut entity.mobile {
                        mobile.vel = new_vel;
                    }
                }
            }
            ProjectileTick::EntityHit {
                pos: hit_pos,
                target,
                impact,
            } => {
                impacts.push(PendingImpact { target, impact });
                // Remove projectile component (mark as inert)
                if let Some(entity) = state.entities.get_mut(proj_key) {
                    entity.pos = Some(hit_pos);
                    entity.projectile = None;
                    entity.mobile = None;
                }
            }
            ProjectileTick::GroundHit { pos: hit_pos } => {
                // Mark as inert at ground position
                if let Some(entity) = state.entities.get_mut(proj_key) {
                    entity.pos = Some(hit_pos);
                    entity.projectile = None;
                    entity.mobile = None;
                }
            }
        }
    }

    impacts
}

// ---------------------------------------------------------------------------
// Phase 5: Impact resolution
// ---------------------------------------------------------------------------

/// Apply pending impacts through the D2 pipeline.
fn apply_all_impacts(state: &mut GameState, impacts: &[PendingImpact]) {
    for pending in impacts {
        let target_key = pending.target;

        // Build defender state from entity
        let (facing, vitals_snapshot, defender_skill, block, armor_zones) = {
            let entity = match state.entities.get(target_key) {
                Some(e) => e,
                None => continue,
            };

            let facing = entity.combatant.as_ref().map(|c| c.facing).unwrap_or(0.0);

            let vitals = match &entity.vitals {
                Some(v) => v.clone(),
                None => continue, // no vitals = can't take damage
            };
            let defender_skill = entity
                .person
                .as_ref()
                .map(|p| p.combat_skill)
                .unwrap_or(0.0);

            // Block capability from equipment
            let block = entity
                .equipment
                .as_ref()
                .and_then(|eq| {
                    eq.shield
                        .and_then(|shield_key| state.entities.get(shield_key))
                        .and_then(|shield| shield.weapon_props.as_ref())
                        .or_else(|| {
                            eq.weapon
                                .and_then(|weapon_key| state.entities.get(weapon_key))
                                .and_then(|weapon_entity| weapon_entity.weapon_props.as_ref())
                        })
                })
                .filter(|weapon| weapon.block_arc > 0.0)
                .map(|w| BlockCapability {
                    arc: w.block_arc,
                    efficiency: w.block_efficiency,
                    maneuver: martial::select_block_maneuver(
                        defender_skill,
                        pending.impact.attack_motion,
                        pending.impact.height_diff,
                        pending.impact.tick,
                        target_key,
                        pending.impact.attacker_id,
                    ),
                    read_skill: defender_skill,
                });

            // Per-zone armor lookup
            let mut armor_zones: [Option<super::armor::ArmorProperties>; 5] = Default::default();
            if let Some(eq) = &entity.equipment {
                for (i, slot) in eq.armor_slots.iter().enumerate() {
                    if let Some(armor_key) = slot
                        && let Some(armor_entity) = state.entities.get(*armor_key)
                    {
                        armor_zones[i] = armor_entity.armor_props.clone();
                    }
                }
            }

            (facing, vitals, defender_skill, block, armor_zones)
        };

        // Build DefenderState with references to owned data
        let armor_refs: [Option<&super::armor::ArmorProperties>; 5] = [
            armor_zones[0].as_ref(),
            armor_zones[1].as_ref(),
            armor_zones[2].as_ref(),
            armor_zones[3].as_ref(),
            armor_zones[4].as_ref(),
        ];

        let defender = DefenderState {
            entity_id: target_key,
            facing,
            vitals: &vitals_snapshot,
            block,
            armor_at_zone: armor_refs,
        };

        let result = damage::resolve_impact(&pending.impact, &defender);

        // Record combat observation.
        let impact = &pending.impact;
        let attacker_skill = state
            .entities
            .get(impact.attacker_id)
            .and_then(|e| e.person.as_ref())
            .map(|p| p.combat_skill)
            .unwrap_or(0.0);
        let attacker_weapon = state
            .entities
            .get(impact.attacker_id)
            .and_then(|e| e.equipment.as_ref())
            .and_then(|eq| eq.weapon)
            .and_then(|wk| state.entities.get(wk))
            .and_then(|we| we.weapon_props.as_ref());

        let (
            blocked,
            block_maneuver,
            block_stamina,
            penetrated,
            pen_depth,
            residual,
            wound_sev,
            bleed,
            stagger_force,
            stagger,
            hit_zone,
        ) = match &result {
            ImpactResult::Blocked {
                stamina_cost,
                maneuver,
            } => {
                (
                    true,
                    Some(*maneuver),
                    *stamina_cost,
                    false,
                    0.0,
                    0.0,
                    None,
                    0.0,
                    0.0,
                    false,
                    super::armor::BodyZone::Torso,
                ) // zone unknown for blocks
            }
            ImpactResult::Deflected {
                transmitted_force,
                block_maneuver,
            } => (
                false,
                *block_maneuver,
                0.0,
                false,
                0.0,
                *transmitted_force,
                None,
                0.0,
                *transmitted_force,
                *transmitted_force > 20.0,
                super::armor::BodyZone::Torso,
            ),
            ImpactResult::Wounded {
                wound,
                transmitted_force,
                block_maneuver,
            } => (
                false,
                *block_maneuver,
                0.0,
                true,
                1.0,
                *transmitted_force,
                Some(wound.severity),
                wound.bleed_rate,
                *transmitted_force,
                *transmitted_force > 20.0,
                wound.zone,
            ),
        };

        // Find armor at the hit zone.
        let zone_idx = match hit_zone {
            super::armor::BodyZone::Head => 0,
            super::armor::BodyZone::Torso => 1,
            super::armor::BodyZone::LeftArm => 2,
            super::armor::BodyZone::RightArm => 3,
            super::armor::BodyZone::Legs => 4,
        };
        let hit_armor = armor_zones[zone_idx].as_ref();

        state.combat_log.record(CombatObservation {
            tick: impact.tick,
            attacker: impact.attacker_id,
            defender: target_key,
            damage_type: impact.damage_type,
            weapon_material: attacker_weapon
                .map(|w| w.material)
                .unwrap_or(super::armor::MaterialType::Iron),
            weapon_sharpness: impact.sharpness,
            weapon_hardness: attacker_weapon.map(|w| w.hardness).unwrap_or(5.0),
            weapon_weight: attacker_weapon.map(|w| w.weight).unwrap_or(1.0),
            armor_construction: hit_armor.map(|a| a.construction),
            armor_material: hit_armor.map(|a| a.material),
            armor_hardness: hit_armor.map(|a| a.hardness).unwrap_or(0.0),
            armor_thickness: hit_armor.map(|a| a.thickness).unwrap_or(0.0),
            armor_coverage: hit_armor.map(|a| a.coverage).unwrap_or(0.0),
            hit_zone,
            angle_of_incidence: 0.0, // computed inside resolve_impact, not exposed
            impact_force: impact.kinetic_energy,
            attack_motion: impact.attack_motion,
            blocked,
            block_maneuver,
            block_stamina_cost: block_stamina,
            penetrated,
            penetration_depth: pen_depth,
            residual_force: residual,
            wound_severity: wound_sev,
            bleed_rate: bleed,
            stagger_force,
            stagger,
            distance: 0.0, // available from PendingImpact context in future
            height_diff: impact.height_diff,
            attacker_skill,
            defender_skill,
            defender_stamina: vitals_snapshot.stamina,
            defender_facing_offset: (impact.attack_direction - facing).abs(),
        });

        // Apply result to entity
        if let Some(entity) = state.entities.get_mut(target_key)
            && let (Some(vitals), Some(wounds)) = (&mut entity.vitals, &mut entity.wounds)
        {
            damage::apply_impact_result(result, vitals, wounds);
        }
    }
}

// ---------------------------------------------------------------------------
// Phase 6: Vitals
// ---------------------------------------------------------------------------

/// Tick bleed, stamina recovery, and movement stamina drain for all entities.
fn tick_vitals(state: &mut GameState, dt: f32) {
    let current_tick = state.tick;

    for (_, entity) in &mut state.entities {
        if let Some(vitals) = &mut entity.vitals {
            // Bleed
            if let Some(wounds) = &entity.wounds {
                vitals.tick_bleed(wounds, current_tick, dt);
            }

            // Stamina recovery (also ticks stagger)
            let wounds_ref = entity.wounds.as_ref().map(|w| w.as_slice()).unwrap_or(&[]);
            vitals.tick_stamina_recovery(wounds_ref, dt);

            // Movement drain
            vitals.tick_movement_drain(dt);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::agent::{
        OperationalCommand, OperationsLayer, StrategicDirective, StrategyLayer, TacticalCommand,
        TacticalLayer,
    };
    use super::super::equipment::Equipment;
    use super::super::formation::FormationType;
    use super::super::lifecycle::spawn_entity;
    use super::super::mapgen;
    use super::super::movement::Mobile;
    use super::super::perception::StrategicView;
    use super::super::state::{Combatant, EntityBuilder, Person, Role, Stack, StackId};
    use super::super::vitals::Vitals;
    use super::super::wound::WoundList;
    use super::*;
    use smallvec::smallvec;

    struct NoopStrategy;
    impl StrategyLayer for NoopStrategy {
        fn plan(&mut self, _view: &StrategicView) -> Vec<StrategicDirective> {
            Vec::new()
        }
    }

    struct NoopOperations;
    impl OperationsLayer for NoopOperations {
        fn execute(
            &mut self,
            _state: &GameState,
            _directives: &[StrategicDirective],
            _player: u8,
        ) -> Vec<OperationalCommand> {
            Vec::new()
        }
    }

    struct FixedTactical {
        commands: Vec<TacticalCommand>,
    }

    impl TacticalLayer for FixedTactical {
        fn decide(
            &mut self,
            _state: &GameState,
            _stack: &Stack,
            _player: u8,
        ) -> Vec<TacticalCommand> {
            self.commands.clone()
        }
    }

    #[test]
    fn tick_advances_time() {
        let mut state = mapgen::generate(15, 15, 2, 42);
        assert_eq!(state.tick, 0);
        assert_eq!(state.game_time, 0.0);

        tick(&mut state, 1.0);

        assert_eq!(state.tick, 1);
        assert!((state.game_time - 1.0).abs() < 1e-10);
    }

    #[test]
    fn tick_preserves_entity_count_without_combat() {
        let mut state = mapgen::generate(15, 15, 2, 42);
        let initial_count = state.entities.len();

        // Run several ticks — no combat should happen without agent commands
        for _ in 0..10 {
            tick(&mut state, 1.0);
        }

        assert_eq!(
            state.entities.len(),
            initial_count,
            "no entities should die without combat"
        );
    }

    #[test]
    fn tick_moves_entities_with_waypoints() {
        let mut state = mapgen::generate(15, 15, 2, 42);

        // Find a mobile entity and give it a waypoint
        let (key, initial_pos) = state
            .entities
            .iter()
            .find_map(|(k, e)| {
                if e.mobile.is_some() && e.pos.is_some() {
                    Some((k, e.pos.unwrap()))
                } else {
                    None
                }
            })
            .expect("should have mobile entity");

        let target = Vec3::new(initial_pos.x + 100.0, initial_pos.y, initial_pos.z);
        state.entities[key]
            .mobile
            .as_mut()
            .unwrap()
            .waypoints
            .push(target);

        // Run ticks
        for _ in 0..20 {
            tick(&mut state, 1.0);
        }

        let final_pos = state.entities[key].pos.unwrap();
        let moved_dist = (final_pos - initial_pos).length();
        assert!(
            moved_dist > 5.0,
            "entity should have moved toward waypoint, moved {moved_dist}"
        );
    }

    #[test]
    fn tick_bleeds_wounded_entity() {
        let mut state = mapgen::generate(15, 15, 2, 42);

        // Find an entity with vitals and give it a wound
        let key = state
            .entities
            .iter()
            .find_map(|(k, e)| {
                if e.vitals.is_some() && e.wounds.is_some() {
                    Some(k)
                } else {
                    None
                }
            })
            .expect("should have entity with vitals");

        // Add a wound
        let wound = super::super::wound::Wound {
            zone: super::super::armor::BodyZone::Torso,
            severity: super::super::wound::Severity::Puncture,
            bleed_rate: 0.05,
            damage_type: super::super::armor::DamageType::Pierce,
            attacker_id: key, // self-inflicted for testing
            created_at: 0,
        };
        state.entities[key].wounds.as_mut().unwrap().push(wound);

        let blood_before = state.entities[key].vitals.as_ref().unwrap().blood;

        tick(&mut state, 1.0);

        let blood_after = state.entities[key].vitals.as_ref().unwrap().blood;
        assert!(
            blood_after < blood_before,
            "wounded entity should bleed: {blood_before} → {blood_after}"
        );
    }

    #[test]
    fn tick_makes_dead_entity_inert() {
        let mut state = mapgen::generate(15, 15, 2, 42);

        // Find an entity with vitals + mobile and kill it
        let key = state
            .entities
            .iter()
            .find_map(|(k, e)| {
                if e.vitals.is_some() && e.mobile.is_some() {
                    Some(k)
                } else {
                    None
                }
            })
            .expect("should have entity with vitals");

        state.entities[key].vitals.as_mut().unwrap().blood = 0.0;

        tick(&mut state, 1.0);

        // Entity persists as inert corpse
        assert!(
            state.entities.contains_key(key),
            "dead entity should persist"
        );
        assert!(
            state.entities[key].mobile.is_none(),
            "dead entity loses mobile"
        );
        assert!(
            state.entities[key].pos.is_some(),
            "dead entity keeps position"
        );
    }

    #[test]
    fn tick_detects_elimination() {
        let mut state = mapgen::generate(15, 15, 2, 42);

        // Kill all of player 0's persons — give civilians vitals first, then kill
        let p0_keys: Vec<EntityKey> = state
            .entities
            .iter()
            .filter_map(|(k, e)| {
                if e.owner == Some(0) && e.person.is_some() {
                    Some(k)
                } else {
                    None
                }
            })
            .collect();

        for k in &p0_keys {
            let entity = &mut state.entities[*k];
            if entity.vitals.is_none() {
                entity.vitals = Some(Vitals::new());
                entity.wounds = Some(WoundList::new());
            }
            entity.vitals.as_mut().unwrap().blood = 0.0;
        }

        let result = tick(&mut state, 1.0);

        assert!(
            result.eliminated.contains(&0),
            "player 0 should be eliminated"
        );
        assert!(
            !result.eliminated.contains(&1),
            "player 1 should not be eliminated"
        );
    }

    #[test]
    fn tick_hundred_ticks_no_panic() {
        let mut state = mapgen::generate(20, 20, 2, 99);
        for _ in 0..100 {
            tick(&mut state, 1.0);
        }
        // Just verifying no panics or infinite loops
        assert_eq!(state.tick, 100);
    }

    #[test]
    fn spatial_index_stays_consistent_after_ticks() {
        let mut state = mapgen::generate(15, 15, 2, 42);

        // Give some entities waypoints
        let keys_with_pos: Vec<(EntityKey, Vec3)> = state
            .entities
            .iter()
            .filter_map(|(k, e)| {
                if e.mobile.is_some() {
                    e.pos.map(|p| (k, p))
                } else {
                    None
                }
            })
            .take(5)
            .collect();

        for (key, pos) in &keys_with_pos {
            state.entities[*key]
                .mobile
                .as_mut()
                .unwrap()
                .waypoints
                .push(Vec3::new(pos.x + 50.0, pos.y + 50.0, pos.z));
        }

        for _ in 0..20 {
            tick(&mut state, 1.0);
        }

        // Verify spatial index consistency: every entity with a hex should
        // be in the spatial index at that hex.
        for (key, entity) in &state.entities {
            if let Some(hex) = entity.hex {
                assert!(
                    state.spatial_index.entities_at(hex).contains(&key),
                    "entity {key:?} should be in spatial index at {hex:?}"
                );
            }
        }
    }

    #[test]
    fn melee_attack_resolves_in_tick() {
        use super::super::spatial::GeoMaterial;
        use super::super::spatial::Heightfield;

        let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
        let mut state = GameState::new(20, 20, 2, hf);

        // Spawn attacker with a weapon
        let attacker = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(100.0, 100.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals()
                .equipment(Equipment::empty()),
        );

        let sword = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .owner(0)
                .weapon_props(super::super::weapon::iron_sword()),
        );
        super::super::lifecycle::contain(&mut state, attacker, sword);
        state.entities[attacker].equipment.as_mut().unwrap().weapon = Some(sword);

        // Spawn target within reach
        let target = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(101.0, 100.0, 0.0))
                .owner(1)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals()
                .equipment(Equipment::empty()),
        );

        // Set up attack
        let attack = super::super::weapon::AttackState::new(target, sword);
        state.entities[attacker].combatant.as_mut().unwrap().attack = Some(attack);

        // Tick through windup (iron sword: 4 ticks)
        let _blood_before = state.entities[target].vitals.as_ref().unwrap().blood;

        for _ in 0..5 {
            tick(&mut state, 1.0);
        }

        // Target should have taken damage OR the attack whiffed (distance-dependent)
        // At 1m apart with 1.5m reach, should hit.
        let _blood_after = state
            .entities
            .get(target)
            .map(|e| e.vitals.as_ref().unwrap().blood);

        // Attack should have resolved (combat state cleared)
        let attack_cleared = state
            .entities
            .get(attacker)
            .and_then(|e| e.combatant.as_ref())
            .map(|c| c.attack.is_none())
            .unwrap_or(true);

        assert!(attack_cleared, "attack should have resolved after windup");
    }

    #[test]
    fn run_agent_phase_applies_tactical_commands() {
        use super::super::spatial::GeoMaterial;
        use super::super::spatial::Heightfield;

        let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
        let mut state = GameState::new(20, 20, 2, hf);

        let attacker = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(100.0, 100.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals()
                .equipment(Equipment::empty()),
        );
        let sword = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .owner(0)
                .weapon_props(super::super::weapon::iron_sword()),
        );
        super::super::lifecycle::contain(&mut state, attacker, sword);
        state.entities[attacker].equipment.as_mut().unwrap().weapon = Some(sword);

        let target = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(101.0, 100.0, 0.0))
                .owner(1)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals()
                .equipment(Equipment::empty()),
        );

        state.stacks.push(Stack {
            id: StackId(1),
            owner: 0,
            members: smallvec![attacker],
            formation: FormationType::Line,
            leader: attacker,
        });

        let mut agents = [LayeredAgent::new(
            Box::new(NoopStrategy),
            Box::new(NoopOperations),
            Box::new(FixedTactical {
                commands: vec![TacticalCommand::Attack { attacker, target }],
            }),
            0,
            50,
            5,
        )];

        let phase = run_agent_phase(&mut state, &mut agents);

        assert_eq!(phase.outputs.len(), 1);
        assert_eq!(phase.summaries.len(), 1);
        assert_eq!(phase.summaries[0].tactical_applied, 1);
        assert_eq!(
            state.entities[attacker]
                .combatant
                .as_ref()
                .and_then(|combatant| combatant.attack.as_ref())
                .map(|attack| attack.target),
            Some(target)
        );
    }

    #[test]
    fn tick_with_agents_runs_agent_phase_before_movement() {
        let mut state = mapgen::generate(15, 15, 2, 42);

        let mover = state
            .entities
            .iter()
            .find_map(|(key, entity)| {
                (entity.owner == Some(0) && entity.mobile.is_some() && entity.pos.is_some())
                    .then_some(key)
            })
            .expect("should have mobile entity for player 0");
        let initial_pos = state.entities[mover].pos.unwrap();

        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members: smallvec![mover],
            formation: FormationType::Line,
            leader: mover,
        });

        struct RouteOperations {
            stack: StackId,
            destination: Vec3,
        }

        impl OperationsLayer for RouteOperations {
            fn execute(
                &mut self,
                _state: &GameState,
                _directives: &[StrategicDirective],
                _player: u8,
            ) -> Vec<OperationalCommand> {
                vec![OperationalCommand::RouteStack {
                    stack: self.stack,
                    waypoints: vec![self.destination],
                }]
            }
        }

        let mut agents = [LayeredAgent::new(
            Box::new(NoopStrategy),
            Box::new(RouteOperations {
                stack: stack_id,
                destination: Vec3::new(initial_pos.x + 100.0, initial_pos.y, initial_pos.z),
            }),
            Box::new(FixedTactical {
                commands: Vec::new(),
            }),
            0,
            1,
            1,
        )];

        let result = tick_with_agents(&mut state, &mut agents, 1.0);

        assert_eq!(result.impacts, 0);
        assert_eq!(state.tick, 1);
        let final_pos = state.entities[mover].pos.unwrap();
        assert!(
            final_pos.x > initial_pos.x,
            "entity should move after route command is applied before movement"
        );
    }
}
