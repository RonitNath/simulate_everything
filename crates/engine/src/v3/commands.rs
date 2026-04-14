use super::agent::{AgentOutput, EntityTask, OperationalCommand, TacticalCommand};
use super::economy;
use super::equipment::{self, Equipment};
use super::formation::FormationType;
use super::lifecycle::{contain, uncontain};
use super::martial;
use super::movement::Mobile;
use super::state::{Combatant, GameState, Role, Stack, TaskAssignment};
use super::vitals::Vitals;
use super::weapon::AttackState;
use crate::v2::state::EntityKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Applied,
    Deferred,
    Rejected,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CommandApplySummary {
    pub operational_applied: usize,
    pub operational_rejected: usize,
    pub operational_deferred: usize,
    pub tactical_applied: usize,
    pub tactical_rejected: usize,
    pub tactical_deferred: usize,
}

impl CommandApplySummary {
    fn record_operational(&mut self, status: CommandStatus) {
        match status {
            CommandStatus::Applied => self.operational_applied += 1,
            CommandStatus::Deferred => self.operational_deferred += 1,
            CommandStatus::Rejected => self.operational_rejected += 1,
        }
    }

    fn record_tactical(&mut self, status: CommandStatus) {
        match status {
            CommandStatus::Applied => self.tactical_applied += 1,
            CommandStatus::Deferred => self.tactical_deferred += 1,
            CommandStatus::Rejected => self.tactical_rejected += 1,
        }
    }
}

pub fn apply_agent_output(state: &mut GameState, output: &AgentOutput) -> CommandApplySummary {
    let mut summary = CommandApplySummary::default();
    for cmd in &output.operational_commands {
        summary.record_operational(apply_operational_command(state, cmd));
    }
    for cmd in &output.tactical_commands {
        summary.record_tactical(apply_tactical_command(state, cmd));
    }
    summary
}

pub fn validate_operational_command(cmd: &OperationalCommand, state: &GameState) -> bool {
    match cmd {
        OperationalCommand::AssignTask { entity, .. } => {
            if state.entities.get(*entity).is_none() {
                tracing::warn!("AssignTask: entity {:?} not found, dropping", entity);
                return false;
            }
        }
        OperationalCommand::FormStack { entities, .. } => {
            if entities.is_empty() {
                tracing::warn!("FormStack: empty entity list, dropping");
                return false;
            }
            for e in entities {
                if state.entities.get(*e).is_none() {
                    tracing::warn!("FormStack: entity {:?} not found, dropping", e);
                    return false;
                }
            }
        }
        OperationalCommand::RouteStack { stack, .. } => {
            if !state.stacks.iter().any(|s| s.id == *stack) {
                tracing::warn!("RouteStack: stack {:?} not found, dropping", stack);
                return false;
            }
        }
        OperationalCommand::DisbandStack { stack } => {
            if !state.stacks.iter().any(|s| s.id == *stack) {
                tracing::warn!("DisbandStack: stack {:?} not found, dropping", stack);
                return false;
            }
        }
        OperationalCommand::ProduceEquipment { workshop, .. } => {
            if state.entities.get(*workshop).is_none() {
                tracing::warn!(
                    "ProduceEquipment: workshop {:?} not found, dropping",
                    workshop
                );
                return false;
            }
        }
        OperationalCommand::EquipEntity { entity, equipment } => {
            if state.entities.get(*entity).is_none() {
                tracing::warn!("EquipEntity: entity {:?} not found, dropping", entity);
                return false;
            }
            if state.entities.get(*equipment).is_none() {
                tracing::warn!("EquipEntity: equipment {:?} not found, dropping", equipment);
                return false;
            }
        }
        OperationalCommand::EstablishSupplyRoute { .. } => {}
        OperationalCommand::FoundSettlement { entity, .. } => {
            if state.entities.get(*entity).is_none() {
                tracing::warn!("FoundSettlement: entity {:?} not found, dropping", entity);
                return false;
            }
        }
    }
    true
}

