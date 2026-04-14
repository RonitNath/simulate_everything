use std::collections::{BinaryHeap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use moka::sync::Cache;

use super::hex::hex_to_world;
use super::spatial::Vec3;
use crate::v2::hex::{Axial, distance, neighbors};

// ---------------------------------------------------------------------------
// Tunable constants
// ---------------------------------------------------------------------------

/// Maximum path cache entries.
const CACHE_MAX_CAPACITY: u64 = 4096;

/// Cache time-to-live in seconds.
const CACHE_TTL_SECS: u64 = 60;

// ---------------------------------------------------------------------------
// A* on hex graph
// ---------------------------------------------------------------------------

/// Priority queue entry for A* (min-heap via reversed ordering).
#[derive(PartialEq)]
struct AStarEntry {
    priority: f32,
    pos: Axial,
}

impl Eq for AStarEntry {}

impl PartialOrd for AStarEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AStarEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.priority.total_cmp(&self.priority)
    }
}

/// A* pathfinding on the hex graph.
///
/// `edge_cost` returns the movement cost from one hex to another, or `None`
/// if the edge is impassable. This decouples pathfinding from any specific
/// game state representation.
///
/// `is_in_bounds` returns whether a hex coordinate is valid.
///
/// Returns path excluding `from`, including `to`. Empty if unreachable or same.
pub fn find_path_astar<F, B>(from: Axial, to: Axial, edge_cost: F, is_in_bounds: B) -> Vec<Axial>
where
    F: Fn(Axial, Axial) -> Option<f32>,
    B: Fn(Axial) -> bool,
{
    if from == to {
        return vec![];
    }
    if !is_in_bounds(to) {
        return vec![];
    }

    let mut heap: BinaryHeap<AStarEntry> = BinaryHeap::new();
    let mut g_score: HashMap<Axial, f32> = HashMap::new();
    let mut came_from: HashMap<Axial, Axial> = HashMap::new();

    g_score.insert(from, 0.0);
    heap.push(AStarEntry {
        priority: 0.0,
        pos: from,
    });

    while let Some(AStarEntry { pos: current, .. }) = heap.pop() {
        if current == to {
            return reconstruct_path(&came_from, from, to);
        }

        let current_g = g_score[&current];

        for neighbor in neighbors(current) {
            if !is_in_bounds(neighbor) {
                continue;
            }
            let Some(cost) = edge_cost(current, neighbor) else {
                continue;
            };
            let tentative_g = current_g + cost;
            if tentative_g < *g_score.get(&neighbor).unwrap_or(&f32::MAX) {
                g_score.insert(neighbor, tentative_g);
                came_from.insert(neighbor, current);
                // Heuristic: hex distance * minimum possible edge cost (1.0).
                let h = distance(neighbor, to) as f32;
                heap.push(AStarEntry {
                    priority: tentative_g + h,
                    pos: neighbor,
                });
            }
        }
    }

    vec![] // unreachable
}

fn reconstruct_path(came_from: &HashMap<Axial, Axial>, from: Axial, to: Axial) -> Vec<Axial> {
    let mut path = vec![];
    let mut node = to;
    while node != from {
        path.push(node);
        node = came_from[&node];
    }
    path.reverse();
    path
}

// ---------------------------------------------------------------------------
// Path smoothing (string-pulling)
// ---------------------------------------------------------------------------

/// Convert hex waypoints to world-space Vec3 positions.
pub fn hex_path_to_world(hex_path: &[Axial]) -> Vec<Vec3> {
    hex_path.iter().map(|&ax| hex_to_world(ax)).collect()
}

/// String-pulling: remove unnecessary waypoints by checking if a straight
/// line from N-1 to N+1 avoids impassable terrain.
///
/// `is_passable_line` returns true if a straight line between two world
/// positions doesn't cross impassable terrain. The caller implements this
/// using hex ray-casting.
pub fn smooth_path<F>(waypoints: &[Vec3], is_passable_line: F) -> Vec<Vec3>
where
    F: Fn(Vec3, Vec3) -> bool,
{
    if waypoints.len() <= 2 {
        return waypoints.to_vec();
    }

    let mut smoothed = vec![waypoints[0]];
    let mut i = 0;

    while i < waypoints.len() - 1 {
        // Try to skip as far ahead as possible.
        let mut furthest = i + 1;
        for j in (i + 2)..waypoints.len() {
            if is_passable_line(waypoints[i], waypoints[j]) {
                furthest = j;
            } else {
                break;
            }
        }
        smoothed.push(waypoints[furthest]);
        i = furthest;
    }

    smoothed
}

