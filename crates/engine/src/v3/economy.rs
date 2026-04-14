use super::agent::{EntityTask, EquipmentType};
use super::armor::MaterialType;
use super::equipment::{self, Equipment};
use super::lifecycle::{contain, spawn_entity};
use super::physical::{MatterStack, PhysicalProperties, SiteProperties};
use super::spatial::Vec3;
use super::state::{CommodityKind, EntityBuilder, GameState, Role, TaskAssignment};
use super::weapon;
use crate::v2::state::EntityKey;
use simulate_everything_protocol::{MaterialKind, MatterState, PropertyTag};

const FARM_OUTPUT_PER_TASK: f32 = 1.0;
const WORKSHOP_MATERIAL_PER_TASK: f32 = 0.5;
const FOOD_UPKEEP_PER_PERSON: f32 = 0.15;
const STARVATION_STAMINA_DRAIN: f32 = 0.05;

fn site_tag_for_task(task: &TaskAssignment) -> PropertyTag {
    match task {
        TaskAssignment::Farm { .. } => PropertyTag::Farm,
        TaskAssignment::Workshop { .. } => PropertyTag::Workshop,
        TaskAssignment::Patrol
        | TaskAssignment::Garrison
        | TaskAssignment::Train
        | TaskAssignment::Idle => PropertyTag::Workshop,
    }
}

pub(crate) fn stockpile_physical(commodity: CommodityKind) -> PhysicalProperties {
    match commodity {
        CommodityKind::Food => {
            PhysicalProperties::new(50.0, 0.1, MaterialKind::Plant, MatterState::Solid)
                .with_tags(&[PropertyTag::Edible, PropertyTag::Stockpile])
        }
        CommodityKind::Material => {
            PhysicalProperties::new(80.0, 0.35, MaterialKind::Wood, MatterState::Solid)
                .with_tags(&[PropertyTag::Workable, PropertyTag::Stockpile])
        }
        CommodityKind::Ore => {
            PhysicalProperties::new(80.0, 0.6, MaterialKind::Stone, MatterState::Powder)
                .with_tags(&[PropertyTag::Workable, PropertyTag::Stockpile])
        }
        CommodityKind::Wood => {
            PhysicalProperties::new(80.0, 0.3, MaterialKind::Wood, MatterState::Solid)
                .with_tags(&[PropertyTag::Workable, PropertyTag::Stockpile])
        }
        CommodityKind::Stone => {
            PhysicalProperties::new(80.0, 0.8, MaterialKind::Stone, MatterState::Solid)
                .with_tags(&[PropertyTag::Workable, PropertyTag::Stockpile])
        }
    }
}

pub(crate) fn site_physical(tag: PropertyTag) -> PhysicalProperties {
    let mut physical = PhysicalProperties::new(900.0, 0.4, MaterialKind::Wood, MatterState::Solid)
        .with_tags(&[PropertyTag::Structural, PropertyTag::Container]);
    physical.insert_tag(tag);
    if matches!(tag, PropertyTag::Farm) {
        physical.insert_tag(PropertyTag::Harvestable);
    } else {
        physical.insert_tag(PropertyTag::Shelter);
    }
    if matches!(tag, PropertyTag::Workshop) {
        physical.insert_tag(PropertyTag::Tool);
    }
    physical
}

pub fn tick_economy(state: &mut GameState, dt: f32) {
    produce_resources(state, dt);
    consume_food(state, dt);
}

pub fn produce_resources(state: &mut GameState, dt: f32) {
    let mut food_deltas = vec![0.0f32; state.num_players as usize];
    let mut material_deltas = vec![0.0f32; state.num_players as usize];

    for entity in state.entities.values() {
        let Some(owner) = entity.owner else {
            continue;
        };
        let Some(person) = entity.person.as_ref() else {
            continue;
        };
        let Some(task) = person.task.as_ref() else {
            continue;
        };
        if entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false) {
            continue;
        }

        match task {
            TaskAssignment::Farm { site } => {
                if valid_site(state, *site, owner, PropertyTag::Farm) {
                    food_deltas[owner as usize] += FARM_OUTPUT_PER_TASK * dt;
                }
            }
            TaskAssignment::Workshop { site } => {
                if valid_site(state, *site, owner, PropertyTag::Workshop) {
                    material_deltas[owner as usize] += WORKSHOP_MATERIAL_PER_TASK * dt;
                }
            }
            TaskAssignment::Patrol
            | TaskAssignment::Garrison
            | TaskAssignment::Train
            | TaskAssignment::Idle => {}
        }
    }

    for owner in 0..state.num_players {
        let food = food_deltas[owner as usize];
        if food > 0.0 {
            add_player_stockpile(state, owner, CommodityKind::Food, food);
        }
        let material = material_deltas[owner as usize];
        if material > 0.0 {
            add_player_stockpile(state, owner, CommodityKind::Material, material);
        }
    }
}

