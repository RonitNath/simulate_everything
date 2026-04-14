use smallvec::SmallVec;

use super::hex::{world_to_hex_2d, within_hysteresis};
use super::spatial::Vec2;
use crate::v2::hex::{Axial, axial_to_offset, offset_to_axial};
use crate::v2::state::EntityKey;

/// Spatial index: flat array indexed by hex offset coordinates.
/// Each cell holds a SmallVec of entity keys.
#[derive(Debug, Clone)]
pub struct SpatialIndex {
    width: usize,
    height: usize,
    cells: Vec<SmallVec<[EntityKey; 4]>>,
}

impl SpatialIndex {
    pub fn new(width: usize, height: usize) -> Self {
        let count = width * height;
        Self {
            width,
            height,
            cells: vec![SmallVec::new(); count],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    // -----------------------------------------------------------------------
    // Incremental update
    // -----------------------------------------------------------------------

    /// Insert an entity into a hex cell.
    pub fn insert(&mut self, hex: Axial, key: EntityKey) {
        if let Some(idx) = self.index(hex) {
            let cell = &mut self.cells[idx];
            if !cell.contains(&key) {
                cell.push(key);
            }
        }
    }

    /// Remove an entity from a hex cell.
    pub fn remove(&mut self, hex: Axial, key: EntityKey) {
        if let Some(idx) = self.index(hex) {
            self.cells[idx].retain(|k| *k != key);
        }
    }

    /// Move an entity from one hex to another.
    pub fn move_entity(&mut self, from: Axial, to: Axial, key: EntityKey) {
        if from != to {
            self.remove(from, key);
            self.insert(to, key);
        }
    }

    // -----------------------------------------------------------------------
    // Full rebuild (debug / initialization fallback)
    // -----------------------------------------------------------------------

    /// Clear the index and rebuild from a list of (entity_key, hex) pairs.
    pub fn rebuild<I>(&mut self, entities: I)
    where
        I: IntoIterator<Item = (EntityKey, Axial)>,
    {
        for cell in &mut self.cells {
            cell.clear();
        }
        for (key, hex) in entities {
            self.insert(hex, key);
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// All entities in a hex.
    pub fn entities_at(&self, hex: Axial) -> &[EntityKey] {
        self.index(hex)
            .and_then(|idx| self.cells.get(idx))
            .map(|cell| cell.as_slice())
            .unwrap_or(&[])
    }

    /// Whether a hex contains any entities.
    pub fn has_entities_at(&self, hex: Axial) -> bool {
        !self.entities_at(hex).is_empty()
    }

    /// Iterator over all entities in hexes adjacent to `hex`.
    pub fn entities_adjacent(&self, hex: Axial) -> impl Iterator<Item = EntityKey> + '_ {
        crate::v2::hex::neighbors(hex)
            .into_iter()
            .flat_map(|nb| self.entities_at(nb).iter().copied())
    }

    /// All valid hex coordinates in this index.
    pub fn all_hexes(&self) -> impl Iterator<Item = Axial> + '_ {
        (0..self.height).flat_map(move |row| {
            (0..self.width).map(move |col| offset_to_axial(row as i32, col as i32))
        })
    }

    // -----------------------------------------------------------------------
    // Internal
    // -----------------------------------------------------------------------

    fn index(&self, ax: Axial) -> Option<usize> {
        let (row, col) = axial_to_offset(ax);
        if row < 0 || col < 0 {
            return None;
        }
        let (row, col) = (row as usize, col as usize);
        if row < self.height && col < self.width {
            Some(row * self.width + col)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Hex projection with hysteresis
// ---------------------------------------------------------------------------

/// Update an entity's hex membership with hysteresis.
/// Returns the new hex if it changed, None if unchanged.
///
/// An entity only changes hex when its position is past the center of
/// the new hex (distance to new center < 0.4 × hex_radius).
pub fn update_hex_membership(current_hex: Axial, pos: Vec2) -> Axial {
    let candidate = world_to_hex_2d(pos);
    if candidate == current_hex {
        return current_hex;
    }
    // Only switch if we're solidly in the new hex
    if within_hysteresis(pos, candidate) {
        candidate
    } else {
        current_hex
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex;
    use slotmap::SlotMap;

    fn make_key(sm: &mut SlotMap<EntityKey, ()>) -> EntityKey {
        sm.insert(())
    }

    #[test]
    fn insert_and_query() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let k1 = make_key(&mut sm);
        let k2 = make_key(&mut sm);

        let mut idx = SpatialIndex::new(10, 10);
        let hex = Axial::new(0, 0);
        idx.insert(hex, k1);
        idx.insert(hex, k2);

        assert_eq!(idx.entities_at(hex).len(), 2);
        assert!(idx.entities_at(hex).contains(&k1));
        assert!(idx.entities_at(hex).contains(&k2));
    }

    #[test]
    fn remove_entity() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let k1 = make_key(&mut sm);
        let k2 = make_key(&mut sm);

        let mut idx = SpatialIndex::new(10, 10);
        let hex = Axial::new(1, 1);
        idx.insert(hex, k1);
        idx.insert(hex, k2);
        idx.remove(hex, k1);

        assert_eq!(idx.entities_at(hex).len(), 1);
        assert!(!idx.entities_at(hex).contains(&k1));
        assert!(idx.entities_at(hex).contains(&k2));
    }

    #[test]
    fn move_entity() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let k = make_key(&mut sm);

        let mut idx = SpatialIndex::new(10, 10);
        let from = Axial::new(0, 0);
        let to = Axial::new(1, 0);
        idx.insert(from, k);
        idx.move_entity(from, to, k);

        assert!(idx.entities_at(from).is_empty());
        assert_eq!(idx.entities_at(to), &[k]);
    }

    #[test]
    fn rebuild_matches_incremental() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let keys: Vec<_> = (0..5).map(|_| make_key(&mut sm)).collect();
        let hexes = [
            Axial::new(0, 0),
            Axial::new(1, 0),
            Axial::new(0, 1),
            Axial::new(1, 1),
            Axial::new(2, 0),
        ];

        // Incremental
        let mut inc = SpatialIndex::new(10, 10);
        for (k, h) in keys.iter().zip(hexes.iter()) {
            inc.insert(*h, *k);
        }

        // Full rebuild
        let mut full = SpatialIndex::new(10, 10);
        full.rebuild(keys.iter().zip(hexes.iter()).map(|(k, h)| (*k, *h)));

        // Compare
        for h in &hexes {
            let mut a: Vec<_> = inc.entities_at(*h).to_vec();
            let mut b: Vec<_> = full.entities_at(*h).to_vec();
            a.sort_by_key(|k| format!("{:?}", k));
            b.sort_by_key(|k| format!("{:?}", k));
            assert_eq!(a, b, "mismatch at {:?}", h);
        }
    }

    #[test]
    fn no_duplicate_on_double_insert() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let k = make_key(&mut sm);

        let mut idx = SpatialIndex::new(10, 10);
        let hex = Axial::new(0, 0);
        idx.insert(hex, k);
        idx.insert(hex, k);
        assert_eq!(idx.entities_at(hex).len(), 1);
    }

    #[test]
    fn hysteresis_prevents_oscillation() {
        use super::super::hex::hex_to_world;

        let hex_a = Axial::new(0, 0);
        let hex_b = Axial::new(1, 0);
        let center_a = hex_to_world(hex_a).xy();
        let center_b = hex_to_world(hex_b).xy();

        // Midpoint between two hex centers — should NOT trigger a switch
        let midpoint = Vec2::new(
            (center_a.x + center_b.x) / 2.0,
            (center_a.y + center_b.y) / 2.0,
        );

        // Entity currently in hex_a, at the midpoint
        let result = update_hex_membership(hex_a, midpoint);
        // Should stay in hex_a because midpoint is NOT within hysteresis of hex_b
        assert_eq!(result, hex_a);
    }

    #[test]
    fn hysteresis_allows_switch_at_center() {
        use super::super::hex::hex_to_world;

        let hex_a = Axial::new(0, 0);
        let hex_b = Axial::new(1, 0);
        let center_b = hex_to_world(hex_b).xy();

        // At the center of hex_b — should trigger a switch
        let result = update_hex_membership(hex_a, center_b);
        assert_eq!(result, hex_b);
    }
}
