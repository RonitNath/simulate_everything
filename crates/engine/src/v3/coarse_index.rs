use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::spatial::Vec3;
use crate::v2::state::EntityKey;

/// Cell size in meters for the coarse spatial index (~500m).
const COARSE_CELL_SIZE: f32 = 500.0;

/// Maximum player slots for fixed-size per-player arrays.
pub const MAX_PLAYERS: usize = 16;

/// Aggregate data for one coarse hex cell.
///
/// Maintained incrementally — when entities enter/leave the cell,
/// the aggregate is updated rather than recomputed from scratch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoarseHexAggregate {
    /// Entity count per player.
    pub population: [u16; MAX_PLAYERS],
    /// Sum of combat effectiveness per player (soldier count for now).
    pub army_strength: [f32; MAX_PLAYERS],
    /// Entity keys for full iteration when needed.
    pub entities: SmallVec<[EntityKey; 16]>,
}

impl CoarseHexAggregate {
    fn new() -> Self {
        Self {
            population: [0; MAX_PLAYERS],
            army_strength: [0.0; MAX_PLAYERS],
            entities: SmallVec::new(),
        }
    }

    /// Total population across all players.
    pub fn total_population(&self) -> u32 {
        self.population.iter().map(|&p| p as u32).sum()
    }

    /// Whether this cell has any entities.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// Coarse-resolution spatial index for strategic AI queries.
///
/// Uses a simple 2D grid hash at ~500m cell size. Each cell holds an
/// aggregate with per-player population, army strength, and entity lists.
/// Strategic AI can query "total army strength in a 2km region" as a sum
/// of coarse cell aggregates in O(cells) instead of O(all entities).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoarseIndex {
    cell_size: f32,
    inv_cell_size: f32,
    cells: HashMap<(i32, i32), CoarseHexAggregate>,
}

impl CoarseIndex {
    pub fn new() -> Self {
        Self {
            cell_size: COARSE_CELL_SIZE,
            inv_cell_size: 1.0 / COARSE_CELL_SIZE,
            cells: HashMap::new(),
        }
    }

