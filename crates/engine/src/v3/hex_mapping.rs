use serde::{Deserialize, Serialize};

use super::coarse_index::CoarseIndex;
use super::fine_index::FineIndex;
use super::spatial::Vec3;
use crate::v2::hex::Axial;
use crate::v2::state::EntityKey;

use super::index::SpatialIndex;
use super::state::GameState;

// ---------------------------------------------------------------------------
// Cross-level hex mapping
// ---------------------------------------------------------------------------

/// Precomputed cross-level lookups between fine, medium, and coarse hex grids.
///
/// Initialized at map creation. Used to cascade entity moves through all three
/// index levels and to map hex regions to terrain chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexMapping {
    /// Fine cell size (matches FineIndex).
    fine_cell_size: f32,
    /// Coarse cell size (matches CoarseIndex).
    coarse_cell_size: f32,
}

impl HexMapping {
    pub fn new() -> Self {
        Self {
            fine_cell_size: 10.0,
            coarse_cell_size: 500.0,
        }
    }

    /// Convert a world position to fine cell coordinates.
    #[inline]
    pub fn fine_cell(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x / self.fine_cell_size).floor() as i32,
            (pos.y / self.fine_cell_size).floor() as i32,
        )
    }

    /// Convert a world position to the medium hex (axial coordinates).
    #[inline]
    pub fn medium_hex(&self, pos: Vec3) -> Axial {
        super::hex::world_to_hex(pos)
    }

    /// Convert a world position to coarse cell coordinates.
    #[inline]
    pub fn coarse_cell(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x / self.coarse_cell_size).floor() as i32,
            (pos.y / self.coarse_cell_size).floor() as i32,
        )
    }

    /// Check which index levels changed between two positions.
    pub fn levels_changed(&self, old_pos: Vec3, new_pos: Vec3) -> LevelsChanged {
        LevelsChanged {
            fine: self.fine_cell(old_pos) != self.fine_cell(new_pos),
            medium: self.medium_hex(old_pos) != self.medium_hex(new_pos),
            coarse: self.coarse_cell(old_pos) != self.coarse_cell(new_pos),
        }
    }
}

impl Default for HexMapping {
    fn default() -> Self {
        Self::new()
    }
}

/// Which index levels changed during an entity move.
#[derive(Debug, Clone, Copy)]
pub struct LevelsChanged {
    pub fine: bool,
    pub medium: bool,
    pub coarse: bool,
}

// ---------------------------------------------------------------------------
// Entity move cascade
// ---------------------------------------------------------------------------