pub fn consume_food(state: &mut GameState, dt: f32) {
    let mut required = vec![0.0f32; state.num_players as usize];

    for entity in state.entities.values() {
        let Some(owner) = entity.owner else {
            continue;
        };
        if entity.person.is_none() {
            continue;
        }
        if entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false) {
            continue;
        }
        required[owner as usize] += FOOD_UPKEEP_PER_PERSON * dt;
    }

    let mut shortages = vec![0.0f32; state.num_players as usize];
    for owner in 0..state.num_players {
        let need = required[owner as usize];
        if need <= 0.0 {
            continue;
        }
        let available = player_stockpile_amount(state, owner, CommodityKind::Food);
        let consumed = available.min(need);
        if consumed > 0.0 {
            consume_player_stockpile(state, owner, CommodityKind::Food, consumed);
        }
        shortages[owner as usize] = (need - consumed).max(0.0);
    }

    if shortages.iter().all(|shortage| *shortage <= 0.0) {
        return;
    }

    let mut starving_counts = vec![0u32; state.num_players as usize];
    for entity in state.entities.values() {
        let Some(owner) = entity.owner else {
            continue;
        };
        if entity.person.is_none() {
            continue;
        }
        if entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false) {
            continue;
        }
        if shortages[owner as usize] > 0.0 {
            starving_counts[owner as usize] += 1;
        }
    }

    for entity in state.entities.values_mut() {
        let Some(owner) = entity.owner else {
            continue;
        };
        let shortage = shortages[owner as usize];
        if shortage <= 0.0 {
            continue;
        }
        let Some(_person) = entity.person.as_ref() else {
            continue;
        };
        let Some(vitals) = entity.vitals.as_mut() else {
            continue;
        };
        if vitals.is_dead() {
            continue;
        }

        let count = starving_counts[owner as usize].max(1) as f32;
        let share = (shortage / count).max(0.0);
        vitals.drain_stamina(STARVATION_STAMINA_DRAIN * share / FOOD_UPKEEP_PER_PERSON.max(0.001));
    }
}

pub fn produce_equipment_now(
    state: &mut GameState,
    workshop: EntityKey,
    item_type: EquipmentType,
) -> bool {
    let Some(owner) = state.entities.get(workshop).and_then(|entity| entity.owner) else {
        return false;
    };
    if !valid_site(state, workshop, owner, PropertyTag::Workshop) {
        return false;
    }

    let cost = item_cost(item_type);
    if player_stockpile_amount(state, owner, CommodityKind::Material) < cost {
        return false;
    }

    let Some(workshop_pos) = state.entities.get(workshop).and_then(|entity| entity.pos) else {
        return false;
    };

    let builder = if let Some(weapon_props) = item_weapon(item_type) {
        EntityBuilder::new()
            .pos(workshop_pos)
            .owner(owner)
            .weapon_props(weapon_props)
    } else if let Some(armor_props) = item_armor(item_type) {
        EntityBuilder::new()
            .pos(workshop_pos)
            .owner(owner)
            .armor_props(armor_props)
    } else {
        return false;
    };

    consume_player_stockpile(state, owner, CommodityKind::Material, cost);
    let item_key = spawn_entity(state, builder);
    contain(state, workshop, item_key);
    auto_assign_to_soldier(state, owner, item_key);
    true
}

pub fn generate_economy_ready(
    width: usize,
    height: usize,
    num_players: u8,
    seed: u64,
) -> GameState {
    let mut state = super::mapgen::generate(width, height, num_players, seed);
    bootstrap_shared_economy_layout(&mut state);
    state
}