pub fn validate_tactical_command(cmd: &TacticalCommand, state: &GameState) -> bool {
    match cmd {
        TacticalCommand::Attack { attacker, target } => {
            if state.entities.get(*attacker).is_none() {
                tracing::warn!("Attack: attacker {:?} not found, dropping", attacker);
                return false;
            }
            if state.entities.get(*target).is_none() {
                tracing::warn!("Attack: target {:?} not found, dropping", target);
                return false;
            }
        }
        TacticalCommand::SetFacing { entity, .. }
        | TacticalCommand::Block { entity }
        | TacticalCommand::Hold { entity } => {
            if state.entities.get(*entity).is_none() {
                tracing::warn!("Tactical: entity {:?} not found, dropping", entity);
                return false;
            }
        }
        TacticalCommand::Retreat { entity, .. } => {
            if state.entities.get(*entity).is_none() {
                tracing::warn!("Retreat: entity {:?} not found, dropping", entity);
                return false;
            }
        }
        TacticalCommand::SetFormation { stack, .. } => {
            if !state.stacks.iter().any(|s| s.id == *stack) {
                tracing::warn!("SetFormation: stack {:?} not found, dropping", stack);
                return false;
            }
        }
    }
    true
}

pub fn apply_operational_command(state: &mut GameState, cmd: &OperationalCommand) -> CommandStatus {
    if !validate_operational_command(cmd, state) {
        return CommandStatus::Rejected;
    }

    match cmd {
        OperationalCommand::AssignTask { entity, task } => {
            apply_entity_task(state, *entity, task);
            CommandStatus::Applied
        }
        OperationalCommand::FormStack { entities, .. } => {
            let leader = entities[0];
            let owner = state
                .entities
                .get(leader)
                .and_then(|e| e.owner)
                .unwrap_or(0);
            let stack_id = state.alloc_stack_id();
            state.stacks.push(Stack {
                id: stack_id,
                owner,
                members: entities.iter().copied().collect(),
                formation: FormationType::Line,
                leader,
            });
            CommandStatus::Applied
        }
        OperationalCommand::RouteStack { stack, waypoints } => {
            let members: Vec<_> = state
                .stacks
                .iter()
                .find(|s| s.id == *stack)
                .map(|s| s.members.to_vec())
                .unwrap_or_default();
            for member_key in members {
                if let Some(entity) = state.entities.get_mut(member_key)
                    && let Some(mobile) = &mut entity.mobile
                {
                    mobile.waypoints = waypoints.clone();
                }
            }
            CommandStatus::Applied
        }
        OperationalCommand::DisbandStack { stack } => {
            state.stacks.retain(|existing| existing.id != *stack);
            CommandStatus::Applied
        }
        OperationalCommand::ProduceEquipment {
            workshop,
            item_type,
        } => {
            if economy::produce_equipment_now(state, *workshop, *item_type) {
                CommandStatus::Applied
            } else {
                CommandStatus::Rejected
            }
        }
        OperationalCommand::EstablishSupplyRoute { .. }
        | OperationalCommand::FoundSettlement { .. } => CommandStatus::Deferred,
        OperationalCommand::EquipEntity { entity, equipment } => {
            if equip_entity_item(state, *entity, *equipment) {
                CommandStatus::Applied
            } else {
                tracing::warn!(
                    "EquipEntity: unable to assign item {:?} to {:?}",
                    equipment,
                    entity
                );
                CommandStatus::Rejected
            }
        }
    }
}

pub fn apply_tactical_command(state: &mut GameState, cmd: &TacticalCommand) -> CommandStatus {
    if !validate_tactical_command(cmd, state) {
        return CommandStatus::Rejected;
    }

    match cmd {
        TacticalCommand::Attack { attacker, target } => {
            let Some(attacker_view) = state.entities.get(*attacker) else {
                return CommandStatus::Rejected;
            };
            let skill = attacker_view
                .person
                .as_ref()
                .map(|person| person.combat_skill)
                .unwrap_or(0.5);
            let attacker_pos = attacker_view.pos.unwrap_or(super::spatial::Vec3::ZERO);
            let Some(target_pos) = state.entities.get(*target).and_then(|e| e.pos) else {
                return CommandStatus::Rejected;
            };

            if let Some(entity) = state.entities.get_mut(*attacker)
                && let Some(combatant) = &mut entity.combatant
                && combatant.attack.is_none()
                && combatant.cooldown.is_none()
                && let Some(eq) = &entity.equipment
                && let Some(weapon_key) = eq.weapon
            {
                let motion = martial::select_attack_motion(
                    skill,
                    state.tick,
                    *attacker,
                    *target,
                    attacker_pos.z - target_pos.z,
                );
                combatant.attack = Some(AttackState::for_melee(*target, weapon_key, motion, skill));
            }
            CommandStatus::Applied
        }
        TacticalCommand::SetFacing { entity, angle } => {
            if let Some(e) = state.entities.get_mut(*entity)
                && let Some(combatant) = &mut e.combatant
            {
                combatant.facing = *angle;
            }
            CommandStatus::Applied
        }
        TacticalCommand::Retreat { entity, toward } => {
            if let Some(e) = state.entities.get_mut(*entity)
                && let Some(mobile) = &mut e.mobile
            {
                mobile.waypoints = vec![*toward];
            }
            CommandStatus::Applied
        }
        TacticalCommand::Hold { entity } => {
            if let Some(e) = state.entities.get_mut(*entity)
                && let Some(mobile) = &mut e.mobile
            {
                mobile.waypoints.clear();
            }
            CommandStatus::Applied
        }
        TacticalCommand::SetFormation { stack, formation } => {
            if let Some(s) = state.stacks.iter_mut().find(|s| s.id == *stack) {
                s.formation = *formation;
            }
            CommandStatus::Applied
        }
        TacticalCommand::Block { .. } => CommandStatus::Deferred,
    }
}

