use super::hex::world_to_hex;
use super::state::GameState;
use crate::v2::hex::Axial;
use crate::v2::state::EntityKey;

pub fn resolution_demand_at(state: &GameState, hex: Axial) -> f32 {
    let mut owners = std::collections::BTreeSet::new();
    let mut people = 0.0f32;
    let mut soldiers = 0.0f32;
    for &entity_key in state.spatial_index.entities_at(hex) {
        if let Some(entity) = state.entities.get(entity_key) {
            if entity.person.is_some() {
                people += 1.0;
                if entity.combatant.is_some() {
                    soldiers += 1.0;
                }
            }
            if let Some(owner) = entity.owner {
                owners.insert(owner);
            }
        }
    }
    if owners.len() <= 1 {
        return 0.0;
    }
    let conflict_intensity = (owners.len() as f32 - 1.0).max(0.0);
    let outcome_uncertainty = (people / 8.0).clamp(0.0, 1.0);
    let stakes = (soldiers / people.max(1.0)).clamp(0.2, 1.0);
    (conflict_intensity * outcome_uncertainty * stakes).clamp(0.0, 1.0)
}

pub fn enemy_nearby(state: &GameState, entity_key: EntityKey, radius: f32) -> bool {
    let Some(entity) = state.entities.get(entity_key) else {
        return false;
    };
    let Some(owner) = entity.owner else {
        return false;
    };
    let Some(pos) = entity.pos else {
        return false;
    };
    let center_hex = world_to_hex(pos);
    for hex in super::index::ring_hexes(center_hex, 3) {
        for &other_key in state.spatial_index.entities_at(hex) {
            if other_key == entity_key {
                continue;
            }
            let Some(other) = state.entities.get(other_key) else {
                continue;
            };
            if other.owner == Some(owner) || other.person.is_none() {
                continue;
            }
            let Some(other_pos) = other.pos else {
                continue;
            };
            if (other_pos.x - pos.x).powi(2) + (other_pos.y - pos.y).powi(2) <= radius * radius {
                return true;
            }
        }
    }
    false
}
