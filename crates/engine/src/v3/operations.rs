/// V3 shared operations layer: stack/equipment coordination and behavior availability.
///
/// All agent personalities share this implementation. Strategy speaks in
/// archetypes and priorities; operations translates to concrete entity tasks,
/// equipment loadouts, and stack management using the damage estimate table.
use super::agent::{
    EconomicFocus, EquipmentType, OperationalCommand, OperationsLayer, Posture, StackArchetype,
    StrategicDirective,
};
use super::armor::{ArmorConstruction, DamageType, MaterialType};
use super::damage_table::{DamageEstimateTable, MatchupKey};
use super::state::{GameState, Role, Stack};
use crate::v2::hex::Axial;
use crate::v2::state::EntityKey;
use simulate_everything_protocol::PropertyTag;

// ---------------------------------------------------------------------------
// Tunable constants
// ---------------------------------------------------------------------------

/// Minimum entities per stack.
const MIN_STACK_SIZE: usize = 3;

/// Maximum entities per stack.
const MAX_STACK_SIZE: usize = 32;

/// Target role ratios (farmer, worker, soldier) by economic focus.
fn role_weights(focus: EconomicFocus) -> (f32, f32, f32) {
    match focus {
        EconomicFocus::Growth => (0.60, 0.20, 0.20),
        EconomicFocus::Military => (0.25, 0.15, 0.60),
        EconomicFocus::Infrastructure => (0.35, 0.45, 0.20),
    }
}

// ---------------------------------------------------------------------------
// SharedOperationsLayer
// ---------------------------------------------------------------------------

/// Shared operations layer used by all agent personalities.
pub struct SharedOperationsLayer {
    posture: Posture,
    economic_focus: EconomicFocus,
    priority_regions: Vec<(Axial, f32)>,
    expansion_target: Option<Axial>,
    stack_requests: Vec<(StackArchetype, Axial)>,
    /// Per-agent damage estimate table for loadout decisions.
    pub damage_table: DamageEstimateTable,
}

impl Default for SharedOperationsLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedOperationsLayer {
    pub fn new() -> Self {
        Self {
            posture: Posture::Expand,
            economic_focus: EconomicFocus::Growth,
            priority_regions: Vec::new(),
            expansion_target: None,
            stack_requests: Vec::new(),
            damage_table: DamageEstimateTable::from_physics(),
        }
    }

    fn update_from_directives(&mut self, directives: &[StrategicDirective]) {
        self.stack_requests.clear();
        for d in directives {
            match d {
                StrategicDirective::SetPosture(p) => self.posture = *p,
                StrategicDirective::SetEconomicFocus(f) => self.economic_focus = *f,
                StrategicDirective::PrioritizeRegion { center, priority } => {
                    if let Some(entry) = self
                        .priority_regions
                        .iter_mut()
                        .find(|(c, _)| *c == *center)
                    {
                        entry.1 = *priority;
                    } else {
                        self.priority_regions.push((*center, *priority));
                    }
                }
                StrategicDirective::RequestStack { archetype, region } => {
                    self.stack_requests.push((*archetype, *region));
                }
                StrategicDirective::SetExpansionTarget { hex } => {
                    self.expansion_target = Some(*hex);
                }
            }
        }
        self.priority_regions.retain(|(_, p)| *p > 0.05);
    }

    /// Form stacks from available soldiers based on strategic requests.
    fn form_stacks(&self, state: &GameState, player: u8) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();

        // Collect available soldiers (not already in a stack).
        let stacked: std::collections::HashSet<EntityKey> = state
            .stacks
            .iter()
            .filter(|s| s.owner == player)
            .flat_map(|s| s.members.iter().copied())
            .collect();

        let mut available: Vec<EntityKey> = Vec::new();
        for (key, entity) in &state.entities {
            if entity.owner != Some(player) {
                continue;
            }
            if entity.person.as_ref().map(|p| p.role) != Some(Role::Soldier) {
                continue;
            }
            if stacked.contains(&key) {
                continue;
            }
            available.push(key);
        }

        // Form stacks for each request.
        for &(archetype, _region) in &self.stack_requests {
            if available.len() < MIN_STACK_SIZE {
                break;
            }

            let size = match archetype {
                StackArchetype::Settler => 2,
                StackArchetype::Skirmisher => MIN_STACK_SIZE,
                _ => MAX_STACK_SIZE.min(available.len()),
            };

            let members: Vec<EntityKey> = available.drain(..size.min(available.len())).collect();
            if !members.is_empty() {
                commands.push(OperationalCommand::FormStack {
                    entities: members,
                    archetype,
                });
            }
        }

