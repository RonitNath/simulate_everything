/// V3 shared tactical layer: per-stack combat decisions every tick.
///
/// Runs for each stack within engagement range of enemies. Assigns targets
/// using damage table matchup reasoning, manages formations, facing, and
/// retreat decisions.
use super::agent::{TacticalCommand, TacticalLayer};
use super::damage_table::{DamageEstimateTable, MatchupKey};
use super::armor::{ArmorConstruction, DamageType, MaterialType};
use super::formation::FormationType;
use super::state::{GameState, Stack, StackId};
use super::index::ring_hexes;
use super::hex::world_to_hex;
use crate::v2::state::EntityKey;

// ---------------------------------------------------------------------------
// Tunable constants
// ---------------------------------------------------------------------------

/// Engagement detection radius in meters (matches agent.rs).
const ENGAGEMENT_RADIUS: f32 = 300.0;

/// Rolling window size (ticks) for retreat decision.
const RETREAT_WINDOW: usize = 20;

/// Retreat when own casualty rate exceeds this multiple of enemy rate.
const RETREAT_RATIO: f32 = 2.0;

/// Number of hex rings to search for nearby enemies.
const SEARCH_RINGS: i32 = 4;

// ---------------------------------------------------------------------------
// SharedTacticalLayer
// ---------------------------------------------------------------------------

/// Shared tactical layer used by all agent personalities.
pub struct SharedTacticalLayer {
    /// Per-agent damage table (shared with operations).
    pub damage_table: DamageEstimateTable,
    /// Rolling casualty tracking: (tick, own_deaths, enemy_deaths).
    casualty_history: Vec<(u64, u32, u32)>,
}

impl SharedTacticalLayer {
    pub fn new(damage_table: DamageEstimateTable) -> Self {
        Self {
            damage_table,
            casualty_history: Vec::new(),
        }
    }

    /// Record casualties for retreat tracking. Called externally after damage resolution.
    pub fn record_casualties(&mut self, tick: u64, own_deaths: u32, enemy_deaths: u32) {
        self.casualty_history.push((tick, own_deaths, enemy_deaths));
        // Trim old entries beyond the window.
        if self.casualty_history.len() > RETREAT_WINDOW * 2 {
            let cutoff = self.casualty_history.len() - RETREAT_WINDOW;
            self.casualty_history.drain(..cutoff);
        }
    }

    /// Check if retreat should be ordered based on recent casualty rates.
    fn should_retreat(&self, current_tick: u64) -> bool {
        let window_start = current_tick.saturating_sub(RETREAT_WINDOW as u64);
        let recent: Vec<_> = self.casualty_history.iter()
            .filter(|(t, _, _)| *t >= window_start)
            .collect();

        if recent.is_empty() { return false; }

        let own_total: u32 = recent.iter().map(|(_, own, _)| own).sum();
        let enemy_total: u32 = recent.iter().map(|(_, _, enemy)| enemy).sum();

        if enemy_total == 0 {
            // We're taking losses but enemy isn't — retreat if we've lost anyone.
            return own_total > 0;
        }

        own_total as f32 / enemy_total as f32 > RETREAT_RATIO
    }

    /// Select targets for stack members using damage table matchup reasoning.
    fn assign_targets(
        &self,
        state: &GameState,
        stack: &Stack,
    ) -> Vec<TacticalCommand> {
        let mut commands = Vec::new();
        let nearby_enemies = find_nearby_enemies(state, stack, ENGAGEMENT_RADIUS);

        if nearby_enemies.is_empty() { return commands; }

        for &member_key in &stack.members {
            let member = match state.entities.get(member_key) {
                Some(e) => e,
                None => continue,
            };

            // Get member's weapon damage type.
            let weapon_dt = member.equipment.as_ref()
                .and_then(|eq| eq.weapon)
                .and_then(|wk| state.entities.get(wk))
                .and_then(|we| we.weapon_props.as_ref())
                .map(|wp| (wp.damage_type, wp.material));

            // Score each enemy by expected damage output.
            let best_target = nearby_enemies.iter()
                .filter_map(|&enemy_key| {
                    let enemy = state.entities.get(enemy_key)?;
                    let score = self.score_target(weapon_dt, enemy_key, state);
                    Some((enemy_key, score))
                })
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            if let Some((target_key, _score)) = best_target {
                commands.push(TacticalCommand::Attack {
                    attacker: member_key,
                    target: target_key,
                });
            }
        }

        commands
    }

