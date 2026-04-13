use slotmap::SlotMap;
use smallvec::SmallVec;

use super::hex::{Axial, axial_to_offset, neighbors};
use super::state::{Unit, UnitKey};

#[derive(Debug, Clone, Default)]
pub struct SpatialIndex {
    width: usize,
    height: usize,
    cells: Vec<SmallVec<[UnitKey; 4]>>,
}

impl SpatialIndex {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![SmallVec::new(); width * height],
        }
    }

    pub fn rebuild(&mut self, units: &SlotMap<UnitKey, Unit>) {
        if self.cells.len() != self.width * self.height {
            self.cells = vec![SmallVec::new(); self.width * self.height];
        }
        for cell in &mut self.cells {
            cell.clear();
        }
        for (key, unit) in units.iter() {
            if let Some(idx) = self.index(unit.pos) {
                self.cells[idx].push(key);
            }
        }
    }

    pub fn units_at(&self, ax: Axial) -> &[UnitKey] {
        self.index(ax)
            .and_then(|idx| self.cells.get(idx))
            .map(|cell| cell.as_slice())
            .unwrap_or(&[])
    }

    pub fn has_unit_at(&self, ax: Axial) -> bool {
        !self.units_at(ax).is_empty()
    }

    pub fn units_adjacent(&self, ax: Axial) -> impl Iterator<Item = UnitKey> + '_ {
        neighbors(ax)
            .into_iter()
            .flat_map(|neighbor| self.units_at(neighbor).iter().copied())
    }

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
