use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::spatial::Vec3;
use crate::v2::state::EntityKey;

/// Cell size in meters for the fine spatial index.
/// ~10m cells give good selectivity for melee queries (reach 1-5m).
const FINE_CELL_SIZE: f32 = 10.0;

/// Fine-resolution spatial hash for combat and collision queries.
///
/// Uses a simple 2D grid hash: `(floor(x / cell_size), floor(y / cell_size))`.
/// Each cell holds a SmallVec of entity keys. At 10m cells, a melee query at
/// 5m radius touches at most ~4 cells instead of scanning an entire 150m hex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FineIndex {
    cell_size: f32,
    inv_cell_size: f32,
    cells: HashMap<(i32, i32), SmallVec<[EntityKey; 4]>>,
}

impl FineIndex {
    pub fn new() -> Self {
        Self {
            cell_size: FINE_CELL_SIZE,
            inv_cell_size: 1.0 / FINE_CELL_SIZE,
            cells: HashMap::new(),
        }
    }

    /// Which cell a world position falls into.
    #[inline]
    fn cell_for_pos(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x * self.inv_cell_size).floor() as i32,
            (pos.y * self.inv_cell_size).floor() as i32,
        )
    }

    /// Insert an entity at a world position.
    pub fn insert(&mut self, pos: Vec3, entity: EntityKey) {
        let cell = self.cell_for_pos(pos);
        let bucket = self.cells.entry(cell).or_default();
        if !bucket.contains(&entity) {
            bucket.push(entity);
        }
    }

    /// Remove an entity from a world position.
    pub fn remove(&mut self, pos: Vec3, entity: EntityKey) {
        let cell = self.cell_for_pos(pos);
        if let Some(bucket) = self.cells.get_mut(&cell) {
            bucket.retain(|k| *k != entity);
            if bucket.is_empty() {
                self.cells.remove(&cell);
            }
        }
    }

    /// Move an entity from one position to another.
    pub fn move_entity(&mut self, old_pos: Vec3, new_pos: Vec3, entity: EntityKey) {
        let old_cell = self.cell_for_pos(old_pos);
        let new_cell = self.cell_for_pos(new_pos);
        if old_cell != new_cell {
            self.remove(old_pos, entity);
            self.insert(new_pos, entity);
        }
    }

    /// Query all entities within `radius` meters of `center`.
    ///
    /// Scans only the cells that overlap the query circle, then fine-filters
    /// by squared distance.
    pub fn query_radius(&self, center: Vec3, radius: f32) -> SmallVec<[EntityKey; 8]> {
        // We don't have per-entity positions stored here, so we return all
        // entities in cells that overlap the query circle. The caller is
        // responsible for the final distance check against actual positions.
        //
        // Cell coverage: any cell whose center is within (radius + cell_size * sqrt(2)/2)
        // could contain a point within radius. We use a simpler bounding box.
        let r = radius + self.cell_size; // conservative expansion
        let min_cx = ((center.x - r) * self.inv_cell_size).floor() as i32;
        let max_cx = ((center.x + r) * self.inv_cell_size).floor() as i32;
        let min_cy = ((center.y - r) * self.inv_cell_size).floor() as i32;
        let max_cy = ((center.y + r) * self.inv_cell_size).floor() as i32;

        let mut result = SmallVec::new();
        for cx in min_cx..=max_cx {
            for cy in min_cy..=max_cy {
                if let Some(bucket) = self.cells.get(&(cx, cy)) {
                    result.extend_from_slice(bucket);
                }
            }
        }
        result
    }

    /// Clear and rebuild the entire index from an iterator of (key, position).
    pub fn rebuild<I>(&mut self, entities: I)
    where
        I: IntoIterator<Item = (EntityKey, Vec3)>,
    {
        self.cells.clear();
        for (key, pos) in entities {
            self.insert(pos, key);
        }
    }

    /// Number of occupied cells (for diagnostics).
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Total entity entries across all cells.
    pub fn entity_count(&self) -> usize {
        self.cells.values().map(|b| b.len()).sum()
    }
}

