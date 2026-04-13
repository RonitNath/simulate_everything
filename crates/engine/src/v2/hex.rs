use serde::{Deserialize, Serialize};

/// Axial hex coordinate (flat-top orientation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Axial {
    pub q: i32,
    pub r: i32,
}

impl Axial {
    pub fn new(q: i32, r: i32) -> Self {
        Self { q, r }
    }
}

/// Convert even-r offset (row, col) to axial coordinates.
pub fn offset_to_axial(row: i32, col: i32) -> Axial {
    let q = col - (row - (row & 1)) / 2;
    let r = row;
    Axial { q, r }
}

/// Convert axial to even-r offset, returning (row, col).
pub fn axial_to_offset(ax: Axial) -> (i32, i32) {
    let col = ax.q + (ax.r - (ax.r & 1)) / 2;
    let row = ax.r;
    (row, col)
}

// Flat-top hex neighbor directions in axial coordinates.
// Order: NE=0, E=1, SE=2, SW=3, W=4, NW=5
const DIRECTIONS: [(i32, i32); 6] = [
    (1, -1), // NE edge 0
    (1, 0),  // E  edge 1
    (0, 1),  // SE edge 2
    (-1, 1), // SW edge 3
    (-1, 0), // W  edge 4
    (0, -1), // NW edge 5
];

/// The 6 neighbors of a hex, in consistent order NE=0, E=1, SE=2, SW=3, W=4, NW=5.
pub fn neighbors(ax: Axial) -> [Axial; 6] {
    DIRECTIONS.map(|(dq, dr)| Axial {
        q: ax.q + dq,
        r: ax.r + dr,
    })
}

/// Hex distance between two axial coordinates.
pub fn distance(a: Axial, b: Axial) -> i32 {
    let dq = (a.q - b.q).abs();
    let dr = (a.r - b.r).abs();
    let ds = ((a.q + a.r) - (b.q + b.r)).abs();
    (dq + dr + ds) / 2
}

/// Returns the edge index (0-5) from a to b if they are adjacent, None otherwise.
/// Edge e from a is the neighbor at index e. The opposite edge from b is (e + 3) % 6.
pub fn shared_edge(a: Axial, b: Axial) -> Option<u8> {
    let dq = b.q - a.q;
    let dr = b.r - a.r;
    DIRECTIONS
        .iter()
        .position(|&(ddq, ddr)| ddq == dq && ddr == dr)
        .map(|i| i as u8)
}

/// All hexes within hex distance `radius` of `center` (inclusive).
pub fn within_radius(center: Axial, radius: i32) -> Vec<Axial> {
    let mut result = Vec::new();
    for q in -radius..=radius {
        let r_min = (-radius).max(-q - radius);
        let r_max = radius.min(-q + radius);
        for r in r_min..=r_max {
            result.push(Axial {
                q: center.q + q,
                r: center.r + r,
            });
        }
    }
    result
}

/// All hexes at exactly hex distance `radius` from `center`.
pub fn ring(center: Axial, radius: i32) -> Vec<Axial> {
    if radius == 0 {
        return vec![center];
    }
    let mut result = Vec::new();
    // Start at the "6 o'clock" position and walk around
    let start_dir = DIRECTIONS[4]; // W direction
    let mut current = Axial {
        q: center.q + start_dir.0 * radius,
        r: center.r + start_dir.1 * radius,
    };
    // Walk along each of 6 sides
    for side in 0..6 {
        let (dq, dr) = DIRECTIONS[(side + 2) % 6];
        for _ in 0..radius {
            result.push(current);
            current = Axial {
                q: current.q + dq,
                r: current.r + dr,
            };
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbors_returns_six() {
        let a = Axial::new(3, -2);
        assert_eq!(neighbors(a).len(), 6);
    }

    #[test]
    fn distance_self_is_zero() {
        let a = Axial::new(5, -3);
        assert_eq!(distance(a, a), 0);
    }

    #[test]
    fn distance_symmetric() {
        let a = Axial::new(1, 2);
        let b = Axial::new(-3, 5);
        assert_eq!(distance(a, b), distance(b, a));
    }

    #[test]
    fn distance_adjacent_is_one() {
        let a = Axial::new(0, 0);
        for nb in neighbors(a) {
            assert_eq!(distance(a, nb), 1);
        }
    }

    #[test]
    fn shared_edge_adjacent_some() {
        let a = Axial::new(0, 0);
        for nb in neighbors(a) {
            assert!(shared_edge(a, nb).is_some());
        }
    }

    #[test]
    fn shared_edge_non_adjacent_none() {
        let a = Axial::new(0, 0);
        let far = Axial::new(3, 3);
        assert!(shared_edge(a, far).is_none());
    }

    #[test]
    fn shared_edge_opposite_edges() {
        let a = Axial::new(0, 0);
        for nb in neighbors(a) {
            let e_ab = shared_edge(a, nb).unwrap();
            let e_ba = shared_edge(nb, a).unwrap();
            assert_eq!((e_ab + 3) % 6, e_ba);
        }
    }

    #[test]
    fn offset_axial_roundtrip() {
        for row in -10..=10 {
            for col in -10..=10 {
                let ax = offset_to_axial(row, col);
                let (r2, c2) = axial_to_offset(ax);
                assert_eq!((row, col), (r2, c2));
            }
        }
    }

    #[test]
    fn ring_sizes() {
        let center = Axial::new(0, 0);
        assert_eq!(ring(center, 1).len(), 6);
        assert_eq!(ring(center, 2).len(), 12);
        assert_eq!(ring(center, 3).len(), 18);
    }

    #[test]
    fn within_radius_size() {
        let center = Axial::new(0, 0);
        assert_eq!(within_radius(center, 3).len(), 37);
    }
}