        commands
    }

    /// Route existing stacks toward strategic objectives.
    fn route_stacks(&self, state: &GameState, player: u8) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();

        for stack in &state.stacks {
            if stack.owner != player {
                continue;
            }

            let destination = match self.posture {
                Posture::Attack => self
                    .priority_regions
                    .iter()
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(c, _)| *c),
                Posture::Expand => self.expansion_target,
                Posture::Defend | Posture::Consolidate => {
                    // Route toward nearest settlement.
                    find_nearest_settlement_hex(state, stack, player)
                }
            };

            if let Some(dest) = destination {
                let dest_world = super::hex::hex_to_world(dest);
                commands.push(OperationalCommand::RouteStack {
                    stack: stack.id,
                    waypoints: vec![dest_world],
                });
            }
        }

        commands
    }

    /// Translate a stack archetype into equipment types using the damage table.
    /// Returns equipment types to produce for the archetype given observed
    /// enemy armor types.
    pub fn archetype_loadout(
        &self,
        archetype: StackArchetype,
        enemy_armor: Option<(ArmorConstruction, MaterialType)>,
    ) -> Vec<EquipmentType> {
        match archetype {
            StackArchetype::HeavyInfantry => {
                let weapon = self.best_weapon_vs(enemy_armor);
                vec![
                    weapon,
                    EquipmentType::Shield,
                    EquipmentType::CuirassPlate,
                    EquipmentType::HelmetPlate,
                ]
            }
            StackArchetype::LightInfantry => {
                let weapon = self.best_weapon_vs(enemy_armor);
                vec![weapon, EquipmentType::Shield, EquipmentType::CuirassChain]
            }
            StackArchetype::Skirmisher => {
                vec![EquipmentType::Bow, EquipmentType::CuirassPadded]
            }
            StackArchetype::Cavalry => {
                vec![
                    EquipmentType::Spear,
                    EquipmentType::CuirassChain,
                    EquipmentType::HelmetChain,
                ]
            }
            StackArchetype::Garrison => {
                vec![
                    EquipmentType::Spear,
                    EquipmentType::Shield,
                    EquipmentType::CuirassPlate,
                ]
            }
            StackArchetype::Settler => {
                vec![] // settlers don't need military equipment
            }
        }
    }

    /// Select the best weapon type against observed enemy armor using the damage table.
    fn best_weapon_vs(
        &self,
        enemy_armor: Option<(ArmorConstruction, MaterialType)>,
    ) -> EquipmentType {
        let (ac, am) = enemy_armor.unwrap_or((ArmorConstruction::Padded, MaterialType::Leather));

        // Check each damage type's effectiveness against this armor.
        let candidates = [
            (DamageType::Slash, EquipmentType::Sword),
            (DamageType::Pierce, EquipmentType::Spear),
            (DamageType::Crush, EquipmentType::Mace),
        ];

        let mut best = EquipmentType::Sword;
        let mut best_rate = 0.0f32;

        for &(dt, eq) in &candidates {
            // Check with iron weapon material as reference.
            let key = MatchupKey {
                damage_type: dt,
                weapon_material: MaterialType::Iron,
                armor_construction: ac,
                armor_material: am,
            };
            if let Some(est) = self.damage_table.get(&key)
                && est.wound_rate > best_rate
            {
                best_rate = est.wound_rate;
                best = eq;
            }
        }

        best
    }

    /// Produce equipment at workshops based on stack archetype needs.
    fn produce_equipment(&self, state: &GameState, player: u8) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();

        // Find workshops owned by this player.
        let workshops: Vec<EntityKey> = state
            .entities
            .iter()
            .filter(|(_, e)| {
                e.owner == Some(player)
                    && e.site.is_some()
                    && e.physical
                        .as_ref()
                        .map(|p| p.has_tag(PropertyTag::Workshop))
                        .unwrap_or(false)
            })
            .map(|(k, _)| k)
            .collect();

        if workshops.is_empty() {
            return commands;
        }

        // For each pending stack request, determine needed equipment.
        let mut workshop_idx = 0;
        for &(archetype, _region) in &self.stack_requests {
            let loadout = self.archetype_loadout(archetype, None);
            for item_type in loadout {
                if workshop_idx >= workshops.len() {
                    break;
                }
                commands.push(OperationalCommand::ProduceEquipment {
                    workshop: workshops[workshop_idx % workshops.len()],
                    item_type,
                });
                workshop_idx += 1;
            }
        }

        commands
    }

    /// Accessors for testing.
    pub fn posture(&self) -> Posture {
        self.posture
    }
    pub fn economic_focus(&self) -> EconomicFocus {
        self.economic_focus
    }
}

