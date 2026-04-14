use super::collision::{ray_geometry, Geometry, Hit};
use super::hex::{hexes_along_ray, hexes_in_radius, HEX_RADIUS};
use super::index::SpatialIndex;
use super::spatial::Vec2;
use crate::v2::hex::{self, Axial};
use crate::v2::state::EntityKey;

/// An entity record for spatial queries. Callers provide a closure that
/// maps EntityKey → Option<(Vec2, Option<Geometry>)> to avoid coupling
/// the query module to the entity storage format.
#[derive(Debug, Clone, Copy)]
pub struct EntityHit {
    pub key: EntityKey,
    pub hit: Hit,
}

// ---------------------------------------------------------------------------
// query_radius
// ---------------------------------------------------------------------------

/// Find all entities within `dist` meters of `center`.
/// Uses hex culling: only checks entities in hexes that could contain results.
/// Returns entity keys (no ordering guarantee).
pub fn query_radius<F>(
    index: &SpatialIndex,
    center: Vec2,
    dist: f32,
    entity_pos: F,
) -> Vec<EntityKey>
where
    F: Fn(EntityKey) -> Option<Vec2>,
{
    let dist_sq = dist * dist;
    let candidate_hexes = hexes_in_radius(center, dist);
    let mut result = Vec::new();

    for hex in candidate_hexes {
        for &key in index.entities_at(hex) {
            if let Some(pos) = entity_pos(key) {
                let d = (pos - center).length_squared();
                if d <= dist_sq {
                    result.push(key);
                }
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// query_ring
// ---------------------------------------------------------------------------

/// Find all entities within exactly `n` hex rings of `center_hex`.
/// Ring 0 = the center hex itself. Ring 1 = the 6 adjacent hexes. Etc.
pub fn query_ring(index: &SpatialIndex, center_hex: Axial, n: i32) -> Vec<EntityKey> {
    let hexes = if n == 0 {
        vec![center_hex]
    } else {
        hex::within_radius(center_hex, n)
    };

    let mut result = Vec::new();
    for hex in hexes {
        result.extend_from_slice(index.entities_at(hex));
    }
    result
}

// ---------------------------------------------------------------------------
// query_ray
// ---------------------------------------------------------------------------

/// Cast a ray and return all entity intersections, ordered by distance.
/// Hex culling: only tests entities in hexes along the ray path.
pub fn query_ray<F>(
    index: &SpatialIndex,
    origin: Vec2,
    direction: Vec2,
    max_dist: f32,
    entity_geom: F,
) -> Vec<EntityHit>
where
    F: Fn(EntityKey) -> Option<Geometry>,
{
    let candidate_hexes = hexes_along_ray(origin, direction, max_dist);
    let dir = direction.normalize();
    let mut hits = Vec::new();

    for hex in candidate_hexes {
        for &key in index.entities_at(hex) {
            if let Some(geom) = entity_geom(key) {
                if let Some(hit) = ray_geometry(origin, dir, &geom) {
                    if hit.t <= max_dist && hit.t >= 0.0 {
                        hits.push(EntityHit { key, hit });
                    }
                }
            }
        }
    }

    // Sort by distance
    hits.sort_by(|a, b| a.hit.t.partial_cmp(&b.hit.t).unwrap_or(std::cmp::Ordering::Equal));

    // Deduplicate (entity might appear in multiple hex cells along the ray)
    hits.dedup_by_key(|h| h.key);

    hits
}

// ---------------------------------------------------------------------------
// query_arc
// ---------------------------------------------------------------------------

/// Sample a parabolic arc and return all entity intersections, ordered by
/// distance along the arc. The arc is defined by initial velocity and gravity.
///
/// Returns ALL intersections — callers walk the list and decide when the
/// projectile stops (e.g., after energy is depleted).
pub fn query_arc<F>(
    index: &SpatialIndex,
    origin: Vec2,
    velocity: Vec2,
    gravity: f32,
    max_time: f32,
    entity_geom: F,
) -> Vec<EntityHit>
where
    F: Fn(EntityKey) -> Option<Geometry>,
{
    // Sample the arc at regular intervals and cast short rays between samples
    let dt = 0.05; // 50ms time steps
    let steps = (max_time / dt).ceil() as usize;

    let mut hits = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut arc_dist = 0.0_f32;

    let mut prev = origin;
    for i in 1..=steps {
        let t = (i as f32 * dt).min(max_time);
        // Parabolic position: p = origin + velocity * t + 0.5 * gravity * t^2
        // In 2D, gravity only affects the vertical (y) component in this simplified model.
        // For a true 3D arc, the caller would use Vec3. Here we project to 2D.
        let current = Vec2::new(
            origin.x + velocity.x * t,
            origin.y + velocity.y * t - 0.5 * gravity * t * t,
        );

        let seg_dir = current - prev;
        let seg_len = seg_dir.length();
        if seg_len < 1e-6 {
            prev = current;
            continue;
        }
        let seg_norm = seg_dir * (1.0 / seg_len);

        // Check hexes near this segment
        let mid = Vec2::new(
            (prev.x + current.x) / 2.0,
            (prev.y + current.y) / 2.0,
        );
        let candidate_hexes = hexes_in_radius(mid, seg_len / 2.0 + HEX_RADIUS);

        for hex in candidate_hexes {
            for &key in index.entities_at(hex) {
                if seen.contains(&key) {
                    continue;
                }
                if let Some(geom) = entity_geom(key) {
                    if let Some(hit) = ray_geometry(prev, seg_norm, &geom) {
                        if hit.t <= seg_len {
                            seen.insert(key);
                            hits.push(EntityHit {
                                key,
                                hit: Hit {
                                    t: arc_dist + hit.t,
                                    point: hit.point,
                                    normal: hit.normal,
                                },
                            });
                        }
                    }
                }
            }
        }

        arc_dist += seg_len;
        prev = current;
    }

    // Sort by arc distance
    hits.sort_by(|a, b| a.hit.t.partial_cmp(&b.hit.t).unwrap_or(std::cmp::Ordering::Equal));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::collision::{Circle, LineSegment};
    use super::super::hex::hex_to_world;
    use slotmap::SlotMap;

    fn setup_index_with_entities(
        sm: &mut SlotMap<EntityKey, Vec2>,
        positions: &[(f32, f32)],
    ) -> (SpatialIndex, Vec<EntityKey>) {
        let mut index = SpatialIndex::new(20, 20);
        let mut keys = Vec::new();
        for &(x, y) in positions {
            let pos = Vec2::new(x, y);
            let hex = super::super::hex::world_to_hex_2d(pos);
            let key = sm.insert(pos);
            index.insert(hex, key);
            keys.push(key);
        }
        (index, keys)
    }

    #[test]
    fn query_radius_finds_nearby() {
        let mut sm = SlotMap::<EntityKey, Vec2>::with_key();
        let (index, keys) = setup_index_with_entities(&mut sm, &[
            (0.0, 0.0),
            (10.0, 0.0),
            (500.0, 500.0), // far away
        ]);

        let result = query_radius(&index, Vec2::ZERO, 50.0, |k| sm.get(k).copied());
        assert!(result.contains(&keys[0]));
        assert!(result.contains(&keys[1]));
        assert!(!result.contains(&keys[2]));
    }

    #[test]
    fn query_ring_center() {
        let mut sm = SlotMap::<EntityKey, Vec2>::with_key();
        let origin_hex = Axial::new(0, 0);
        let key = sm.insert(Vec2::ZERO);
        let mut index = SpatialIndex::new(20, 20);
        index.insert(origin_hex, key);

        let result = query_ring(&index, origin_hex, 0);
        assert!(result.contains(&key));
    }

    #[test]
    fn query_ray_hits_wall() {
        let mut sm = SlotMap::<EntityKey, Vec2>::with_key();
        let wall_pos = Vec2::new(50.0, 0.0);
        let wall_hex = super::super::hex::world_to_hex_2d(wall_pos);
        let wall_key = sm.insert(wall_pos);

        let mut index = SpatialIndex::new(20, 20);
        index.insert(wall_hex, wall_key);

        let wall_geom = Geometry::Segment(LineSegment {
            start: Vec2::new(50.0, -20.0),
            end: Vec2::new(50.0, 20.0),
            thickness: 1.0,
        });

        let hits = query_ray(
            &index,
            Vec2::ZERO,
            Vec2::new(1.0, 0.0),
            200.0,
            |k| {
                if k == wall_key {
                    Some(wall_geom)
                } else {
                    None
                }
            },
        );

        assert!(!hits.is_empty());
        assert_eq!(hits[0].key, wall_key);
        assert!((hits[0].hit.t - 50.0).abs() < 2.0);
    }

    #[test]
    fn query_arc_through_two_walls() {
        let mut sm = SlotMap::<EntityKey, Vec2>::with_key();

        // Two walls at x=30 and x=60
        let w1_pos = Vec2::new(30.0, 0.0);
        let w2_pos = Vec2::new(60.0, 0.0);
        let w1_hex = super::super::hex::world_to_hex_2d(w1_pos);
        let w2_hex = super::super::hex::world_to_hex_2d(w2_pos);
        let w1_key = sm.insert(w1_pos);
        let w2_key = sm.insert(w2_pos);

        let mut index = SpatialIndex::new(20, 20);
        index.insert(w1_hex, w1_key);
        index.insert(w2_hex, w2_key);

        let w1_geom = Geometry::Segment(LineSegment {
            start: Vec2::new(30.0, -20.0),
            end: Vec2::new(30.0, 20.0),
            thickness: 1.0,
        });
        let w2_geom = Geometry::Segment(LineSegment {
            start: Vec2::new(60.0, -20.0),
            end: Vec2::new(60.0, 20.0),
            thickness: 1.0,
        });

        let hits = query_arc(
            &index,
            Vec2::ZERO,
            Vec2::new(100.0, 0.0), // horizontal velocity
            0.0,                    // no gravity (flat trajectory)
            2.0,                    // 2 seconds
            |k| {
                if k == w1_key {
                    Some(w1_geom)
                } else if k == w2_key {
                    Some(w2_geom)
                } else {
                    None
                }
            },
        );

        assert_eq!(hits.len(), 2, "should hit both walls");
        assert_eq!(hits[0].key, w1_key, "first wall should be hit first");
        assert_eq!(hits[1].key, w2_key, "second wall should be hit second");
        assert!(hits[0].hit.t < hits[1].hit.t, "hits should be in distance order");
    }
}
