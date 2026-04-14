use smallvec::SmallVec;

use super::hex::world_to_hex;
use super::state::{EntityBuilder, GameState};
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

    // Update spatial indices
    if let Some(hex) = state.entities[key].hex {
        state.spatial_index.insert(hex, key);
    }
    if let Some(pos) = state.entities[key].pos {
        state.fine_index.insert(pos, key);
        let e = &state.entities[key];
        let is_soldier = e
            .person
            .as_ref()
            .map(|p| p.role == super::state::Role::Soldier)
            .unwrap_or(false);
        state.coarse_index.insert(pos, key, e.owner, is_soldier);
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
    // Remove contained entity from spatial indices
    if let Some(e) = state.entities.get(contained) {
        if let Some(hex) = e.hex {
            state.spatial_index.remove(hex, contained);
        }
        if let Some(pos) = e.pos {
            let is_soldier = e
                .person
                .as_ref()
                .map(|p| p.role == super::state::Role::Soldier)
                .unwrap_or(false);
            state.fine_index.remove(pos, contained);
            state.coarse_index.remove(pos, contained, e.owner, is_soldier);
        }
    }

    // Set containment on child
    if let Some(child) = state.entities.get_mut(contained) {
        child.contained_in = Some(container);
        child.pos = None;
        child.hex = None;
    }

    // Add to parent's contains list
    if let Some(parent) = state.entities.get_mut(container)
        && !parent.contains.contains(&contained)
    {
        parent.contains.push(contained);
    }
}