    /// Score how effective our weapon is against a target's armor.
    fn score_target(
        &self,
        weapon_info: Option<(DamageType, MaterialType)>,
        target_key: EntityKey,
        state: &GameState,
    ) -> f32 {
        let (damage_type, weapon_mat) = weapon_info.unwrap_or((DamageType::Slash, MaterialType::Iron));

        let target = match state.entities.get(target_key) {
            Some(e) => e,
            None => return 0.0,
        };

        // Check target's armor.
        let armor_info = target.equipment.as_ref()
            .and_then(|eq| {
                // Check torso armor as representative.
                eq.armor_slots[1] // torso index
                    .and_then(|ak| state.entities.get(ak))
                    .and_then(|ae| ae.armor_props.as_ref())
                    .map(|ap| (ap.construction, ap.material))
            });

        let (ac, am) = armor_info.unwrap_or((ArmorConstruction::Padded, MaterialType::Leather));

        let key = MatchupKey {
            damage_type,
            weapon_material: weapon_mat,
            armor_construction: ac,
            armor_material: am,
        };

        self.damage_table.get(&key)
            .map(|est| est.wound_rate)
            .unwrap_or(0.5) // no data = neutral
    }

    /// Assign formation based on stack composition.
    fn assign_formation(
        &self,
        state: &GameState,
        stack: &Stack,
    ) -> Option<TacticalCommand> {
        let mut has_ranged = false;
        let mut has_shield = false;
        let member_count = stack.members.len();

        for &member_key in &stack.members {
            let member = match state.entities.get(member_key) {
                Some(e) => e,
                None => continue,
            };
            if let Some(eq) = &member.equipment {
                if eq.shield.is_some() { has_shield = true; }
                if let Some(wk) = eq.weapon {
                    if let Some(we) = state.entities.get(wk) {
                        if let Some(wp) = &we.weapon_props {
                            if wp.is_ranged() { has_ranged = true; }
                        }
                    }
                }
            }
        }

        let formation = if has_ranged && !has_shield {
            FormationType::Skirmish
        } else if has_shield && member_count >= 4 {
            FormationType::Line
        } else if member_count >= 8 {
            FormationType::Square
        } else {
            FormationType::Column
        };

        // Only emit if different from current.
        if stack.formation != formation {
            Some(TacticalCommand::SetFormation {
                stack: stack.id,
                formation,
            })
        } else {
            None
        }
    }

    /// Set facing for stack members toward nearest threat.
    fn assign_facing(
        &self,
        state: &GameState,
        stack: &Stack,
    ) -> Vec<TacticalCommand> {
        let mut commands = Vec::new();
        let nearby_enemies = find_nearby_enemies(state, stack, ENGAGEMENT_RADIUS);

        if nearby_enemies.is_empty() { return commands; }

        // Find centroid of enemies.
        let (mut cx, mut cy, mut count) = (0.0f32, 0.0f32, 0u32);
        for &ek in &nearby_enemies {
            if let Some(e) = state.entities.get(ek) {
                if let Some(pos) = e.pos {
                    cx += pos.x;
                    cy += pos.y;
                    count += 1;
                }
            }
        }
        if count == 0 { return commands; }
        cx /= count as f32;
        cy /= count as f32;

        for &member_key in &stack.members {
            let member = match state.entities.get(member_key) {
                Some(e) => e,
                None => continue,
            };
            if let Some(pos) = member.pos {
                let dx = cx - pos.x;
                let dy = cy - pos.y;
                let angle = dy.atan2(dx);
                commands.push(TacticalCommand::SetFacing {
                    entity: member_key,
                    angle,
                });
            }
        }

        commands
    }
}