pub fn bootstrap_shared_economy_layout(state: &mut GameState) {
    let villages: Vec<(u8, Vec3)> = state
        .entities
        .values()
        .filter_map(|entity| {
            let owner = entity.owner?;
            let pos = entity.pos?;
            let physical = entity.physical.as_ref()?;
            let site = entity.site.as_ref()?;
            (site.build_progress >= 1.0 && physical.has_tag(PropertyTag::Settlement))
                .then_some((owner, pos))
        })
        .collect();

    for (owner, center) in villages {
        ensure_site(
            state,
            owner,
            PropertyTag::Farm,
            Vec3::new(center.x + 25.0, center.y, center.z),
            8,
            80.0,
        );
        ensure_site(
            state,
            owner,
            PropertyTag::Workshop,
            Vec3::new(center.x - 25.0, center.y, center.z),
            24,
            90.0,
        );
        let _ = ensure_stockpile_resource(state, owner, CommodityKind::Food);
        let _ = ensure_stockpile_resource(state, owner, CommodityKind::Material);
    }
}

pub fn task_assignment_for(task: &EntityTask) -> TaskAssignment {
    match task {
        EntityTask::Farm { field } => TaskAssignment::Farm { site: *field },
        EntityTask::Build { site } | EntityTask::Craft { workshop: site, .. } => {
            TaskAssignment::Workshop { site: *site }
        }
        EntityTask::Patrol { .. } => TaskAssignment::Patrol,
        EntityTask::Garrison { .. } => TaskAssignment::Garrison,
        EntityTask::Train => TaskAssignment::Train,
        EntityTask::Idle => TaskAssignment::Idle,
    }
}

pub fn player_stockpile_amount(state: &GameState, owner: u8, commodity: CommodityKind) -> f32 {
    state
        .entities
        .values()
        .filter(|entity| entity.owner == Some(owner))
        .filter_map(|entity| entity.matter.as_ref())
        .filter(|matter| matter.commodity == commodity)
        .map(|matter| matter.amount)
        .sum()
}

fn add_player_stockpile(state: &mut GameState, owner: u8, commodity: CommodityKind, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    if let Some(key) = ensure_stockpile_resource(state, owner, commodity)
        && let Some(resource) = state
            .entities
            .get_mut(key)
            .and_then(|entity| entity.matter.as_mut())
    {
        resource.amount += amount;
    }
}

fn consume_player_stockpile(
    state: &mut GameState,
    owner: u8,
    commodity: CommodityKind,
    mut amount: f32,
) {
    if amount <= 0.0 {
        return;
    }

    let mut resources: Vec<(EntityKey, u32)> = state
        .entities
        .iter()
        .filter(|(_, entity)| entity.owner == Some(owner))
        .filter_map(|(key, entity)| {
            let resource = entity.matter.as_ref()?;
            (resource.commodity == commodity).then_some((key, entity.id))
        })
        .collect();
    resources.sort_by_key(|(_, id)| *id);

    for (key, _) in resources {
        if amount <= 0.0 {
            break;
        }
        if let Some(resource) = state
            .entities
            .get_mut(key)
            .and_then(|entity| entity.matter.as_mut())
        {
            let used = resource.amount.min(amount);
            resource.amount -= used;
            amount -= used;
        }
    }
}

fn ensure_stockpile_resource(
    state: &mut GameState,
    owner: u8,
    commodity: CommodityKind,
) -> Option<EntityKey> {
    let container = canonical_stockpile_container(state, owner)?;

    if let Some(existing) = state.entities.get(container).and_then(|entity| {
        entity.contains.iter().copied().find(|child| {
            state
                .entities
                .get(*child)
                .and_then(|entity| entity.matter.as_ref())
                .map(|resource| resource.commodity == commodity)
                .unwrap_or(false)
        })
    }) {
        return Some(existing);
    }

    let resource = spawn_entity(
        state,
        EntityBuilder::new()
            .owner(owner)
            .physical(stockpile_physical(commodity))
            .matter(MatterStack {
                commodity,
                amount: 0.0,
            }),
    );
    contain(state, container, resource);
    Some(resource)
}

