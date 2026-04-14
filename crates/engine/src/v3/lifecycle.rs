use smallvec::SmallVec;

use super::hex::world_to_hex;
use super::state::{Entity, EntityBuilder, GameState};
use crate::v2::state::EntityKey;

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn an entity from a builder. Returns the new entity's key.
///
/// - Assigns a monotonic public ID.
/// - Inserts into the SlotMap.
/// - Computes hex membership from position.
/// - Updates the spatial index.
pub fn spawn_entity(state: &mut GameState, builder: EntityBuilder) -> EntityKey {
    let id = state.alloc_id();
    let mut entity = builder.build(id);

    // Compute hex from position
    if let Some(pos) = entity.pos {
        let hex = world_to_hex(pos);
        entity.hex = Some(hex);
    }

    let key = state.entities.insert(entity);

    // Update spatial index
    if let Some(hex) = state.entities[key].hex {
        state.spatial_index.insert(hex, key);
    }

    key
}

// ---------------------------------------------------------------------------
// Containment
// ---------------------------------------------------------------------------

/// Place `contained` inside `container`. Bidirectional: sets contained_in
/// on the child and adds to contains on the parent.
///
/// The contained entity loses its position and hex membership (it's "inside"
/// the container now). It's removed from the spatial index.
pub fn contain(state: &mut GameState, container: EntityKey, contained: EntityKey) {
    // Remove contained entity from spatial index
    if let Some(hex) = state.entities.get(contained).and_then(|e| e.hex) {
        state.spatial_index.remove(hex, contained);
    }

    // Set containment on child
    if let Some(child) = state.entities.get_mut(contained) {
        child.contained_in = Some(container);
        child.pos = None;
        child.hex = None;
    }

    // Add to parent's contains list
    if let Some(parent) = state.entities.get_mut(container) {
        if !parent.contains.contains(&contained) {
            parent.contains.push(contained);
        }
    }
}

/// Remove `contained` from its container. The entity gets no position —
/// caller should set pos if ejecting to the world.
pub fn uncontain(state: &mut GameState, contained: EntityKey) {
    let container_key = state
        .entities
        .get(contained)
        .and_then(|e| e.contained_in);

    if let Some(container_key) = container_key {
        // Remove from parent's contains list
        if let Some(parent) = state.entities.get_mut(container_key) {
            parent.contains.retain(|k| *k != contained);
        }
    }

    // Clear containment on child
    if let Some(child) = state.entities.get_mut(contained) {
        child.contained_in = None;
    }
}

/// Eject all contained entities to the dying entity's position.
/// Each ejected entity gets the container's position and hex, and is
/// inserted into the spatial index.
fn eject_contained(state: &mut GameState, dying_key: EntityKey) {
    let (pos, hex, children) = {
        let dying = match state.entities.get(dying_key) {
            Some(e) => e,
            None => return,
        };
        (dying.pos, dying.hex, dying.contains.clone())
    };

    for child_key in children {
        if let Some(child) = state.entities.get_mut(child_key) {
            child.contained_in = None;
            child.pos = pos;
            child.hex = hex;
        }
        if let Some(hex) = hex {
            state.spatial_index.insert(hex, child_key);
        }
    }

    // Clear the dying entity's contains list
    if let Some(dying) = state.entities.get_mut(dying_key) {
        dying.contains.clear();
    }
}

// ---------------------------------------------------------------------------
// Death and cleanup
// ---------------------------------------------------------------------------

/// Remove dead entities (vitals.blood <= 0) from the game.
///
/// For each dead entity:
/// 1. Eject all contained entities to the dead entity's position.
/// 2. Remove from any container's contains list.
/// 3. Remove from spatial index.
/// 4. Remove from SlotMap.
///
/// Also removes projectile entities that have lost their projectile component
/// (already impacted — marked for cleanup by the projectile system).
pub fn cleanup_dead(state: &mut GameState) {
    // Collect keys of dead entities
    let dead_keys: SmallVec<[EntityKey; 16]> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            // Dead persons
            if let Some(ref vitals) = entity.vitals {
                if vitals.is_dead() {
                    return Some(key);
                }
            }
            None
        })
        .collect();

    for key in dead_keys {
        remove_entity(state, key);
    }
}