// ---------------------------------------------------------------------------
// Path cache
// ---------------------------------------------------------------------------

/// Cache key for A* results: (source_hex, dest_hex, faction_id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathCacheKey {
    pub from: Axial,
    pub to: Axial,
    pub faction_id: u8,
}

impl Hash for PathCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.from.q.hash(state);
        self.from.r.hash(state);
        self.to.q.hash(state);
        self.to.r.hash(state);
        self.faction_id.hash(state);
    }
}

/// Thread-safe path cache using moka.
#[derive(Clone)]
pub struct PathCache {
    cache: Cache<PathCacheKey, Arc<Vec<Axial>>>,
}

impl Default for PathCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PathCache {
    pub fn new() -> Self {
        let cache = Cache::builder()
            .max_capacity(CACHE_MAX_CAPACITY)
            .time_to_live(std::time::Duration::from_secs(CACHE_TTL_SECS))
            .build();
        Self { cache }
    }

    /// Get a cached path, or compute and cache it.
    pub fn get_or_insert<F, B>(
        &self,
        key: PathCacheKey,
        edge_cost: F,
        is_in_bounds: B,
    ) -> Arc<Vec<Axial>>
    where
        F: Fn(Axial, Axial) -> Option<f32>,
        B: Fn(Axial) -> bool,
    {
        if let Some(cached) = self.cache.get(&key) {
            return cached;
        }
        let path = find_path_astar(key.from, key.to, edge_cost, is_in_bounds);
        let arc = Arc::new(path);
        self.cache.insert(key, arc.clone());
        arc
    }

    /// Invalidate all paths that pass through any hex in the given set.
    /// Used when terrain changes.
    pub fn invalidate_hexes(&self, changed: &[Axial]) {
        // moka doesn't support key iteration easily, so we invalidate
        // by removing entries whose from/to match a changed hex.
        // For broader invalidation (paths passing through changed hexes),
        // we clear the entire cache — cheap at our scale.
        if !changed.is_empty() {
            self.cache.invalidate_all();
        }
    }

    /// Invalidate all cached paths for a specific faction (fog of war update).
    pub fn invalidate_faction(&self, _faction_id: u8) {
        // At current scale, clearing the whole cache is acceptable.
        self.cache.invalidate_all();
    }

    pub fn len(&self) -> u64 {
        self.cache.entry_count()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.entry_count() == 0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex::{distance, offset_to_axial};

    const MAP_W: i32 = 20;
    const MAP_H: i32 = 20;

    fn in_bounds(ax: Axial) -> bool {
        let (row, col) = crate::v2::hex::axial_to_offset(ax);
        row >= 0 && col >= 0 && row < MAP_H && col < MAP_W
    }

    fn flat_cost(_from: Axial, _to: Axial) -> Option<f32> {
        Some(1.0)
    }

    /// Cost function with impassable wall segment at col=5, rows 3..=12.
    fn wall_cost(_from: Axial, to: Axial) -> Option<f32> {
        let (row, col) = crate::v2::hex::axial_to_offset(to);
        if col == 5 && (3..=12).contains(&row) {
            None // impassable
        } else {
            Some(1.0)
        }
    }

    #[test]
    fn astar_same_pos_empty() {
        let pos = offset_to_axial(5, 5);
        let path = find_path_astar(pos, pos, flat_cost, in_bounds);
        assert!(path.is_empty());
    }

    #[test]
    fn astar_ends_at_destination() {
        let from = offset_to_axial(3, 3);
        let to = offset_to_axial(8, 8);
        let path = find_path_astar(from, to, flat_cost, in_bounds);
        assert!(!path.is_empty());
        assert_eq!(*path.last().unwrap(), to);
    }

    #[test]
    fn astar_optimal_on_flat() {
        let from = offset_to_axial(3, 3);
        let to = offset_to_axial(8, 8);
        let path = find_path_astar(from, to, flat_cost, in_bounds);
        let expected = distance(from, to) as usize;
        assert_eq!(
            path.len(),
            expected,
            "path length {} != hex distance {}",
            path.len(),
            expected
        );
    }

    #[test]
    fn astar_each_step_adjacent() {
        let from = offset_to_axial(2, 2);
        let to = offset_to_axial(7, 7);
        let path = find_path_astar(from, to, flat_cost, in_bounds);
        let mut prev = from;
        for &step in &path {
            assert_eq!(
                distance(prev, step),
                1,
                "non-adjacent step: {:?} -> {:?}",
                prev,
                step
            );
            prev = step;
        }
    }

    #[test]
    fn astar_routes_around_wall() {
        let from = offset_to_axial(5, 2);
        let to = offset_to_axial(5, 8);
        let path = find_path_astar(from, to, wall_cost, in_bounds);
        assert!(!path.is_empty(), "should find path around wall");
        assert_eq!(*path.last().unwrap(), to);

        // Should be longer than straight-line distance (wall in the way).
        let straight = distance(from, to) as usize;
        assert!(
            path.len() > straight,
            "path around wall ({}) should be longer than straight ({})",
            path.len(),
            straight
        );

        // No step should land on the wall segment (col=5, rows 3..=12).
        for &step in &path {
            let (row, col) = crate::v2::hex::axial_to_offset(step);
            assert!(
                !(col == 5 && (3..=12).contains(&row)),
                "path should not cross wall: {:?} (row={row}, col={col})",
                step
            );
        }
    }

    #[test]
    fn astar_out_of_bounds_returns_empty() {
        let from = offset_to_axial(5, 5);
        let to = Axial::new(999, 999);
        let path = find_path_astar(from, to, flat_cost, in_bounds);
        assert!(path.is_empty());
    }

    // --- String-pulling tests ---

    #[test]
    fn smooth_straight_line_minimal_waypoints() {
        let waypoints = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(30.0, 0.0, 0.0),
            Vec3::new(40.0, 0.0, 0.0),
        ];
        // All line-of-sight checks pass (no obstacles).
        let smoothed = smooth_path(&waypoints, |_, _| true);
        assert_eq!(
            smoothed.len(),
            2,
            "straight line should reduce to start+end: {:?}",
            smoothed
        );
        assert_eq!(smoothed[0], waypoints[0]);
        assert_eq!(smoothed[1], *waypoints.last().unwrap());
    }