    /// Which coarse cell a world position falls into.
    #[inline]
    pub fn cell_for_pos(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x * self.inv_cell_size).floor() as i32,
            (pos.y * self.inv_cell_size).floor() as i32,
        )
    }

    /// Insert an entity into the coarse index.
    /// `owner` is None for neutral entities, Some(player_id) for owned entities.
    /// `is_soldier` indicates whether the entity contributes to army strength.
    pub fn insert(&mut self, pos: Vec3, entity: EntityKey, owner: Option<u8>, is_soldier: bool) {
        let cell = self.cell_for_pos(pos);
        let agg = self
            .cells
            .entry(cell)
            .or_insert_with(CoarseHexAggregate::new);
        if !agg.entities.contains(&entity) {
            agg.entities.push(entity);
            if let Some(p) = owner {
                if (p as usize) < MAX_PLAYERS {
                    agg.population[p as usize] += 1;
                    if is_soldier {
                        agg.army_strength[p as usize] += 1.0;
                    }
                }
            }
        }
    }

    /// Remove an entity from the coarse index.
    pub fn remove(&mut self, pos: Vec3, entity: EntityKey, owner: Option<u8>, is_soldier: bool) {
        let cell = self.cell_for_pos(pos);
        if let Some(agg) = self.cells.get_mut(&cell) {
            let had = agg.entities.contains(&entity);
            agg.entities.retain(|k| *k != entity);
            if had {
                if let Some(p) = owner {
                    if (p as usize) < MAX_PLAYERS {
                        agg.population[p as usize] = agg.population[p as usize].saturating_sub(1);
                        if is_soldier {
                            agg.army_strength[p as usize] =
                                (agg.army_strength[p as usize] - 1.0).max(0.0);
                        }
                    }
                }
            }
            if agg.is_empty() {
                self.cells.remove(&cell);
            }
        }
    }

    /// Move an entity between coarse cells. Only does work if the cell changed.
    pub fn move_entity(
        &mut self,
        old_pos: Vec3,
        new_pos: Vec3,
        entity: EntityKey,
        owner: Option<u8>,
        is_soldier: bool,
    ) {
        let old_cell = self.cell_for_pos(old_pos);
        let new_cell = self.cell_for_pos(new_pos);
        if old_cell != new_cell {
            self.remove(old_pos, entity, owner, is_soldier);
            self.insert(new_pos, entity, owner, is_soldier);
        }
    }

    /// Get the aggregate for a coarse cell, if any entities are in it.
    pub fn aggregate_at(&self, cell: (i32, i32)) -> Option<&CoarseHexAggregate> {
        self.cells.get(&cell)
    }

    /// Sum army strength for a player across all coarse cells within `radius` meters
    /// of `center`. O(cells in radius) instead of O(all entities).
    pub fn army_strength_in_radius(&self, center: Vec3, radius: f32, player: u8) -> f32 {
        if (player as usize) >= MAX_PLAYERS {
            return 0.0;
        }
        let r = radius + self.cell_size;
        let min_cx = ((center.x - r) * self.inv_cell_size).floor() as i32;
        let max_cx = ((center.x + r) * self.inv_cell_size).floor() as i32;
        let min_cy = ((center.y - r) * self.inv_cell_size).floor() as i32;
        let max_cy = ((center.y + r) * self.inv_cell_size).floor() as i32;

        let mut total = 0.0f32;
        for cx in min_cx..=max_cx {
            for cy in min_cy..=max_cy {
                if let Some(agg) = self.cells.get(&(cx, cy)) {
                    total += agg.army_strength[player as usize];
                }
            }
        }
        total
    }

    /// Sum population for a player across all coarse cells within `radius` meters.
    pub fn population_in_radius(&self, center: Vec3, radius: f32, player: u8) -> u32 {
        if (player as usize) >= MAX_PLAYERS {
            return 0;
        }
        let r = radius + self.cell_size;
        let min_cx = ((center.x - r) * self.inv_cell_size).floor() as i32;
        let max_cx = ((center.x + r) * self.inv_cell_size).floor() as i32;
        let min_cy = ((center.y - r) * self.inv_cell_size).floor() as i32;
        let max_cy = ((center.y + r) * self.inv_cell_size).floor() as i32;

        let mut total = 0u32;
        for cx in min_cx..=max_cx {
            for cy in min_cy..=max_cy {
                if let Some(agg) = self.cells.get(&(cx, cy)) {
                    total += agg.population[player as usize] as u32;
                }
            }
        }
        total
    }

    /// Clear and rebuild the entire index from an iterator of entity data.
    pub fn rebuild<I>(&mut self, entities: I)
    where
        I: IntoIterator<Item = (EntityKey, Vec3, Option<u8>, bool)>,
    {
        self.cells.clear();
        for (key, pos, owner, is_soldier) in entities {
            self.insert(pos, key, owner, is_soldier);
        }
    }

    /// Number of occupied coarse cells.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

impl Default for CoarseIndex {
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
    fn insert_and_aggregate() {
        let (_sm, keys) = make_keys(3);
        let mut idx = CoarseIndex::new();

        idx.insert(Vec3::new(100.0, 100.0, 0.0), keys[0], Some(0), true);
        idx.insert(Vec3::new(110.0, 110.0, 0.0), keys[1], Some(0), true);
        idx.insert(Vec3::new(120.0, 120.0, 0.0), keys[2], Some(1), true);

        let cell = idx.cell_for_pos(Vec3::new(100.0, 100.0, 0.0));
        let agg = idx.aggregate_at(cell).unwrap();
        assert_eq!(agg.population[0], 2);
        assert_eq!(agg.population[1], 1);
        assert_eq!(agg.army_strength[0], 2.0);
        assert_eq!(agg.army_strength[1], 1.0);
        assert_eq!(agg.entities.len(), 3);
    }