fn canonical_stockpile_container(state: &GameState, owner: u8) -> Option<EntityKey> {
    state
        .entities
        .iter()
        .filter(|(_, entity)| entity.owner == Some(owner))
        .filter_map(|(key, entity)| {
            let site = entity.site.as_ref()?;
            let physical = entity.physical.as_ref()?;
            let rank = if physical.has_tag(PropertyTag::Settlement) {
                0
            } else {
                1
            };
            Some((key, rank, site.build_progress, entity.id))
        })
        .min_by(|a, b| {
            let left = (a.1, -((a.2 * 1000.0) as i32), a.3);
            let right = (b.1, -((b.2 * 1000.0) as i32), b.3);
            left.cmp(&right)
        })
        .map(|(key, _, _, _)| key)
}

fn valid_site(state: &GameState, site: EntityKey, owner: u8, expected: PropertyTag) -> bool {
    state
        .entities
        .get(site)
        .map(|entity| {
            entity.owner == Some(owner)
                && entity
                    .site
                    .as_ref()
                    .map(|site| site.build_progress >= 1.0)
                    .unwrap_or(false)
                && entity
                    .physical
                    .as_ref()
                    .map(|physical| physical.has_tag(expected))
                    .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn ensure_site(
    state: &mut GameState,
    owner: u8,
    site_tag: PropertyTag,
    pos: Vec3,
    capacity: usize,
    integrity: f32,
) {
    let has_structure = state.entities.values().any(|entity| {
        entity.owner == Some(owner)
            && entity.site.is_some()
            && entity
                .physical
                .as_ref()
                .map(|physical| physical.has_tag(site_tag))
                .unwrap_or(false)
    });
    if has_structure {
        return;
    }

    spawn_entity(
        state,
        EntityBuilder::new()
            .pos(pos)
            .owner(owner)
            .physical(site_physical(site_tag))
            .site(SiteProperties {
                build_progress: 1.0,
                integrity,
                occupancy_capacity: capacity,
            }),
    );
}

fn item_weapon(item_type: EquipmentType) -> Option<super::weapon::WeaponProperties> {
    match item_type {
        EquipmentType::Sword => Some(weapon::iron_sword()),
        EquipmentType::Spear => Some(super::weapon::WeaponProperties {
            material: MaterialType::Iron,
            damage_type: super::armor::DamageType::Pierce,
            sharpness: 0.7,
            hardness: 5.0,
            weight: 1.8,
            reach: 2.2,
            hands_required: 2,
            block_arc: 0.3,
            block_efficiency: 0.5,
            projectile_speed: 0.0,
            projectile_arc: false,
            accuracy_base: 0.0,
            windup_ticks: 5,
            commitment_fraction: 0.6,
            base_recovery: 4.0,
        }),
        EquipmentType::Axe => Some(super::weapon::WeaponProperties {
            material: MaterialType::Iron,
            damage_type: super::armor::DamageType::Slash,
            sharpness: 0.75,
            hardness: 5.0,
            weight: 1.6,
            reach: 1.4,
            hands_required: 1,
            block_arc: 0.35,
            block_efficiency: 0.55,
            projectile_speed: 0.0,
            projectile_arc: false,
            accuracy_base: 0.0,
            windup_ticks: 4,
            commitment_fraction: 0.5,
            base_recovery: 4.0,
        }),
        EquipmentType::Mace => Some(super::weapon::WeaponProperties {
            material: MaterialType::Iron,
            damage_type: super::armor::DamageType::Crush,
            sharpness: 0.1,
            hardness: 6.0,
            weight: 2.0,
            reach: 1.3,
            hands_required: 1,
            block_arc: 0.35,
            block_efficiency: 0.5,
            projectile_speed: 0.0,
            projectile_arc: false,
            accuracy_base: 0.0,
            windup_ticks: 5,
            commitment_fraction: 0.5,
            base_recovery: 4.0,
        }),
        EquipmentType::Bow => Some(weapon::wooden_bow()),
        EquipmentType::Shield => Some(super::weapon::WeaponProperties {
            material: MaterialType::Wood,
            damage_type: super::armor::DamageType::Crush,
            sharpness: 0.0,
            hardness: 3.0,
            weight: 3.0,
            reach: 0.7,
            hands_required: 1,
            block_arc: 1.8,
            block_efficiency: 0.2,
            projectile_speed: 0.0,
            projectile_arc: false,
            accuracy_base: 0.0,
            windup_ticks: 3,
            commitment_fraction: 0.4,
            base_recovery: 2.0,
        }),
        EquipmentType::HelmetPlate
        | EquipmentType::HelmetChain
        | EquipmentType::CuirassPlate
        | EquipmentType::CuirassChain
        | EquipmentType::CuirassPadded
        | EquipmentType::Greaves => None,
    }
}

fn item_armor(item_type: EquipmentType) -> Option<super::armor::ArmorProperties> {
    match item_type {
        EquipmentType::HelmetPlate => Some(super::armor::ArmorProperties {
            material: MaterialType::Iron,
            construction: super::armor::ArmorConstruction::Plate,
            hardness: 6.5,
            thickness: 2.0,
            coverage: 0.9,
            weight: 2.5,
            zones_covered: vec![super::armor::BodyZone::Head],
        }),
        EquipmentType::HelmetChain => Some(super::armor::ArmorProperties {
            material: MaterialType::Iron,
            construction: super::armor::ArmorConstruction::Chain,
            hardness: 5.5,
            thickness: 1.5,
            coverage: 0.85,
            weight: 2.0,
            zones_covered: vec![super::armor::BodyZone::Head],
        }),
        EquipmentType::CuirassPlate => Some(super::armor::ArmorProperties {
            material: MaterialType::Iron,
            construction: super::armor::ArmorConstruction::Plate,
            hardness: 6.5,
            thickness: 2.2,
            coverage: 0.9,
            weight: 8.0,
            zones_covered: vec![super::armor::BodyZone::Torso],
        }),
        EquipmentType::CuirassChain => Some(super::armor::ArmorProperties {
            material: MaterialType::Iron,
            construction: super::armor::ArmorConstruction::Chain,
            hardness: 5.5,
            thickness: 1.8,
            coverage: 0.85,
            weight: 6.5,
            zones_covered: vec![
                super::armor::BodyZone::Torso,
                super::armor::BodyZone::LeftArm,
                super::armor::BodyZone::RightArm,
            ],
        }),
        EquipmentType::CuirassPadded => Some(super::armor::ArmorProperties {
            material: MaterialType::Cloth,
            construction: super::armor::ArmorConstruction::Padded,
            hardness: 2.5,
            thickness: 5.0,
            coverage: 0.8,
            weight: 3.0,
            zones_covered: vec![super::armor::BodyZone::Torso],
        }),
        EquipmentType::Greaves => Some(super::armor::ArmorProperties {
            material: MaterialType::Iron,
            construction: super::armor::ArmorConstruction::Plate,
            hardness: 5.5,
            thickness: 1.8,
            coverage: 0.8,
            weight: 3.5,
            zones_covered: vec![super::armor::BodyZone::Legs],
        }),
        EquipmentType::Sword
        | EquipmentType::Spear
        | EquipmentType::Axe
        | EquipmentType::Mace
        | EquipmentType::Bow
        | EquipmentType::Shield => None,
    }
}

fn item_cost(item_type: EquipmentType) -> f32 {
    match item_type {
        EquipmentType::Sword => 8.0,
        EquipmentType::Spear => 7.0,
        EquipmentType::Axe => 7.0,
        EquipmentType::Mace => 9.0,
        EquipmentType::Bow => 6.0,
        EquipmentType::Shield => 5.0,
        EquipmentType::HelmetPlate => 6.0,
        EquipmentType::HelmetChain => 5.0,
        EquipmentType::CuirassPlate => 14.0,
        EquipmentType::CuirassChain => 11.0,
        EquipmentType::CuirassPadded => 4.0,
        EquipmentType::Greaves => 8.0,
    }
}

fn auto_assign_to_soldier(state: &mut GameState, owner: u8, item_key: EntityKey) {
    let candidates: Vec<_> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            (entity.owner == Some(owner)
                && entity
                    .person
                    .as_ref()
                    .map(|person| person.role == Role::Soldier)
                    .unwrap_or(false))
            .then_some(key)
        })
        .collect();

    for soldier_key in candidates {
        if equip_entity_item(state, soldier_key, item_key) {
            break;
        }
    }
}

fn equip_entity_item(state: &mut GameState, entity_key: EntityKey, item_key: EntityKey) -> bool {
    if !soldier_needs_item(state, entity_key, item_key) {
        return false;
    }

    let Some(item) = state.entities.get(item_key) else {
        return false;
    };
    let weapon_props = item.weapon_props.clone();
    let armor_props = item.armor_props.clone();
    let is_shield = weapon_props
        .as_ref()
        .map(|props| props.block_arc >= 1.2 && props.reach <= 1.0)
        .unwrap_or(false);

    let existing_weapon = state
        .entities
        .get(entity_key)
        .and_then(|entity| entity.equipment.as_ref())
        .and_then(|equipment| equipment.weapon);
    let blocks_two_handed = existing_weapon
        .and_then(|weapon_key| state.entities.get(weapon_key))
        .and_then(|weapon| weapon.weapon_props.as_ref())
        .map(|weapon| weapon.hands_required >= 2)
        .unwrap_or(false);

    let Some(entity) = state.entities.get_mut(entity_key) else {
        return false;
    };
    let equipment = entity.equipment.get_or_insert_with(Equipment::empty);

    let assigned = if let Some(props) = armor_props.as_ref() {
        equipment::equip_armor(equipment, item_key, props);
        true
    } else if is_shield {
        if equipment.shield.is_none() && !blocks_two_handed {
            equipment.shield = Some(item_key);
            true
        } else {
            false
        }
    } else if let Some(props) = weapon_props.as_ref() {
        if equipment.weapon.is_none() && !(props.hands_required >= 2 && equipment.shield.is_some())
        {
            equipment.weapon = Some(item_key);
            true
        } else {
            false
        }
    } else {
        false
    };

    if assigned {
        contain(state, entity_key, item_key);
    }

    assigned
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

#[cfg(test)]
mod tests {
    use super::super::movement::Mobile;
    use super::super::physical::{MatterStack, SiteProperties};
    use super::super::spatial::{GeoMaterial, Heightfield};
    use super::super::state::{Combatant, CommodityKind, GameState, Person};
    use super::*;
    use simulate_everything_protocol::PropertyTag;

    fn test_state() -> GameState {
        let hf = Heightfield::new(12, 12, 0.0, GeoMaterial::Soil);
        GameState::new(12, 12, 2, hf)
    }

    fn spawn_person(
        state: &mut GameState,
        owner: u8,
        role: Role,
        task: Option<TaskAssignment>,
        pos: Vec3,
    ) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .person(Person {
                    role,
                    combat_skill: 0.2,
                    task,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .vitals(),
        )
    }

    fn spawn_site(state: &mut GameState, owner: u8, site_tag: PropertyTag, pos: Vec3) -> EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .physical(site_physical(site_tag))
                .site(SiteProperties {
                    build_progress: 1.0,
                    integrity: 100.0,
                    occupancy_capacity: 12,
                }),
        )
    }

    #[test]
    fn stockpile_helpers_create_and_reuse_resources() {
        let mut state = test_state();
        let village = spawn_site(&mut state, 0, PropertyTag::Settlement, Vec3::ZERO);
        let food = ensure_stockpile_resource(&mut state, 0, CommodityKind::Food).unwrap();
        let same_food = ensure_stockpile_resource(&mut state, 0, CommodityKind::Food).unwrap();

        assert_eq!(food, same_food);
        assert_eq!(state.entities[food].contained_in, Some(village));
    }

    #[test]
    fn farmer_task_produces_food() {
        let mut state = test_state();
        let village = spawn_site(&mut state, 0, PropertyTag::Settlement, Vec3::ZERO);
        let farm = spawn_site(&mut state, 0, PropertyTag::Farm, Vec3::new(10.0, 0.0, 0.0));
        let _farmer = spawn_person(
            &mut state,
            0,
            Role::Farmer,
            Some(TaskAssignment::Farm { site: farm }),
            Vec3::new(8.0, 0.0, 0.0),
        );

        let food = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .owner(0)
                .physical(stockpile_physical(CommodityKind::Food))
                .matter(MatterStack {
                    commodity: CommodityKind::Food,
                    amount: 0.0,
                }),
        );
        contain(&mut state, village, food);

        produce_resources(&mut state, 2.0);

        assert!(player_stockpile_amount(&state, 0, CommodityKind::Food) >= 2.0);
    }

    #[test]
    fn workshop_task_produces_material() {
        let mut state = test_state();
        let _village = spawn_site(&mut state, 0, PropertyTag::Settlement, Vec3::ZERO);
        let workshop = spawn_site(
            &mut state,
            0,
            PropertyTag::Workshop,
            Vec3::new(10.0, 0.0, 0.0),
        );
        let _worker = spawn_person(
            &mut state,
            0,
            Role::Worker,
            Some(TaskAssignment::Workshop { site: workshop }),
            Vec3::new(8.0, 0.0, 0.0),
        );

        produce_resources(&mut state, 2.0);

        assert!(player_stockpile_amount(&state, 0, CommodityKind::Material) >= 1.0);
    }

    #[test]
    fn food_consumption_drains_stockpile() {
        let mut state = test_state();
        let village = spawn_site(&mut state, 0, PropertyTag::Settlement, Vec3::ZERO);
        let food = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .owner(0)
                .physical(stockpile_physical(CommodityKind::Food))
                .matter(MatterStack {
                    commodity: CommodityKind::Food,
                    amount: 10.0,
                }),
        );
        contain(&mut state, village, food);
        let _person = spawn_person(
            &mut state,
            0,
            Role::Idle,
            Some(TaskAssignment::Idle),
            Vec3::ZERO,
        );

        consume_food(&mut state, 2.0);

        assert!(player_stockpile_amount(&state, 0, CommodityKind::Food) < 10.0);
    }

    #[test]
    fn food_shortage_drains_stamina_not_blood() {
        let mut state = test_state();
        let village = spawn_site(&mut state, 0, PropertyTag::Settlement, Vec3::ZERO);
        let _food = ensure_stockpile_resource(&mut state, 0, CommodityKind::Food).unwrap();
        let person = spawn_person(
            &mut state,
            0,
            Role::Idle,
            Some(TaskAssignment::Idle),
            Vec3::ZERO,
        );
        contain(&mut state, village, person);
        let blood_before = state.entities[person].vitals.as_ref().unwrap().blood;

        consume_food(&mut state, 2.0);

        let vitals = state.entities[person].vitals.as_ref().unwrap();
        assert!(vitals.stamina < 1.0);
        assert_eq!(vitals.blood, blood_before);
    }

    #[test]
    fn produce_equipment_spends_material_and_equips_soldier() {
        let mut state = test_state();
        let village = spawn_site(&mut state, 0, PropertyTag::Settlement, Vec3::ZERO);
        let workshop = spawn_site(
            &mut state,
            0,
            PropertyTag::Workshop,
            Vec3::new(5.0, 0.0, 0.0),
        );
        let material = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .owner(0)
                .physical(stockpile_physical(CommodityKind::Material))
                .matter(MatterStack {
                    commodity: CommodityKind::Material,
                    amount: 20.0,
                }),
        );
        contain(&mut state, village, material);
        let soldier = spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(2.0, 0.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.4,
                    task: Some(TaskAssignment::Train),
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals()
                .equipment(Equipment::empty()),
        );
        assert!(produce_equipment_now(
            &mut state,
            workshop,
            EquipmentType::Sword
        ));
        assert!(player_stockpile_amount(&state, 0, CommodityKind::Material) < 20.0);
        assert!(
            state.entities[soldier]
                .equipment
                .as_ref()
                .unwrap()
                .weapon
                .is_some()
        );
    }

    #[test]
    fn produce_equipment_fails_without_material() {
        let mut state = test_state();
        let _village = spawn_site(&mut state, 0, PropertyTag::Settlement, Vec3::ZERO);
        let workshop = spawn_site(
            &mut state,
            0,
            PropertyTag::Workshop,
            Vec3::new(5.0, 0.0, 0.0),
        );

        assert!(!produce_equipment_now(
            &mut state,
            workshop,
            EquipmentType::Sword
        ));
    }

    #[test]
    fn generate_economy_ready_bootstraps_support_structures() {
        let state = generate_economy_ready(20, 20, 2, 42);
        let farms = state
            .entities
            .values()
            .filter(|entity| {
                entity
                    .physical
                    .as_ref()
                    .map(|p| p.has_tag(PropertyTag::Farm))
                    .unwrap_or(false)
            })
            .count();
        let workshops = state
            .entities
            .values()
            .filter(|entity| {
                entity
                    .physical
                    .as_ref()
                    .map(|p| p.has_tag(PropertyTag::Workshop))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(farms, 2);
        assert_eq!(workshops, 2);
    }
}