impl OperationsLayer for SharedOperationsLayer {
    fn execute(
        &mut self,
        state: &GameState,
        directives: &[StrategicDirective],
        player: u8,
    ) -> Vec<OperationalCommand> {
        self.update_from_directives(directives);
        let mut commands = Vec::new();
        commands.extend(self.form_stacks(state, player));
        commands.extend(self.route_stacks(state, player));
        commands.extend(self.produce_equipment(state, player));
        if self.economic_focus == EconomicFocus::Infrastructure
            && let Some((center, _)) = self.priority_regions.first().copied()
        {
            commands.push(OperationalCommand::EstablishSupplyRoute {
                from: center,
                to: self.expansion_target.unwrap_or(center),
            });
        }
        commands
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn find_nearest_site(
    state: &GameState,
    entity_key: EntityKey,
    player: u8,
    required_tag: PropertyTag,
) -> Option<EntityKey> {
    let entity_pos = state.entities.get(entity_key)?.pos?;

    state
        .entities
        .iter()
        .filter(|(_, e)| {
            e.owner == Some(player)
                && e.site.is_some()
                && e.physical
                    .as_ref()
                    .map(|physical| physical.has_tag(required_tag))
                    .unwrap_or(false)
        })
        .filter_map(|(k, e)| {
            let pos = e.pos?;
            let dx = pos.x - entity_pos.x;
            let dy = pos.y - entity_pos.y;
            Some((k, dx * dx + dy * dy))
        })
        .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(k, _)| k)
}

fn find_nearest_settlement_hex(state: &GameState, stack: &Stack, player: u8) -> Option<Axial> {
    let leader_pos = state.entities.get(stack.leader)?.pos?;

    state
        .entities
        .iter()
        .filter(|(_, e)| {
            e.owner == Some(player)
                && e.site.is_some()
                && e.physical
                    .as_ref()
                    .map(|physical| physical.has_tag(PropertyTag::Settlement))
                    .unwrap_or(false)
        })
        .filter_map(|(_, e)| e.hex)
        .min_by(|a, b| {
            let a_world = super::hex::hex_to_world(*a);
            let b_world = super::hex::hex_to_world(*b);
            let da = (a_world.x - leader_pos.x).powi(2) + (a_world.y - leader_pos.y).powi(2);
            let db = (b_world.x - leader_pos.x).powi(2) + (b_world.y - leader_pos.y).powi(2);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
}

// ---------------------------------------------------------------------------
// Null operations — does nothing (for testing)
// ---------------------------------------------------------------------------

/// Operations layer that issues no commands. For testing baseline behavior.
pub struct NullOperationsLayer;

impl OperationsLayer for NullOperationsLayer {
    fn execute(
        &mut self,
        _state: &GameState,
        _directives: &[StrategicDirective],
        _player: u8,
    ) -> Vec<OperationalCommand> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::formation::FormationType;
    use super::super::lifecycle::spawn_entity;
    use super::super::movement::Mobile;
    use super::super::physical::{PhysicalProperties, SiteProperties};
    use super::super::spatial::{GeoMaterial, Heightfield, Vec3};
    use super::super::state::{Combatant, EntityBuilder, Person, StackId};
    use super::*;
    use simulate_everything_protocol::{MaterialKind, MatterState, PropertyTag};
    use smallvec::SmallVec;

    fn test_state() -> GameState {
        let hf = Heightfield::new(30, 30, 0.0, GeoMaterial::Soil);
        GameState::new(30, 30, 2, hf)
    }

    fn spawn_soldier(state: &mut GameState, pos: Vec3, owner: u8) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new()),
        )
    }