fn apply_entity_task(state: &mut GameState, entity_key: EntityKey, task: &EntityTask) {
    let assigned_task = economy::task_assignment_for(task);
    match task {
        EntityTask::Farm { .. } => {
            if let Some(person) = state
                .entities
                .get_mut(entity_key)
                .and_then(|e| e.person.as_mut())
            {
                person.role = Role::Farmer;
                person.task = Some(assigned_task.clone());
            }
        }
        EntityTask::Build { .. } | EntityTask::Craft { .. } => {
            if let Some(person) = state
                .entities
                .get_mut(entity_key)
                .and_then(|e| e.person.as_mut())
            {
                person.role = Role::Worker;
                person.task = Some(assigned_task.clone());
            }
        }
        EntityTask::Patrol { waypoints } => {
            if let Some(e) = state.entities.get_mut(entity_key) {
                if let Some(person) = e.person.as_mut() {
                    person.task = Some(assigned_task.clone());
                }
                if let Some(mobile) = &mut e.mobile {
                    mobile.waypoints = waypoints.clone();
                }
            }
        }
        EntityTask::Garrison { position } => {
            if let Some(e) = state.entities.get_mut(entity_key) {
                if let Some(person) = e.person.as_mut() {
                    person.task = Some(assigned_task.clone());
                }
                if let Some(mobile) = &mut e.mobile {
                    mobile.waypoints = vec![*position];
                }
            }
        }
        EntityTask::Train => {
            if let Some(e) = state.entities.get_mut(entity_key) {
                if let Some(person) = e.person.as_mut() {
                    person.role = Role::Soldier;
                    person.combat_skill = person.combat_skill.max(0.35);
                    person.task = Some(assigned_task.clone());
                }
                if e.mobile.is_none() {
                    e.mobile = Some(Mobile::new(2.0, 10.0));
                }
                if e.vitals.is_none() {
                    e.vitals = Some(Vitals::new());
                    e.wounds = Some(Default::default());
                }
                if e.combatant.is_none() {
                    e.combatant = Some(Combatant::new());
                }
                if e.equipment.is_none() {
                    e.equipment = Some(Equipment::empty());
                }
            }
            auto_equip_soldier(state, entity_key);
        }
        EntityTask::Idle => {
            if let Some(e) = state.entities.get_mut(entity_key) {
                if let Some(person) = e.person.as_mut() {
                    person.role = Role::Idle;
                    person.task = Some(TaskAssignment::Idle);
                }
                if let Some(mobile) = e.mobile.as_mut() {
                    mobile.waypoints.clear();
                }
            }
        }
    }
}

fn soldier_needs_item(state: &GameState, soldier_key: EntityKey, item_key: EntityKey) -> bool {
    let Some(soldier) = state.entities.get(soldier_key) else {
        return false;
    };
    let Some(item) = state.entities.get(item_key) else {
        return false;
    };
    let equipment = soldier.equipment.as_ref();

    if item.weapon_props.is_some() {
        let is_shield = item
            .weapon_props
            .as_ref()
            .map(|props| props.block_arc >= 1.2 && props.reach <= 1.0)
            .unwrap_or(false);
        if is_shield {
            return equipment.and_then(|eq| eq.shield).is_none();
        }
        return equipment.and_then(|eq| eq.weapon).is_none();
    }

    if let Some(armor_props) = item.armor_props.as_ref() {
        let eq = equipment.cloned().unwrap_or_else(Equipment::empty);
        return armor_props
            .zones_covered
            .iter()
            .any(|zone| eq.armor_slots[equipment::zone_index(*zone)].is_none());
    }

    false
}

