use smallvec::SmallVec;

use super::damage::{self, BlockCapability, DefenderState, Impact, ImpactResult};
use super::equipment::zone_index;
use super::hex::world_to_hex;
use super::index::update_hex_membership;
use super::lifecycle::{self, cleanup_dead, cleanup_inert_projectiles};
use super::movement::{self, Mobile};
use super::projectile::{self, ProjectileTick};
use super::spatial::Vec3;
use super::state::GameState;
use super::steering;
use super::weapon::{self, AttackTick};
use super::wound::WoundList;
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

// ---------------------------------------------------------------------------
// Main tick function
// ---------------------------------------------------------------------------

/// Advance the simulation by one tick.
///
/// Orchestrates all subsystems in the correct order:
/// 1. Spatial index rebuild
/// 2. Steering → movement
/// 3. Melee combat
/// 4. Projectile advancement
/// 5. Impact resolution
/// 6. Vitals (bleed, stamina, stagger)
/// 7. Cleanup (dead entities, spent projectiles)
/// 8. Elimination check
pub fn tick(state: &mut GameState, dt: f64) -> TickResult {
    let mut result = TickResult::default();
    let dt_f32 = dt as f32;

    // --- Phase 1: Spatial index ---
    rebuild_spatial_index(state);

    // --- Phase 2: Movement ---
    // TODO: Agent layer produces commands here (A domain, not yet implemented)
    compute_steering_and_move(state, dt_f32);

    // --- Phase 3: Melee combat ---
    let melee_impacts = resolve_melee_attacks(state);

    // --- Phase 4: Projectile advancement ---
    let projectile_impacts = advance_projectiles(state);
    result.impacts = melee_impacts.len() + projectile_impacts.len();

    // --- Phase 5: Impact resolution ---
    apply_all_impacts(state, &melee_impacts);
    apply_all_impacts(state, &projectile_impacts);

    // --- Phase 6: Vitals ---
    tick_vitals(state, dt_f32);

    // --- Phase 7: Cleanup ---
    let count_before = state.entities.len();
    cleanup_dead(state);
    result.deaths = count_before - state.entities.len();
    cleanup_inert_projectiles(state);

    // --- Phase 8: Elimination ---
    result.eliminated = lifecycle::check_elimination(state);

    // --- Advance time ---
    state.game_time += dt;
    state.tick += 1;

    result
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
                .map(|w| {
                    super::wound::zone_wound_weight(
                        w,
                        super::armor::BodyZone::Legs,
                    )
                })
                .unwrap_or(0.0);

            let speed_factors = movement::SpeedFactors {
                base_capability: 3.0, // default person speed
                slope_factor: 1.0,    // TODO: compute from heightfield
                surface_factor: 1.0,  // TODO: compute from material at pos
                encumbrance_factor: 1.0, // TODO: compute from carried weight
                wound_factor: movement::wound_factor(leg_wound_weight),
                stamina_factor: movement::stamina_factor(stamina),
            };
            let derived_speed = speed_factors.derived_speed();

            // Steering: seek next waypoint with separation
            let mut accel = Vec3::ZERO;

            if let Some(&wp) = mobile.waypoints.first() {
                accel = steering::arrive(
                    pos,
                    mobile.vel,
                    wp,
                    mobile.steering_force,
                    derived_speed,
                    50.0,
                );
            }

            // Separation from nearby entities
            let neighbors: SmallVec<[Vec3; 16]> = mobile_positions
                .iter()
                .filter(|(k, _)| *k != key)
                .filter(|(_, p)| (*p - pos).length_squared() < 30.0 * 30.0)
                .map(|(_, p)| *p)
                .collect();

            if !neighbors.is_empty() {
                let sep = steering::separation(pos, &neighbors, 15.0);
                accel = accel + sep;
            }

            // Integrate
            let mut mobile_clone = mobile.clone();
            let new_pos =
                movement::integrate(pos, &mut mobile_clone, accel, derived_speed, dt);

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
            let attack = match entity
                .combatant
                .as_mut()
                .and_then(|c| c.attack.as_mut())
            {
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
                let target_pos = state
                    .entities
                    .get(target_key)
                    .and_then(|e| e.pos)
                    .unwrap_or(Vec3::ZERO);

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

                if let Some(impact) = weapon::resolve_melee(
                    &weapon_props,
                    attacker_key,
                    attacker_pos,
                    target_pos,
                    stagger.as_ref(),
                    state.tick,
                ) {
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
                    combatant.cooldown =
                        Some(weapon::CooldownState::new(cd));
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
            if cd.tick() {
                Some(key)
            } else {
                None
            }
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
            let radius = entity
                .mobile
                .as_ref()
                .map(|m| m.radius)
                .unwrap_or(10.0); // default collision radius for non-mobile
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
            |x, y| {
                // TODO: use heightfield for proper terrain height
                let _ = (x, y);
                0.0
            },
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
        let (facing, vitals_snapshot, block, armor_zones) = {
            let entity = match state.entities.get(target_key) {
                Some(e) => e,
                None => continue,
            };

            let facing = entity
                .combatant
                .as_ref()
                .map(|c| c.facing)
                .unwrap_or(0.0);

            let vitals = match &entity.vitals {
                Some(v) => v.clone(),
                None => continue, // no vitals = can't take damage
            };

            // Block capability from equipment
            let block = entity
                .equipment
                .as_ref()
                .and_then(|eq| eq.shield)
                .and_then(|shield_key| state.entities.get(shield_key))
                .and_then(|shield| shield.weapon_props.as_ref())
                .map(|w| BlockCapability {
                    arc: w.block_arc,
                    efficiency: w.block_efficiency,
                });

            // Per-zone armor lookup
            let mut armor_zones: [Option<super::armor::ArmorProperties>; 5] =
                Default::default();
            if let Some(eq) = &entity.equipment {
                for (i, slot) in eq.armor_slots.iter().enumerate() {
                    if let Some(armor_key) = slot {
                        if let Some(armor_entity) = state.entities.get(*armor_key) {
                            armor_zones[i] = armor_entity.armor_props.clone();
                        }
                    }
                }
            }

            (facing, vitals, block, armor_zones)
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

        // Apply result to entity
        if let Some(entity) = state.entities.get_mut(target_key) {
            if let (Some(vitals), Some(wounds)) =
                (&mut entity.vitals, &mut entity.wounds)
            {
                damage::apply_impact_result(result, vitals, wounds);
            }
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
            let wounds_ref = entity
                .wounds
                .as_ref()
                .map(|w| w.as_slice())
                .unwrap_or(&[]);
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
    use super::*;
    use super::super::equipment::Equipment;
    use super::super::lifecycle::spawn_entity;
    use super::super::mapgen;
    use super::super::movement::Mobile;
    use super::super::state::{
        Combatant, EntityBuilder, Person, Role,
    };
    use super::super::vitals::Vitals;

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
    fn tick_removes_dead_entity() {
        let mut state = mapgen::generate(15, 15, 2, 42);

        // Find an entity with vitals and kill it
        let key = state
            .entities
            .iter()
            .find_map(|(k, e)| {
                if e.vitals.is_some() {
                    Some(k)
                } else {
                    None
                }
            })
            .expect("should have entity with vitals");

        state.entities[key].vitals.as_mut().unwrap().blood = 0.0;

        tick(&mut state, 1.0);

        assert!(!state.entities.contains_key(key), "dead entity should be removed");
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
        let blood_before = state.entities[target].vitals.as_ref().unwrap().blood;

        for _ in 0..5 {
            tick(&mut state, 1.0);
        }

        // Target should have taken damage OR the attack whiffed (distance-dependent)
        // At 1m apart with 1.5m reach, should hit.
        let blood_after = state.entities.get(target).map(|e| {
            e.vitals.as_ref().unwrap().blood
        });

        // Attack should have resolved (combat state cleared)
        let attack_cleared = state
            .entities
            .get(attacker)
            .and_then(|e| e.combatant.as_ref())
            .map(|c| c.attack.is_none())
            .unwrap_or(true);

        assert!(attack_cleared, "attack should have resolved after windup");
    }
}