impl TacticalLayer for SharedTacticalLayer {
    fn decide(
        &mut self,
        state: &GameState,
        stack: &Stack,
        _player: u8,
    ) -> Vec<TacticalCommand> {
        let mut commands = Vec::new();

        // Check retreat.
        if self.should_retreat(state.tick) {
            // Issue retreat toward the rear (away from enemies).
            let retreat_pos = retreat_direction(state, stack);
            for &member_key in &stack.members {
                commands.push(TacticalCommand::Retreat {
                    entity: member_key,
                    toward: retreat_pos,
                });
            }
            return commands;
        }

        // Formation assignment.
        if let Some(cmd) = self.assign_formation(state, stack) {
            commands.push(cmd);
        }

        // Target assignment via matchup reasoning.
        commands.extend(self.assign_targets(state, stack));

        // Facing toward threats.
        commands.extend(self.assign_facing(state, stack));

        commands
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find enemy entities within `radius` of any stack member.
fn find_nearby_enemies(state: &GameState, stack: &Stack, radius: f32) -> Vec<EntityKey> {
    let mut enemies = Vec::new();
    let radius_sq = radius * radius;

    for &member_key in &stack.members {
        let member = match state.entities.get(member_key) {
            Some(e) => e,
            None => continue,
        };
        let member_pos = match member.pos {
            Some(p) => p,
            None => continue,
        };
        let member_hex = world_to_hex(member_pos);

        for hex in ring_hexes(member_hex, SEARCH_RINGS) {
            for &entity_key in state.spatial_index.entities_at(hex) {
                if enemies.contains(&entity_key) { continue; }
                let entity = match state.entities.get(entity_key) {
                    Some(e) => e,
                    None => continue,
                };
                let entity_owner = match entity.owner {
                    Some(o) => o,
                    None => continue,
                };
                if entity_owner == stack.owner { continue; }
                if entity.person.is_none() { continue; }
                if let Some(pos) = entity.pos {
                    let dx = pos.x - member_pos.x;
                    let dy = pos.y - member_pos.y;
                    if dx * dx + dy * dy <= radius_sq {
                        enemies.push(entity_key);
                    }
                }
            }
        }
    }

    enemies
}

/// Compute retreat direction: away from enemy centroid.
fn retreat_direction(state: &GameState, stack: &Stack) -> super::spatial::Vec3 {
    use super::spatial::Vec3;

    // Stack centroid.
    let (mut sx, mut sy, mut sc) = (0.0f32, 0.0f32, 0u32);
    for &mk in &stack.members {
        if let Some(e) = state.entities.get(mk) {
            if let Some(pos) = e.pos {
                sx += pos.x;
                sy += pos.y;
                sc += 1;
            }
        }
    }
    if sc == 0 { return Vec3::ZERO; }
    sx /= sc as f32;
    sy /= sc as f32;

    // Enemy centroid.
    let enemies = find_nearby_enemies(state, stack, ENGAGEMENT_RADIUS);
    let (mut ex, mut ey, mut ec) = (0.0f32, 0.0f32, 0u32);
    for &ek in &enemies {
        if let Some(e) = state.entities.get(ek) {
            if let Some(pos) = e.pos {
                ex += pos.x;
                ey += pos.y;
                ec += 1;
            }
        }
    }
    if ec == 0 { return Vec3::new(sx, sy, 0.0); }
    ex /= ec as f32;
    ey /= ec as f32;

    // Retreat away from enemies.
    let dx = sx - ex;
    let dy = sy - ey;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    Vec3::new(sx + dx / len * 100.0, sy + dy / len * 100.0, 0.0)
}

// ---------------------------------------------------------------------------
// Null tactical — does nothing (for testing)
// ---------------------------------------------------------------------------

/// Tactical layer that issues no commands. For testing baseline behavior.
pub struct NullTacticalLayer;

impl TacticalLayer for NullTacticalLayer {
    fn decide(
        &mut self,
        _state: &GameState,
        _stack: &Stack,
        _player: u8,
    ) -> Vec<TacticalCommand> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lifecycle::spawn_entity;
    use super::super::movement::Mobile;
    use super::super::spatial::{GeoMaterial, Heightfield, Vec3};
    use super::super::state::{Combatant, EntityBuilder, Person, Role};
    use super::super::equipment::Equipment;
    use super::super::weapon;
    use smallvec::SmallVec;

    fn test_state() -> GameState {
        let hf = Heightfield::new(30, 30, 0.0, GeoMaterial::Soil);
        GameState::new(30, 30, 2, hf)
    }

    fn spawn_soldier(state: &mut GameState, pos: Vec3, owner: u8) -> EntityKey {
        spawn_entity(state, EntityBuilder::new()
            .pos(pos)
            .owner(owner)
            .person(Person { role: Role::Soldier, combat_skill: 0.5 })
            .mobile(Mobile::new(2.0, 10.0))
            .combatant(Combatant::new())
            .vitals())
    }

    fn spawn_armed_soldier(state: &mut GameState, pos: Vec3, owner: u8) -> EntityKey {
        use super::super::lifecycle::contain;

        let soldier = spawn_soldier(state, pos, owner);

        // Create a sword entity and contain it.
        let sword_key = spawn_entity(state, EntityBuilder::new()
            .weapon_props(weapon::iron_sword()));
        contain(state, soldier, sword_key);

        // Equip the sword.
        if let Some(entity) = state.entities.get_mut(soldier) {
            let mut eq = Equipment::empty();
            eq.weapon = Some(sword_key);
            entity.equipment = Some(eq);
        }

        soldier
    }

    fn make_stack(state: &mut GameState, members: &[EntityKey], owner: u8) -> StackId {
        let id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id,
            owner,
            members: SmallVec::from_slice(members),
            formation: FormationType::Column,
            leader: members[0],
        });
        id
    }

    #[test]
    fn target_assignment_finds_enemies() {
        let mut state = test_state();
        let s1 = spawn_armed_soldier(&mut state, Vec3::new(100.0, 100.0, 0.0), 0);
        let _e1 = spawn_soldier(&mut state, Vec3::new(150.0, 100.0, 0.0), 1);

        let stack_id = make_stack(&mut state, &[s1], 0);

        let mut tactical = SharedTacticalLayer::new(DamageEstimateTable::from_physics());
        let commands = tactical.decide(&state, &state.stacks[0], 0);

        let attacks: Vec<_> = commands.iter()
            .filter(|c| matches!(c, TacticalCommand::Attack { .. }))
            .collect();
        assert!(!attacks.is_empty(), "should assign attack targets");
    }