    #[test]
    fn remove_decrements_aggregate() {
        let (_sm, keys) = make_keys(2);
        let mut idx = CoarseIndex::new();
        let pos = Vec3::new(100.0, 100.0, 0.0);

        idx.insert(pos, keys[0], Some(0), true);
        idx.insert(pos, keys[1], Some(0), false);
        idx.remove(pos, keys[0], Some(0), true);

        let cell = idx.cell_for_pos(pos);
        let agg = idx.aggregate_at(cell).unwrap();
        assert_eq!(agg.population[0], 1);
        assert_eq!(agg.army_strength[0], 0.0);
        assert_eq!(agg.entities.len(), 1);
        assert!(agg.entities.contains(&keys[1]));
    }

    #[test]
    fn remove_last_entity_cleans_cell() {
        let (_sm, keys) = make_keys(1);
        let mut idx = CoarseIndex::new();
        let pos = Vec3::new(100.0, 100.0, 0.0);

        idx.insert(pos, keys[0], Some(0), true);
        idx.remove(pos, keys[0], Some(0), true);

        let cell = idx.cell_for_pos(pos);
        assert!(idx.aggregate_at(cell).is_none());
        assert_eq!(idx.cell_count(), 0);
    }

    #[test]
    fn move_across_coarse_boundary() {
        let (_sm, keys) = make_keys(1);
        let mut idx = CoarseIndex::new();

        let old = Vec3::new(100.0, 100.0, 0.0);
        let new = Vec3::new(600.0, 600.0, 0.0); // different coarse cell
        idx.insert(old, keys[0], Some(0), true);
        idx.move_entity(old, new, keys[0], Some(0), true);

        let old_cell = idx.cell_for_pos(old);
        let new_cell = idx.cell_for_pos(new);
        assert!(idx.aggregate_at(old_cell).is_none());
        let agg = idx.aggregate_at(new_cell).unwrap();
        assert_eq!(agg.population[0], 1);
    }

    #[test]
    fn army_strength_in_radius() {
        let (_sm, keys) = make_keys(5);
        let mut idx = CoarseIndex::new();

        // 3 soldiers for player 0 near origin
        idx.insert(Vec3::new(0.0, 0.0, 0.0), keys[0], Some(0), true);
        idx.insert(Vec3::new(100.0, 0.0, 0.0), keys[1], Some(0), true);
        idx.insert(Vec3::new(200.0, 0.0, 0.0), keys[2], Some(0), true);
        // 1 soldier for player 1 nearby
        idx.insert(Vec3::new(50.0, 50.0, 0.0), keys[3], Some(1), true);
        // 1 soldier far away
        idx.insert(Vec3::new(5000.0, 5000.0, 0.0), keys[4], Some(0), true);

        let str_p0 = idx.army_strength_in_radius(Vec3::new(100.0, 0.0, 0.0), 2000.0, 0);
        assert_eq!(str_p0, 3.0); // should find 3 nearby, not the far one

        let str_p1 = idx.army_strength_in_radius(Vec3::new(100.0, 0.0, 0.0), 2000.0, 1);
        assert_eq!(str_p1, 1.0);
    }

    #[test]
    fn no_duplicate_on_double_insert() {
        let (_sm, keys) = make_keys(1);
        let mut idx = CoarseIndex::new();
        let pos = Vec3::new(0.0, 0.0, 0.0);
        idx.insert(pos, keys[0], Some(0), true);
        idx.insert(pos, keys[0], Some(0), true);

        let cell = idx.cell_for_pos(pos);
        let agg = idx.aggregate_at(cell).unwrap();
        assert_eq!(agg.entities.len(), 1);
        assert_eq!(agg.population[0], 1);
    }

    #[test]
    fn population_in_radius_matches_brute_force() {
        let (_sm, keys) = make_keys(20);
        let mut idx = CoarseIndex::new();

        for (i, k) in keys.iter().enumerate() {
            let x = (i as f32) * 100.0;
            idx.insert(Vec3::new(x, 0.0, 0.0), *k, Some(0), true);
        }

        let pop = idx.population_in_radius(Vec3::new(500.0, 0.0, 0.0), 2000.0, 0);
        // All 20 entities are within 2000m of x=500
        assert_eq!(pop, 20);
    }
}