/// Remove a single entity from the game, handling all cleanup.
pub fn remove_entity(state: &mut GameState, key: EntityKey) {
    // Eject contained entities
    eject_contained(state, key);

    // Remove from container if contained
    uncontain(state, key);

    // Remove from spatial index
    if let Some(hex) = state.entities.get(key).and_then(|e| e.hex) {
        state.spatial_index.remove(hex, key);
    }

    // Remove from SlotMap
    state.entities.remove(key);
}

/// Remove inert projectiles (projectile component is None but entity still exists
/// as a projectile-type entity). Called after projectile impacts are processed.
pub fn cleanup_inert_projectiles(state: &mut GameState) {
    let inert: SmallVec<[EntityKey; 16]> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            // Entity has no person, no structure, no resource — it was a projectile.
            // And now has no projectile component (already impacted).
            if entity.person.is_none()
                && entity.structure.is_none()
                && entity.resource.is_none()
                && entity.weapon_props.is_none()
                && entity.armor_props.is_none()
                && entity.projectile.is_none()
                && entity.mobile.is_some()
            {
                // This is a spent projectile entity (has mobile but nothing else).
                // V3.0: remove immediately. Future: leave as resource entity.
                Some(key)
            } else {
                None
            }
        })
        .collect();

    for key in inert {
        remove_entity(state, key);
    }
}

// ---------------------------------------------------------------------------
// Elimination check
// ---------------------------------------------------------------------------