    #[test]
    fn facing_toward_enemies() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(100.0, 100.0, 0.0), 0);
        let _e1 = spawn_soldier(&mut state, Vec3::new(200.0, 100.0, 0.0), 1);

        make_stack(&mut state, &[s1], 0);

        let mut tactical = SharedTacticalLayer::new(DamageEstimateTable::from_physics());
        let commands = tactical.decide(&state, &state.stacks[0], 0);

        let facings: Vec<_> = commands.iter()
            .filter_map(|c| match c {
                TacticalCommand::SetFacing { angle, .. } => Some(*angle),
                _ => None,
            })
            .collect();
        assert!(!facings.is_empty(), "should set facing");
        // Enemy is to the east (positive x), so angle should be near 0.
        assert!((facings[0]).abs() < 0.5, "should face east toward enemy: {}", facings[0]);
    }

    #[test]
    fn retreat_when_losing() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(100.0, 100.0, 0.0), 0);
        let _e1 = spawn_soldier(&mut state, Vec3::new(150.0, 100.0, 0.0), 1);

        make_stack(&mut state, &[s1], 0);
        state.tick = 30;

        let mut tactical = SharedTacticalLayer::new(DamageEstimateTable::from_physics());

        // Record heavy losses.
        for t in 10..30 {
            tactical.record_casualties(t, 3, 1); // 3:1 loss ratio
        }

        let commands = tactical.decide(&state, &state.stacks[0], 0);
        let retreats = commands.iter()
            .filter(|c| matches!(c, TacticalCommand::Retreat { .. }))
            .count();
        assert!(retreats > 0, "should retreat when losing badly");
    }

    #[test]
    fn no_retreat_when_winning() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(100.0, 100.0, 0.0), 0);
        let _e1 = spawn_soldier(&mut state, Vec3::new(150.0, 100.0, 0.0), 1);

        make_stack(&mut state, &[s1], 0);
        state.tick = 30;

        let mut tactical = SharedTacticalLayer::new(DamageEstimateTable::from_physics());

        // Record favorable losses.
        for t in 10..30 {
            tactical.record_casualties(t, 1, 3); // 1:3 ratio — winning
        }

        let commands = tactical.decide(&state, &state.stacks[0], 0);
        let retreats = commands.iter()
            .filter(|c| matches!(c, TacticalCommand::Retreat { .. }))
            .count();
        assert_eq!(retreats, 0, "should not retreat when winning");
    }

    #[test]
    fn formation_changes_for_ranged() {
        let mut state = test_state();
        // Spawn a ranged soldier (bow).
        let s1 = spawn_entity(&mut state, EntityBuilder::new()
            .pos(Vec3::new(100.0, 100.0, 0.0))
            .owner(0)
            .person(Person { role: Role::Soldier, combat_skill: 0.5 })
            .mobile(Mobile::new(2.0, 10.0))
            .combatant(Combatant::new())
            .vitals());

        let bow_key = spawn_entity(&mut state, EntityBuilder::new()
            .weapon_props(weapon::wooden_bow()));
        super::super::lifecycle::contain(&mut state, s1, bow_key);
        if let Some(e) = state.entities.get_mut(s1) {
            let mut eq = Equipment::empty();
            eq.weapon = Some(bow_key);
            e.equipment = Some(eq);
        }

        let _e1 = spawn_soldier(&mut state, Vec3::new(200.0, 100.0, 0.0), 1);
        make_stack(&mut state, &[s1], 0);

        let mut tactical = SharedTacticalLayer::new(DamageEstimateTable::from_physics());
        let commands = tactical.decide(&state, &state.stacks[0], 0);

        let formation_cmds: Vec<_> = commands.iter()
            .filter_map(|c| match c {
                TacticalCommand::SetFormation { formation, .. } => Some(*formation),
                _ => None,
            })
            .collect();

        // Ranged without shield should get Skirmish.
        assert!(formation_cmds.contains(&FormationType::Skirmish),
            "ranged units should use Skirmish formation: {:?}", formation_cmds);
    }

    #[test]
    fn no_commands_without_enemies() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(100.0, 100.0, 0.0), 0);
        make_stack(&mut state, &[s1], 0);
        // No enemies spawned.

        let mut tactical = SharedTacticalLayer::new(DamageEstimateTable::from_physics());
        let commands = tactical.decide(&state, &state.stacks[0], 0);

        let attacks = commands.iter()
            .filter(|c| matches!(c, TacticalCommand::Attack { .. }))
            .count();
        assert_eq!(attacks, 0, "should not attack without enemies");
    }
}