/// Remove `contained` from its container. The entity gets no position —
/// caller should set pos if ejecting to the world.
pub fn uncontain(state: &mut GameState, contained: EntityKey) {
    let container_key = state.entities.get(contained).and_then(|e| e.contained_in);

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
        // Snapshot owner/role before mutating
        let (owner, is_soldier) = state
            .entities
            .get(child_key)
            .map(|e| {
                let is_s = e
                    .person
                    .as_ref()
                    .map(|p| p.role == super::state::Role::Soldier)
                    .unwrap_or(false);
                (e.owner, is_s)
            })
            .unwrap_or((None, false));

        if let Some(child) = state.entities.get_mut(child_key) {
            child.contained_in = None;
            child.pos = pos;
            child.hex = hex;
        }
        if let Some(hex) = hex {
            state.spatial_index.insert(hex, child_key);
        }
        if let Some(pos) = pos {
            state.fine_index.insert(pos, child_key);
            state.coarse_index.insert(pos, child_key, owner, is_soldier);
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

/// Transition dead entities (vitals.blood <= 0) to inert corpses.
///
/// Dead entities persist in the SlotMap at their position as inert corpses.
/// Equipment remains contained_in the corpse. For each newly dead entity:
/// 1. Strip Mobile component (can't move).
/// 2. Strip Combatant component (can't fight).
/// 3. Entity stays in the SlotMap with position, equipment, and wounds intact.
pub fn cleanup_dead(state: &mut GameState) {
    // Collect keys of newly dead entities (have vitals.dead but still have mobile/combatant)
    let dead_keys: SmallVec<[EntityKey; 16]> = state
        .entities
        .iter()
        .filter_map(|(key, entity)| {
            if let Some(ref vitals) = entity.vitals
                && vitals.is_dead()
                && (entity.mobile.is_some() || entity.combatant.is_some())
            {
                return Some(key);
            }
            None
        })
        .collect();

    for key in dead_keys {
        if let Some(entity) = state.entities.get_mut(key) {
            entity.mobile = None;
            entity.combatant = None;
        }
    }
}

/// Remove a single entity from the game, handling all cleanup.
pub fn remove_entity(state: &mut GameState, key: EntityKey) {
    // Eject contained entities
    eject_contained(state, key);

    // Remove from container if contained
    uncontain(state, key);

    // Remove from spatial indices
    if let Some(e) = state.entities.get(key) {
        if let Some(hex) = e.hex {
            state.spatial_index.remove(hex, key);
        }
        if let Some(pos) = e.pos {
            let is_soldier = e
                .person
                .as_ref()
                .map(|p| p.role == super::state::Role::Soldier)
                .unwrap_or(false);
            state.fine_index.remove(pos, key);
            state.coarse_index.remove(pos, key, e.owner, is_soldier);
        }
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

/// Check if any player has been eliminated (no living person entities remaining).
/// Dead persons (inert corpses) do not count as alive.
/// Returns a list of eliminated player IDs.
pub fn check_elimination(state: &GameState) -> SmallVec<[u8; 4]> {
    let mut alive = [false; 16]; // supports up to 16 players

    for (_, entity) in &state.entities {
        if entity.person.is_some() {
            // Dead persons don't count
            let is_dead = entity.vitals.as_ref().map(|v| v.is_dead()).unwrap_or(false);
            if is_dead {
                continue;
            }
            if let Some(owner) = entity.owner
                && (owner as usize) < alive.len()
            {
                alive[owner as usize] = true;
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
    use super::super::armor::MaterialType;
    use super::super::equipment::Equipment;
    use super::super::movement::Mobile;
    use super::super::spatial::{GeoMaterial, Heightfield, Vec3};
    use super::super::state::{Combatant, Person, Role, Structure, StructureType};
    use super::super::vitals::Vitals;
    use super::*;

    fn test_state() -> GameState {
        let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
        GameState::new(20, 20, 2, hf)
    }

    #[test]
    fn spawn_assigns_id_and_key() {
        let mut gs = test_state();
        let k = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0),
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
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0),
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
            EntityBuilder::new()
                .pos(Vec3::new(100.0, 100.0, 0.0))
                .owner(0),
        );

        contain(&mut gs, container, item);

        assert_eq!(gs.entities[item].contained_in, Some(container));
        assert!(gs.entities[container].contains.contains(&item));
        assert!(
            gs.entities[item].pos.is_none(),
            "contained entity loses pos"
        );
        assert!(
            gs.entities[item].hex.is_none(),
            "contained entity loses hex"
        );
    }

    #[test]
    fn uncontain_clears_both_sides() {
        let mut gs = test_state();
        let container = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(100.0, 100.0, 0.0))
                .owner(0),
        );
        let item = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(100.0, 100.0, 0.0))
                .owner(0),
        );

        contain(&mut gs, container, item);
        uncontain(&mut gs, item);

        assert_eq!(gs.entities[item].contained_in, None);
        assert!(!gs.entities[container].contains.contains(&item));
    }

    #[test]
    fn cleanup_dead_makes_entity_inert() {
        let mut gs = test_state();
        let alive = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                    task: None,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals(),
        );
        let dead = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(20.0, 20.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                    task: None,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals(),
        );

        // Kill the dead entity
        gs.entities[dead].vitals.as_mut().unwrap().blood = 0.0;

        cleanup_dead(&mut gs);

        // Both still in SlotMap
        assert!(gs.entities.contains_key(alive));
        assert!(gs.entities.contains_key(dead));
        // Dead entity is inert: no mobile, no combatant
        assert!(
            gs.entities[dead].mobile.is_none(),
            "dead entity should lose mobile"
        );
        assert!(
            gs.entities[dead].combatant.is_none(),
            "dead entity should lose combatant"
        );
        // Dead entity retains position and person
        assert!(
            gs.entities[dead].pos.is_some(),
            "dead entity keeps position"
        );
        assert!(
            gs.entities[dead].person.is_some(),
            "dead entity keeps person"
        );
        // Alive entity unchanged
        assert!(gs.entities[alive].mobile.is_some());
        assert!(gs.entities[alive].combatant.is_some());
    }

    #[test]
    fn dead_entity_retains_equipment() {
        let mut gs = test_state();
        let soldier = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(50.0, 50.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                    task: None,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
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
        gs.entities[soldier].equipment.as_mut().unwrap().weapon = Some(sword);

        // Kill the soldier
        gs.entities[soldier].vitals.as_mut().unwrap().blood = 0.0;

        cleanup_dead(&mut gs);

        // Soldier persists as inert corpse
        assert!(gs.entities.contains_key(soldier));
        // Equipment still contained in the corpse
        assert!(gs.entities.contains_key(sword));
        assert_eq!(gs.entities[sword].contained_in, Some(soldier));
        assert!(gs.entities[soldier].contains.contains(&sword));
        // Equipment slot still references the sword
        assert_eq!(
            gs.entities[soldier].equipment.as_ref().unwrap().weapon,
            Some(sword)
        );
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
                .person(Person {
                    role: Role::Farmer,
                    combat_skill: 0.0,
                    task: None,
                }),
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
            EntityBuilder::new()
                .pos(Vec3::new(10.0, 10.0, 0.0))
                .owner(0),
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
            EntityBuilder::new()
                .pos(Vec3::new(100.0, 100.0, 0.0))
                .owner(0),
        );
        let item = spawn_entity(
            &mut gs,
            EntityBuilder::new()
                .pos(Vec3::new(100.0, 100.0, 0.0))
                .owner(0),
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