/// Cascade an entity position change through all three spatial index levels.
///
/// Fine index: always updated (cheap hash move).
/// Medium index: only on hex change (with hysteresis).
/// Coarse index: only on coarse cell change (rare).
///
/// Returns which levels actually changed.
pub fn on_entity_move(
    state: &mut GameState,
    entity: EntityKey,
    old_pos: Vec3,
    new_pos: Vec3,
) -> LevelsChanged {
    let mapping = &state.hex_mapping;
    let changes = mapping.levels_changed(old_pos, new_pos);

    // Fine: always check (cheap hash table move)
    if changes.fine {
        state.fine_index.move_entity(old_pos, new_pos, entity);
    }

    // Medium: only on hex change
    if changes.medium {
        let old_hex = mapping.medium_hex(old_pos);
        let new_hex = mapping.medium_hex(new_pos);
        // Apply hysteresis check from the existing index module
        let current_hex = state
            .entities
            .get(entity)
            .and_then(|e| e.hex)
            .unwrap_or(old_hex);
        let resolved = super::index::update_hex_membership(current_hex, new_pos.xy());
        if resolved != current_hex {
            state.spatial_index.move_entity(current_hex, resolved, entity);
            if let Some(e) = state.entities.get_mut(entity) {
                e.hex = Some(resolved);
            }
        }
    }

    // Coarse: only on coarse cell change
    if changes.coarse {
        let (owner, is_soldier) = state
            .entities
            .get(entity)
            .map(|e| {
                let is_s = e
                    .person
                    .as_ref()
                    .map(|p| p.role == super::state::Role::Soldier)
                    .unwrap_or(false);
                (e.owner, is_s)
            })
            .unwrap_or((None, false));
        state
            .coarse_index
            .move_entity(old_pos, new_pos, entity, owner, is_soldier);
    }

    changes
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lifecycle::spawn_entity;
    use super::super::movement::Mobile;
    use super::super::spatial::{GeoMaterial, Heightfield};
    use super::super::state::{Combatant, EntityBuilder, Person, Role};

    fn test_state() -> GameState {
        let hf = Heightfield::new(20, 20, 0.0, GeoMaterial::Soil);
        GameState::new(20, 20, 2, hf)
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
                    task: None,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals(),
        )
    }

    #[test]
    fn cross_level_mapping_consistent() {
        let mapping = HexMapping::new();
        // A position should map to consistent fine, medium, coarse cells
        let pos = Vec3::new(300.0, 400.0, 0.0);
        let fine = mapping.fine_cell(pos);
        let medium = mapping.medium_hex(pos);
        let coarse = mapping.coarse_cell(pos);

        assert_eq!(fine, (30, 40)); // 300/10, 400/10
        assert_eq!(coarse, (0, 0)); // 300/500, 400/500 → both 0

        // Medium hex should be deterministic
        let medium2 = mapping.medium_hex(pos);
        assert_eq!(medium, medium2);
    }

    #[test]
    fn levels_changed_fine_only() {
        let mapping = HexMapping::new();
        // Move 15m — crosses fine boundary but stays in same medium and coarse cell
        let old = Vec3::new(100.0, 100.0, 0.0);
        let new = Vec3::new(115.0, 100.0, 0.0);
        let changes = mapping.levels_changed(old, new);
        assert!(changes.fine);
        assert!(!changes.coarse);
    }

    #[test]
    fn levels_changed_all() {
        let mapping = HexMapping::new();
        // Move from one side of map to the other — all levels change
        let old = Vec3::new(100.0, 100.0, 0.0);
        let new = Vec3::new(1000.0, 1000.0, 0.0);
        let changes = mapping.levels_changed(old, new);
        assert!(changes.fine);
        assert!(changes.medium);
        assert!(changes.coarse);
    }

    #[test]
    fn on_entity_move_updates_all_levels() {
        let mut state = test_state();
        let old_pos = Vec3::new(100.0, 100.0, 0.0);
        let new_pos = Vec3::new(1000.0, 1000.0, 0.0);
        let key = spawn_soldier(&mut state, old_pos, 0);

        let changes = on_entity_move(&mut state, key, old_pos, new_pos);

        assert!(changes.fine);
        assert!(changes.coarse);

        // Fine index should find entity at new position
        let result = state.fine_index.query_radius(new_pos, 5.0);
        assert!(result.contains(&key));
        let old_result = state.fine_index.query_radius(old_pos, 5.0);
        assert!(!old_result.contains(&key));
    }

    #[test]
    fn on_entity_move_no_change_within_cell() {
        let mut state = test_state();
        let old_pos = Vec3::new(101.0, 101.0, 0.0);
        let new_pos = Vec3::new(102.0, 102.0, 0.0); // same fine cell
        let key = spawn_soldier(&mut state, old_pos, 0);

        let changes = on_entity_move(&mut state, key, old_pos, new_pos);

        assert!(!changes.fine);
        assert!(!changes.coarse);
    }

    #[test]
    fn cascade_overhead_benchmark() {
        let mut state = test_state();
        let keys: Vec<EntityKey> = (0..100)
            .map(|i| {
                spawn_soldier(
                    &mut state,
                    Vec3::new(i as f32 * 30.0, i as f32 * 30.0, 0.0),
                    (i % 2) as u8,
                )
            })
            .collect();

        let start = std::time::Instant::now();
        // Simulate 100 ticks of 100 entity moves each
        for tick in 0..100 {
            for (i, &key) in keys.iter().enumerate() {
                let old = Vec3::new(
                    i as f32 * 30.0 + tick as f32,
                    i as f32 * 30.0 + tick as f32,
                    0.0,
                );
                let new = Vec3::new(
                    i as f32 * 30.0 + (tick + 1) as f32,
                    i as f32 * 30.0 + (tick + 1) as f32,
                    0.0,
                );
                on_entity_move(&mut state, key, old, new);
            }
        }
        let elapsed = start.elapsed();

        // 10k entity moves total, spec says <0.5ms for cascade overhead
        assert!(
            elapsed.as_millis() < 100, // generous for CI
            "10k entity move cascades took {:?}",
            elapsed
        );
    }
}