fn equip_entity_item(state: &mut GameState, entity_key: EntityKey, item_key: EntityKey) -> bool {
    let Some(item) = state.entities.get(item_key) else {
        return false;
    };
    let weapon_props = item.weapon_props.clone();
    let armor_props = item.armor_props.clone();
    let is_shield = weapon_props
        .as_ref()
        .map(|props| props.block_arc >= 1.2 && props.reach <= 1.0)
        .unwrap_or(false);

    let Some(entity) = state.entities.get_mut(entity_key) else {
        return false;
    };
    let equipment = entity.equipment.get_or_insert_with(Equipment::empty);

    let assigned = if let Some(props) = armor_props.as_ref() {
        equipment::equip_armor(equipment, item_key, props);
        true
    } else if is_shield {
        if equipment.shield.is_none() {
            equipment.shield = Some(item_key);
            true
        } else {
            false
        }
    } else if equipment.weapon.is_none() {
        equipment.weapon = Some(item_key);
        true
    } else {
        false
    };

    if !assigned {
        return false;
    }

    uncontain(state, item_key);
    contain(state, entity_key, item_key);
    true
}

fn auto_equip_soldier(state: &mut GameState, soldier_key: EntityKey) {
    let owner = match state
        .entities
        .get(soldier_key)
        .and_then(|entity| entity.owner)
    {
        Some(owner) => owner,
        None => return,
    };

    let inventory: Vec<_> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            (entity.owner == Some(owner)
                && entity.person.is_none()
                && (entity.weapon_props.is_some() || entity.armor_props.is_some())
                && entity
                    .contained_in
                    .and_then(|container| state.entities.get(container))
                    .map(|container| container.site.is_some())
                    .unwrap_or(false))
            .then_some(key)
        })
        .collect();

    for item_key in inventory {
        if soldier_needs_item(state, soldier_key, item_key) {
            let _ = equip_entity_item(state, soldier_key, item_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::agent::{EquipmentType, StackArchetype};
    use super::super::armor;
    use super::super::lifecycle::{contain, spawn_entity};
    use super::super::physical::{PhysicalProperties, SiteProperties};
    use super::super::spatial::{GeoMaterial, Heightfield, Vec3};
    use super::super::state::{EntityBuilder, Person};
    use super::*;
    use crate::v2::hex::Axial;
    use simulate_everything_protocol::{MaterialKind, MatterState, PropertyTag};

    fn test_state() -> GameState {
        let hf = Heightfield::new(10, 10, 0.0, GeoMaterial::Soil);
        GameState::new(10, 10, 2, hf)
    }

    fn spawn_person(state: &mut GameState, pos: Vec3, owner: u8) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .person(super::super::state::Person {
                    role: Role::Idle,
                    combat_skill: 0.25,
                    task: None,
                }),
        )
    }

    fn spawn_weapon_item(state: &mut GameState, owner: u8, pos: Vec3) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .weapon_props(super::super::weapon::iron_sword()),
        )
    }

    fn spawn_armor_item(state: &mut GameState, owner: u8, pos: Vec3) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .armor_props(armor::leather_cuirass()),
        )
    }

    fn spawn_workshop_site(state: &mut GameState, owner: u8) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .owner(owner)
                .physical(
                    PhysicalProperties::new(900.0, 0.4, MaterialKind::Wood, MatterState::Solid)
                        .with_tags(&[
                            PropertyTag::Workshop,
                            PropertyTag::Structural,
                            PropertyTag::Container,
                        ]),
                )
                .site(SiteProperties {
                    build_progress: 1.0,
                    integrity: 100.0,
                    occupancy_capacity: 10,
                }),
        )
    }

    #[test]
    fn validate_operational_rejects_missing_entity() {
        use slotmap::KeyData;

        let state = test_state();
        let fake_key = EntityKey::from(KeyData::from_ffi(0xDEAD_BEEF_0000_0001));
        let cmd = OperationalCommand::AssignTask {
            entity: fake_key,
            task: EntityTask::Idle,
        };
        assert!(!validate_operational_command(&cmd, &state));
    }

    #[test]
    fn apply_route_stack_sets_member_waypoints() {
        let mut state = test_state();
        let person = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                    task: None,
                })
                .mobile(Mobile::new(2.0, 10.0)),
        );
        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members: smallvec::smallvec![person],
            formation: FormationType::Line,
            leader: person,
        });

        let waypoint = Vec3::new(30.0, 40.0, 0.0);
        let status = apply_operational_command(
            &mut state,
            &OperationalCommand::RouteStack {
                stack: stack_id,
                waypoints: vec![waypoint],
            },
        );

        assert_eq!(status, CommandStatus::Applied);
        assert_eq!(
            state.entities[person].mobile.as_ref().unwrap().waypoints,
            vec![waypoint]
        );
    }

    #[test]
    fn apply_train_initializes_combat_state_and_auto_equips() {
        let mut state = test_state();
        let soldier = spawn_person(&mut state, Vec3::new(10.0, 10.0, 0.0), 0);
        let depot = spawn_workshop_site(&mut state, 0);
        let sword = spawn_weapon_item(&mut state, 0, Vec3::new(12.0, 10.0, 0.0));
        let cuirass = spawn_armor_item(&mut state, 0, Vec3::new(12.0, 11.0, 0.0));
        contain(&mut state, depot, sword);
        contain(&mut state, depot, cuirass);

        let status = apply_operational_command(
            &mut state,
            &OperationalCommand::AssignTask {
                entity: soldier,
                task: EntityTask::Train,
            },
        );

        assert_eq!(status, CommandStatus::Applied);
        let entity = &state.entities[soldier];
        assert_eq!(entity.person.as_ref().unwrap().role, Role::Soldier);
        assert!(entity.mobile.is_some());
        assert!(entity.vitals.is_some());
        assert!(entity.combatant.is_some());
        assert_eq!(entity.equipment.as_ref().unwrap().weapon, Some(sword));
        assert_eq!(
            entity.equipment.as_ref().unwrap().armor_slots
                [equipment::zone_index(armor::BodyZone::Torso)],
            Some(cuirass)
        );
        assert_eq!(state.entities[sword].contained_in, Some(soldier));
    }

    #[test]
    fn assign_task_persists_task_state() {
        let mut state = test_state();
        let worker = spawn_person(&mut state, Vec3::new(10.0, 10.0, 0.0), 0);
        let workshop = spawn_workshop_site(&mut state, 0);

        let status = apply_operational_command(
            &mut state,
            &OperationalCommand::AssignTask {
                entity: worker,
                task: EntityTask::Build { site: workshop },
            },
        );

        assert_eq!(status, CommandStatus::Applied);
        let person = state.entities[worker].person.as_ref().unwrap();
        assert_eq!(person.role, Role::Worker);
        assert_eq!(
            person.task,
            Some(TaskAssignment::Workshop { site: workshop })
        );
    }

    #[test]
    fn apply_form_stack_and_disband_stack_updates_state() {
        let mut state = test_state();
        let a = spawn_person(&mut state, Vec3::new(10.0, 10.0, 0.0), 0);
        let b = spawn_person(&mut state, Vec3::new(12.0, 10.0, 0.0), 0);

        let status = apply_operational_command(
            &mut state,
            &OperationalCommand::FormStack {
                entities: vec![a, b],
                archetype: StackArchetype::HeavyInfantry,
            },
        );
        assert_eq!(status, CommandStatus::Applied);
        assert_eq!(state.stacks.len(), 1);
        let stack_id = state.stacks[0].id;

        let status = apply_operational_command(
            &mut state,
            &OperationalCommand::DisbandStack { stack: stack_id },
        );
        assert_eq!(status, CommandStatus::Applied);
        assert!(state.stacks.is_empty());
    }

    #[test]
    fn apply_equip_entity_assigns_item_and_contains_it() {
        let mut state = test_state();
        let soldier = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0)
                .equipment(Equipment::empty()),
        );
        let sword = spawn_weapon_item(&mut state, 0, Vec3::new(12.0, 10.0, 0.0));

        let status = apply_operational_command(
            &mut state,
            &OperationalCommand::EquipEntity {
                entity: soldier,
                equipment: sword,
            },
        );

        assert_eq!(status, CommandStatus::Applied);
        assert_eq!(
            state.entities[soldier].equipment.as_ref().unwrap().weapon,
            Some(sword)
        );
        assert_eq!(state.entities[sword].contained_in, Some(soldier));
    }

    #[test]
    fn apply_attack_creates_melee_attack_state() {
        let mut state = test_state();
        let attacker = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.6,
                    task: None,
                })
                .combatant(Combatant::new())
                .equipment(Equipment::empty()),
        );
        let target = spawn_person(&mut state, Vec3::new(11.0, 10.0, 0.0), 1);
        let sword = spawn_weapon_item(&mut state, 0, Vec3::new(9.0, 10.0, 0.0));
        let _ = equip_entity_item(&mut state, attacker, sword);

        let status =
            apply_tactical_command(&mut state, &TacticalCommand::Attack { attacker, target });

        assert_eq!(status, CommandStatus::Applied);
        let attack = state.entities[attacker]
            .combatant
            .as_ref()
            .unwrap()
            .attack
            .as_ref()
            .unwrap();
        assert_eq!(attack.target, target);
        assert_eq!(attack.weapon, sword);
    }

    #[test]
    fn apply_tactical_mutations_update_state() {
        let mut state = test_state();
        let entity = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0)
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new()),
        );
        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members: smallvec::smallvec![entity],
            formation: FormationType::Line,
            leader: entity,
        });

        assert_eq!(
            apply_tactical_command(
                &mut state,
                &TacticalCommand::SetFacing {
                    entity,
                    angle: 1.25,
                }
            ),
            CommandStatus::Applied
        );
        assert_eq!(
            state.entities[entity].combatant.as_ref().unwrap().facing,
            1.25
        );

        let retreat_to = Vec3::new(40.0, 50.0, 0.0);
        assert_eq!(
            apply_tactical_command(
                &mut state,
                &TacticalCommand::Retreat {
                    entity,
                    toward: retreat_to,
                }
            ),
            CommandStatus::Applied
        );
        assert_eq!(
            state.entities[entity].mobile.as_ref().unwrap().waypoints,
            vec![retreat_to]
        );

        assert_eq!(
            apply_tactical_command(&mut state, &TacticalCommand::Hold { entity }),
            CommandStatus::Applied
        );
        assert!(
            state.entities[entity]
                .mobile
                .as_ref()
                .unwrap()
                .waypoints
                .is_empty()
        );

        assert_eq!(
            apply_tactical_command(
                &mut state,
                &TacticalCommand::SetFormation {
                    stack: stack_id,
                    formation: FormationType::Column,
                }
            ),
            CommandStatus::Applied
        );
        assert_eq!(state.stacks[0].formation, FormationType::Column);
        assert_eq!(
            apply_tactical_command(&mut state, &TacticalCommand::Block { entity }),
            CommandStatus::Deferred
        );
    }

    #[test]
    fn apply_agent_output_counts_deferred_and_rejected_commands() {
        use slotmap::KeyData;

        let mut state = test_state();
        let soldier = spawn_person(&mut state, Vec3::new(10.0, 10.0, 0.0), 0);
        let fake_key = EntityKey::from(KeyData::from_ffi(0xDEAD_BEEF_0000_0001));
        let output = AgentOutput {
            player: 0,
            strategy_ran: false,
            operations_ran: false,
            tactical_stacks: 0,
            directives: Vec::new(),
            operational_commands: vec![
                OperationalCommand::AssignTask {
                    entity: soldier,
                    task: EntityTask::Idle,
                },
                OperationalCommand::ProduceEquipment {
                    workshop: soldier,
                    item_type: EquipmentType::Sword,
                },
                OperationalCommand::AssignTask {
                    entity: fake_key,
                    task: EntityTask::Idle,
                },
            ],
            tactical_commands: vec![TacticalCommand::Block { entity: soldier }],
            traces: Vec::new(),
        };

        let summary = apply_agent_output(&mut state, &output);
        assert_eq!(
            summary,
            CommandApplySummary {
                operational_applied: 1,
                operational_rejected: 2,
                operational_deferred: 0,
                tactical_applied: 0,
                tactical_rejected: 0,
                tactical_deferred: 1,
            }
        );
    }

    #[test]
    fn deferred_commands_validate_before_deferring() {
        let mut state = test_state();
        let entity = spawn_person(&mut state, Vec3::new(10.0, 10.0, 0.0), 0);
        assert_eq!(
            apply_operational_command(
                &mut state,
                &OperationalCommand::FoundSettlement {
                    entity,
                    target: Axial::new(1, 1),
                }
            ),
            CommandStatus::Deferred
        );
    }
}