    #[test]
    fn smooth_preserves_corners() {
        let waypoints = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(10.0, 10.0, 0.0), // corner
            Vec3::new(20.0, 10.0, 0.0),
        ];
        // Block line of sight from wp[0] to wp[2] (the corner shortcut).
        let smoothed = smooth_path(&waypoints, |a, b| {
            // Block if trying to skip the corner (from y=0 to y=10 area)
            !((a.y - 0.0).abs() < 0.1 && (b.y - 10.0).abs() < 0.1)
        });
        assert!(
            smoothed.len() > 2,
            "should preserve corner waypoints: {:?}",
            smoothed
        );
    }

    #[test]
    fn smooth_empty_and_single() {
        assert!(smooth_path(&[], |_, _| true).is_empty());
        let single = vec![Vec3::new(5.0, 5.0, 0.0)];
        assert_eq!(smooth_path(&single, |_, _| true).len(), 1);
    }

    // --- Cache tests ---

    #[test]
    fn cache_returns_same_path() {
        let cache = PathCache::new();
        let key = PathCacheKey {
            from: offset_to_axial(3, 3),
            to: offset_to_axial(8, 8),
            faction_id: 0,
        };

        let path1 = cache.get_or_insert(key, flat_cost, in_bounds);
        let path2 = cache.get_or_insert(key, flat_cost, in_bounds);

        // Should be the same Arc (pointer equality).
        assert!(Arc::ptr_eq(&path1, &path2));
    }

    #[test]
    fn cache_invalidation_clears() {
        let cache = PathCache::new();
        let key = PathCacheKey {
            from: offset_to_axial(3, 3),
            to: offset_to_axial(8, 8),
            faction_id: 0,
        };

        let _path = cache.get_or_insert(key, flat_cost, in_bounds);
        cache.cache.run_pending_tasks();
        assert!(!cache.is_empty());

        cache.invalidate_hexes(&[offset_to_axial(5, 5)]);
        // After invalidation, cache should be empty (we clear all for simplicity).
        // Note: moka may need a sync_phase for immediate visibility.
        cache.cache.run_pending_tasks();
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_different_factions_separate() {
        let cache = PathCache::new();
        let key_a = PathCacheKey {
            from: offset_to_axial(3, 3),
            to: offset_to_axial(8, 8),
            faction_id: 0,
        };
        let key_b = PathCacheKey {
            from: offset_to_axial(3, 3),
            to: offset_to_axial(8, 8),
            faction_id: 1,
        };

        let path_a = cache.get_or_insert(key_a, flat_cost, in_bounds);
        let path_b = cache.get_or_insert(key_b, flat_cost, in_bounds);

        // Same path content but different cache entries.
        assert_eq!(*path_a, *path_b);
        // They should NOT be the same Arc (different keys).
        assert!(!Arc::ptr_eq(&path_a, &path_b));
    }
}
