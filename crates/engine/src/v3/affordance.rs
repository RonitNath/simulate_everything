use crate::v2::state::EntityKey;
use simulate_everything_protocol::{CommodityKind, MaterialKind, PropertyTag};

use super::physical::PhysicalProperties;
use super::spatial::Vec3;
use super::state::GameState;

#[derive(Debug, Clone)]
pub enum AffordanceConstraint {
    Tags(Vec<PropertyTag>),
    SiteWithTags(Vec<PropertyTag>),
    Matter(CommodityKind),
    Tool,
    HeatSource,
    Material(MaterialKind),
}

pub fn find_affordance(
    state: &GameState,
    near: Vec3,
    radius: f32,
    constraint: &AffordanceConstraint,
    owner: Option<u8>,
) -> Option<EntityKey> {
    find_all_affordances(state, near, radius, constraint, owner)
        .into_iter()
        .min_by(|a, b| {
            distance_sq(state, *a, near)
                .partial_cmp(&distance_sq(state, *b, near))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

pub fn find_all_affordances(
    state: &GameState,
    near: Vec3,
    radius: f32,
    constraint: &AffordanceConstraint,
    owner: Option<u8>,
) -> Vec<EntityKey> {
    let radius_sq = radius * radius;
    state
        .fine_index
        .query_radius(near, radius)
        .into_iter()
        .filter(|key| {
            let Some(entity) = state.entities.get(*key) else {
                return false;
            };
            if owner.is_some() && entity.owner != owner {
                return false;
            }
            let Some(pos) = entity.pos else {
                return false;
            };
            let dx = pos.x - near.x;
            let dy = pos.y - near.y;
            let dz = pos.z - near.z;
            if dx * dx + dy * dy + dz * dz > radius_sq {
                return false;
            }
            matches_constraint(entity.physical.as_ref(), entity, constraint)
        })
        .collect()
}

fn matches_constraint(
    physical: Option<&PhysicalProperties>,
    entity: &super::state::Entity,
    constraint: &AffordanceConstraint,
) -> bool {
    match constraint {
        AffordanceConstraint::Tags(tags) => physical.map(|p| p.has_all_tags(tags)).unwrap_or(false),
        AffordanceConstraint::SiteWithTags(tags) => {
            entity.site.is_some() && physical.map(|p| p.has_all_tags(tags)).unwrap_or(false)
        }
        AffordanceConstraint::Matter(commodity) => entity
            .matter
            .as_ref()
            .map(|matter| matter.commodity == *commodity && matter.amount > 0.0)
            .unwrap_or(false),
        AffordanceConstraint::Tool => entity.tool_props.is_some(),
        AffordanceConstraint::HeatSource => physical
            .map(|p| p.has_tag(PropertyTag::HeatSource))
            .unwrap_or(false),
        AffordanceConstraint::Material(material) => {
            physical.map(|p| p.material == *material).unwrap_or(false)
        }
    }
}

fn distance_sq(state: &GameState, entity: EntityKey, near: Vec3) -> f32 {
    let pos = state
        .entities
        .get(entity)
        .and_then(|entity| entity.pos)
        .unwrap_or(Vec3::ZERO);
    let dx = pos.x - near.x;
    let dy = pos.y - near.y;
    let dz = pos.z - near.z;
    dx * dx + dy * dy + dz * dz
}