/// Check if any player has been eliminated (no person entities remaining).
/// Returns a list of eliminated player IDs.
pub fn check_elimination(state: &GameState) -> SmallVec<[u8; 4]> {
    let mut alive = [false; 16]; // supports up to 16 players

    for (_, entity) in &state.entities {
        if entity.person.is_some() {
            if let Some(owner) = entity.owner {
                if (owner as usize) < alive.len() {
                    alive[owner as usize] = true;
                }
            }
        }
    }

    let mut eliminated = SmallVec::new();
    for player in 0..state.num_players {
        if !alive[player as usize] {
            eliminated.push(player);
        }
    }
    eliminated
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::equipment::Equipment;
    use super::super::movement::Mobile;
    use super::super::spatial::{GeoMaterial, Heightfield, Vec3};
    use super::super::state::{Combatant, Person, Role, Structure, StructureType};
    use super::super::vitals::Vitals;
    use super::super::armor::MaterialType;

    fn test_state() -> GameState {
        let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
        GameState::new(20, 20, 2, hf)
    }

    #[test]
    fn spawn_assigns_id_and_key() {
        let mut gs = test_state();
        let k = spawn_entity(
            &mut gs,
            EntityBuilder::new().pos(Vec3::new(10.0, 10.0, 0.0)).owner(0),
        );
        assert_eq!(gs.entities[k].id, 1);
        assert!(gs.entities[k].pos.is_some());
        assert!(gs.entities[k].hex.is_some());
    }

    #[test]
    fn spawn_updates_spatial_index() {
        let mut gs = test_state();
        let k = spawn_entity(
            &mut gs,
            EntityBuilder::new().pos(Vec3::new(10.0, 10.0, 0.0)).owner(0),
        );
        let hex = gs.entities[k].hex.unwrap();
        assert!(gs.spatial_index.entities_at(hex).contains(&k));
    }

    #[test]
    fn containment_bidirectional() {
        let mut gs = test_state();
        let container = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(100.0, 100.0, 0.0))
                .owner(0)
                .structure(Structure {
                    structure_type: StructureType::Depot,
                    build_progress: 1.0,
                    integrity: 100.0,
                    capacity: 10,
                    material: MaterialType::Wood,
                }),
        );
        let item = spawn_entity(
            &mut gs,
            EntityBuilder::new().pos(Vec3::new(100.0, 100.0, 0.0)).owner(0),
        );

        contain(&mut gs, container, item);

        assert_eq!(gs.entities[item].contained_in, Some(container));
        assert!(gs.entities[container].contains.contains(&item));
        assert!(gs.entities[item].pos.is_none(), "contained entity loses pos");
        assert!(gs.entities[item].hex.is_none(), "contained entity loses hex");
    }

    #[test]
    fn uncontain_clears_both_sides() {
        let mut gs = test_state();
        let container = spawn_entity(
            &mut gs,
            EntityBuilder::new().pos(Vec3::new(100.0, 100.0, 0.0)).owner(0),
        );
        let item = spawn_entity(
            &mut gs,
            EntityBuilder::new().pos(Vec3::new(100.0, 100.0, 0.0)).owner(0),
        );

        contain(&mut gs, container, item);
        uncontain(&mut gs, item);

        assert_eq!(gs.entities[item].contained_in, None);
        assert!(!gs.entities[container].contains.contains(&item));
    }

    #[test]
    fn cleanup_dead_removes_dead_entities() {
        let mut gs = test_state();
        let alive = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0)
                .person(Person { role: Role::Soldier, combat_skill: 0.5 })
                .vitals(),
        );
        let dead = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(20.0, 20.0, 0.0))
                .owner(0)
                .person(Person { role: Role::Soldier, combat_skill: 0.5 })
                .vitals(),
        );

        // Kill the dead entity
        gs.entities[dead].vitals.as_mut().unwrap().blood = 0.0;

        cleanup_dead(&mut gs);

        assert!(gs.entities.contains_key(alive));
        assert!(!gs.entities.contains_key(dead));
    }

    #[test]
    fn dead_entity_ejects_equipment() {
        let mut gs = test_state();
        let soldier = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(50.0, 50.0, 0.0))
                .owner(0)
                .person(Person { role: Role::Soldier, combat_skill: 0.5 })
                .vitals()
                .equipment(Equipment::empty()),
        );
        let sword = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(50.0, 50.0, 0.0))
                .weapon_props(super::super::weapon::iron_sword()),
        );

        contain(&mut gs, soldier, sword);

        // Kill the soldier
        gs.entities[soldier].vitals.as_mut().unwrap().blood = 0.0;
        let soldier_pos = gs.entities[soldier].pos;

        cleanup_dead(&mut gs);

        // Soldier is gone
        assert!(!gs.entities.contains_key(soldier));
        // Sword is ejected to soldier's position
        assert!(gs.entities.contains_key(sword));
        assert_eq!(gs.entities[sword].pos, soldier_pos);
        assert_eq!(gs.entities[sword].contained_in, None);
    }

    #[test]
    fn check_elimination_no_persons() {
        let mut gs = test_state();
        // No person entities for player 0, player 1 has one
        spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(1)
                .person(Person { role: Role::Farmer, combat_skill: 0.0 }),
        );

        let eliminated = check_elimination(&gs);
        assert!(eliminated.contains(&0), "player 0 should be eliminated");
        assert!(!eliminated.contains(&1), "player 1 should be alive");
    }

    #[test]
    fn remove_entity_cleans_spatial_index() {
        let mut gs = test_state();
        let k = spawn_entity(
            &mut gs,
            EntityBuilder::new().pos(Vec3::new(10.0, 10.0, 0.0)).owner(0),
        );
        let hex = gs.entities[k].hex.unwrap();
        assert!(gs.spatial_index.entities_at(hex).contains(&k));

        remove_entity(&mut gs, k);
        assert!(!gs.spatial_index.entities_at(hex).contains(&k));
        assert!(!gs.entities.contains_key(k));
    }

    #[test]
    fn double_contain_no_duplicate() {
        let mut gs = test_state();
        let container = spawn_entity(
            &mut gs,
            EntityBuilder::new().pos(Vec3::new(100.0, 100.0, 0.0)).owner(0),
        );
        let item = spawn_entity(
            &mut gs,
            EntityBuilder::new().pos(Vec3::new(100.0, 100.0, 0.0)).owner(0),
        );

        contain(&mut gs, container, item);
        contain(&mut gs, container, item);

        assert_eq!(
            gs.entities[container]
                .contains
                .iter()
                .filter(|k| **k == item)
                .count(),
            1,
            "no duplicate in contains list"
        );
    }
}