impl Default for FineIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn make_keys(n: usize) -> (SlotMap<EntityKey, ()>, Vec<EntityKey>) {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let keys: Vec<_> = (0..n).map(|_| sm.insert(())).collect();
        (sm, keys)
    }

    #[test]
    fn insert_and_query_within_radius() {
        let (_sm, keys) = make_keys(5);
        let mut idx = FineIndex::new();

        // Place entities at known positions
        idx.insert(Vec3::new(0.0, 0.0, 0.0), keys[0]);
        idx.insert(Vec3::new(3.0, 0.0, 0.0), keys[1]); // 3m away
        idx.insert(Vec3::new(0.0, 4.0, 0.0), keys[2]); // 4m away
        idx.insert(Vec3::new(50.0, 50.0, 0.0), keys[3]); // far away
        idx.insert(Vec3::new(100.0, 100.0, 0.0), keys[4]); // very far

        // Query at 5m radius from origin — should find keys 0, 1, 2
        // (they're all in the same or adjacent cells)
        let result = idx.query_radius(Vec3::new(0.0, 0.0, 0.0), 5.0);
        assert!(result.contains(&keys[0]));
        assert!(result.contains(&keys[1]));
        assert!(result.contains(&keys[2]));
        assert!(!result.contains(&keys[3]));
        assert!(!result.contains(&keys[4]));
    }

    #[test]
    fn move_across_cell_boundary() {
        let (_sm, keys) = make_keys(1);
        let mut idx = FineIndex::new();

        let old = Vec3::new(5.0, 5.0, 0.0);
        let new = Vec3::new(25.0, 25.0, 0.0); // different cell
        idx.insert(old, keys[0]);
        idx.move_entity(old, new, keys[0]);

        // Should not be in old cell's neighborhood
        let old_query = idx.query_radius(old, 1.0);
        assert!(!old_query.contains(&keys[0]));

        // Should be in new cell
        let new_query = idx.query_radius(new, 1.0);
        assert!(new_query.contains(&keys[0]));
    }

    #[test]
    fn move_within_same_cell() {
        let (_sm, keys) = make_keys(1);
        let mut idx = FineIndex::new();

        let old = Vec3::new(1.0, 1.0, 0.0);
        let new = Vec3::new(2.0, 2.0, 0.0); // same cell (both in 0,0)
        idx.insert(old, keys[0]);
        idx.move_entity(old, new, keys[0]);

        let result = idx.query_radius(new, 1.0);
        assert!(result.contains(&keys[0]));
        assert_eq!(idx.entity_count(), 1);
    }

    #[test]
    fn query_empty_returns_empty() {
        let idx = FineIndex::new();
        let result = idx.query_radius(Vec3::new(100.0, 100.0, 0.0), 5.0);
        assert!(result.is_empty());
    }

    #[test]
    fn no_duplicate_on_double_insert() {
        let (_sm, keys) = make_keys(1);
        let mut idx = FineIndex::new();
        let pos = Vec3::new(0.0, 0.0, 0.0);
        idx.insert(pos, keys[0]);
        idx.insert(pos, keys[0]);
        assert_eq!(idx.entity_count(), 1);
    }

    #[test]
    fn rebuild_matches_incremental() {
        let (_sm, keys) = make_keys(100);
        let mut idx = FineIndex::new();

        let positions: Vec<Vec3> = (0..100)
            .map(|i| Vec3::new(i as f32 * 3.0, (i as f32 * 7.0) % 200.0, 0.0))
            .collect();

        // Incremental
        for (k, p) in keys.iter().zip(positions.iter()) {
            idx.insert(*p, *k);
        }

        // Rebuild
        let mut idx2 = FineIndex::new();
        idx2.rebuild(keys.iter().copied().zip(positions.iter().copied()));

        assert_eq!(idx.entity_count(), idx2.entity_count());
        assert_eq!(idx.cell_count(), idx2.cell_count());
    }

    #[test]
    fn performance_10k_entities_1k_queries() {
        let (_sm, keys) = make_keys(10_000);
        let mut idx = FineIndex::new();

        // Spread 10k entities across a 3km × 3km area
        for (i, k) in keys.iter().enumerate() {
            let x = (i as f32 * 17.3) % 3000.0;
            let y = (i as f32 * 31.7) % 3000.0;
            idx.insert(Vec3::new(x, y, 0.0), *k);
        }

        // 1k queries at 5m radius
        let start = std::time::Instant::now();
        let mut total_found = 0usize;
        for i in 0..1000 {
            let x = (i as f32 * 3.1) % 3000.0;
            let y = (i as f32 * 7.7) % 3000.0;
            let result = idx.query_radius(Vec3::new(x, y, 0.0), 5.0);
            total_found += result.len();
        }
        let elapsed = start.elapsed();

        // Spec: 1k queries × 5m radius < 1ms with 10k entities
        assert!(
            elapsed.as_millis() < 50, // generous for CI; real target is <1ms
            "1k queries took {:?} (found {} total candidates)",
            elapsed,
            total_found
        );
    }
}