    fn spawn_idle(state: &mut GameState, pos: Vec3, owner: u8) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .person(Person {
                    role: Role::Idle,
                    combat_skill: 0.0,
                })
                .mobile(Mobile::new(2.0, 10.0)),
        )
    }

    fn spawn_farm(state: &mut GameState, pos: Vec3, owner: u8) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .physical(
                    PhysicalProperties::new(800.0, 0.35, MaterialKind::Wood, MatterState::Solid)
                        .with_tags(&[
                            PropertyTag::Farm,
                            PropertyTag::Structural,
                            PropertyTag::Container,
                            PropertyTag::Harvestable,
                        ]),
                )
                .site(SiteProperties {
                    build_progress: 1.0,
                    integrity: 100.0,
                    occupancy_capacity: 5,
                }),
        )
    }

    #[test]
    fn directives_update_state() {
        let mut ops = SharedOperationsLayer::new();
        ops.update_from_directives(&[
            StrategicDirective::SetPosture(Posture::Attack),
            StrategicDirective::SetEconomicFocus(EconomicFocus::Military),
        ]);
        assert_eq!(ops.posture(), Posture::Attack);
        assert_eq!(ops.economic_focus(), EconomicFocus::Military);
    }

    #[test]
    fn infrastructure_focus_injects_supply_route() {
        let mut state = test_state();
        let _idle1 = spawn_idle(&mut state, Vec3::new(50.0, 50.0, 0.0), 0);
        let _idle2 = spawn_idle(&mut state, Vec3::new(60.0, 50.0, 0.0), 0);
        let _farm = spawn_farm(&mut state, Vec3::new(55.0, 50.0, 0.0), 0);

        let mut ops = SharedOperationsLayer::new();
        let commands = ops.execute(
            &state,
            &[
                StrategicDirective::SetEconomicFocus(EconomicFocus::Infrastructure),
                StrategicDirective::PrioritizeRegion {
                    center: Axial::new(5, 5),
                    priority: 0.8,
                },
            ],
            0,
        );

        let supply_count = commands
            .iter()
            .filter(|c| matches!(c, OperationalCommand::EstablishSupplyRoute { .. }))
            .count();
        assert!(supply_count > 0, "should inject supply route opportunities");
    }

    #[test]
    fn form_stacks_from_soldiers() {
        let mut state = test_state();
        for i in 0..5 {
            spawn_soldier(&mut state, Vec3::new(50.0 + i as f32, 50.0, 0.0), 0);
        }

        let mut ops = SharedOperationsLayer::new();
        let commands = ops.execute(
            &state,
            &[
                StrategicDirective::SetPosture(Posture::Attack),
                StrategicDirective::RequestStack {
                    archetype: StackArchetype::HeavyInfantry,
                    region: Axial::new(5, 5),
                },
            ],
            0,
        );

        let form_count = commands
            .iter()
            .filter(|c| matches!(c, OperationalCommand::FormStack { .. }))
            .count();
        assert_eq!(
            form_count, 1,
            "should form one stack from available soldiers"
        );
    }

    #[test]
    fn archetype_loadout_heavy_infantry() {
        let ops = SharedOperationsLayer::new();
        let loadout = ops.archetype_loadout(StackArchetype::HeavyInfantry, None);
        assert!(
            loadout.len() >= 3,
            "heavy infantry needs weapon + shield + armor"
        );
        assert!(loadout.contains(&EquipmentType::Shield));
    }

    #[test]
    fn archetype_loadout_vs_plate_prefers_crush() {
        let ops = SharedOperationsLayer::new();
        let loadout = ops.archetype_loadout(
            StackArchetype::HeavyInfantry,
            Some((ArmorConstruction::Plate, MaterialType::Steel)),
        );
        // Against plate, crush should be preferred (mace).
        assert!(
            loadout.contains(&EquipmentType::Mace),
            "should prefer mace against plate armor: {:?}",
            loadout
        );
    }

    #[test]
    fn archetype_loadout_vs_padded_prefers_pierce() {
        let ops = SharedOperationsLayer::new();
        let loadout = ops.archetype_loadout(
            StackArchetype::HeavyInfantry,
            Some((ArmorConstruction::Padded, MaterialType::Leather)),
        );
        // Against padded, pierce should be preferred (spear).
        assert!(
            loadout.contains(&EquipmentType::Spear),
            "should prefer spear against padded armor: {:?}",
            loadout
        );
    }

    #[test]
    fn settler_archetype_no_equipment() {
        let ops = SharedOperationsLayer::new();
        let loadout = ops.archetype_loadout(StackArchetype::Settler, None);
        assert!(loadout.is_empty(), "settlers don't need military equipment");
    }

    #[test]
    fn route_stacks_attack_toward_priority() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(50.0, 50.0, 0.0), 0);

        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members: SmallVec::from_slice(&[s1]),
            formation: FormationType::Line,
            leader: s1,
        });

        let mut ops = SharedOperationsLayer::new();
        let commands = ops.execute(
            &state,
            &[
                StrategicDirective::SetPosture(Posture::Attack),
                StrategicDirective::PrioritizeRegion {
                    center: Axial::new(10, 10),
                    priority: 0.9,
                },
            ],
            0,
        );

        let route_count = commands
            .iter()
            .filter(|c| matches!(c, OperationalCommand::RouteStack { .. }))
            .count();
        assert_eq!(route_count, 1, "should route stack toward priority region");
    }

    #[test]
    fn full_pipeline_no_panic() {
        let mut state = test_state();
        for i in 0..10 {
            spawn_idle(&mut state, Vec3::new(50.0 + i as f32, 50.0, 0.0), 0);
        }
        spawn_farm(&mut state, Vec3::new(55.0, 50.0, 0.0), 0);

        let mut ops = SharedOperationsLayer::new();
        for posture in [
            Posture::Expand,
            Posture::Attack,
            Posture::Defend,
            Posture::Consolidate,
        ] {
            for focus in [
                EconomicFocus::Growth,
                EconomicFocus::Military,
                EconomicFocus::Infrastructure,
            ] {
                let directives = vec![
                    StrategicDirective::SetPosture(posture),
                    StrategicDirective::SetEconomicFocus(focus),
                ];
                let _ = ops.execute(&state, &directives, 0);
            }
        }
    }
}
