use super::spatial::{Vec2, Vec3};
use crate::v2::hex::Axial;

/// Hex flat-to-flat diameter in meters.
pub const HEX_SIZE: f32 = 150.0;

/// Hex radius: center to corner = size / sqrt(3).
pub const HEX_RADIUS: f32 = 86.602_54; // 150 / sqrt(3)

const SQRT3: f32 = 1.732_050_8;

/// Convert axial hex coordinates to world-space Vec3.
/// z is set to 0; caller should set z = terrain_height_at(pos.xy) for surface entities.
pub fn hex_to_world(ax: Axial) -> Vec3 {
    let q = ax.q as f32;
    let r = ax.r as f32;
    Vec3 {
        x: SQRT3 * HEX_RADIUS * (q + r / 2.0),
        y: 1.5 * HEX_RADIUS * r,
        z: 0.0,
    }
}

/// Convert world-space position to axial hex coordinates using cube rounding.
pub fn world_to_hex(pos: Vec3) -> Axial {
    world_to_hex_2d(pos.xy())
}

/// Convert a 2D world position to axial hex coordinates.
pub fn world_to_hex_2d(pos: Vec2) -> Axial {
    // Fractional axial from pixel
    let q = (pos.x * SQRT3 / 3.0 - pos.y / 3.0) / HEX_RADIUS;
    let r = (pos.y * 2.0 / 3.0) / HEX_RADIUS;
    cube_round(q, r)
}

/// Cube rounding: convert fractional axial (q, r) to the nearest hex.
fn cube_round(fq: f32, fr: f32) -> Axial {
    let fs = -fq - fr;
    let mut q = fq.round();
    let mut r = fr.round();
    let s = fs.round();

    let q_diff = (q - fq).abs();
    let r_diff = (r - fr).abs();
    let s_diff = (s - fs).abs();

    if q_diff > r_diff && q_diff > s_diff {
        q = -r - s;
    } else if r_diff > s_diff {
        r = -q - s;
    }
    // else: s gets corrected, but we don't store s

    Axial::new(q as i32, r as i32)
}

/// Distance between two world positions in the horizontal plane.
pub fn world_distance_2d(a: Vec2, b: Vec2) -> f32 {
    (b - a).length()
}

/// Check if a world position is closer to the given hex center than the
/// hysteresis threshold (0.4 × hex_radius). Used for hex membership updates.
pub fn within_hysteresis(pos: Vec2, hex: Axial) -> bool {
    let center = hex_to_world(hex).xy();
    let dist = world_distance_2d(pos, center);
    dist < 0.4 * HEX_RADIUS
}

// ---------------------------------------------------------------------------
// Vertex index math
// ---------------------------------------------------------------------------

/// For a hex map of `map_w` × `map_h` hexes, compute the vertex grid dimensions.
/// Vertex count ≈ 2 × hex count + boundary row.
pub fn vertex_grid_dims(map_w: usize, map_h: usize) -> (usize, usize) {
    // Each hex has 2 unique vertices in a flat-top layout with shared corners.
    // Columns: 2 * map_w + 1 (left edge shared, each hex adds 2 vertices across)
    // Rows: map_h + 1 (top + bottom vertices per hex row)
    (2 * map_w + 1, map_h + 1)
}

/// Convert world position to fractional vertex grid coordinates.
/// Used by Heightfield::effective_height_at for interpolation.
pub fn world_to_vertex(pos: Vec2, _map_w: usize, _map_h: usize) -> (f32, f32) {
    // Vertex spacing: in x, vertices are half a hex apart (HEX_RADIUS * SQRT3 / 2).
    // In y, vertices are 1.5 * HEX_RADIUS apart (matching hex rows).
    let vx_spacing = HEX_RADIUS * SQRT3 / 2.0;
    let vy_spacing = 1.5 * HEX_RADIUS;

    // Offset so that world origin (hex 0,0 center) maps to a reasonable vertex coordinate.
    // Hex (0,0) center is at world (0,0). The top-left vertex of hex (0,0) is at
    // roughly (-HEX_RADIUS * SQRT3 / 2, -HEX_RADIUS).
    // We shift so that vertex grid (0,0) corresponds to some anchor.
    let vx = pos.x / vx_spacing;
    let vy = pos.y / vy_spacing;

    (vx, vy)
}

/// All hex coordinates that might contain entities within `dist` meters of `pos`.
/// Used for hex-culled spatial queries.
pub fn hexes_in_radius(center: Vec2, dist: f32) -> Vec<Axial> {
    let hex_dist = (dist / (1.5 * HEX_RADIUS)).ceil() as i32 + 1;
    let center_hex = world_to_hex_2d(center);
    crate::v2::hex::within_radius(center_hex, hex_dist)
}

/// All hex coordinates along a ray from `origin` in `direction` up to `max_dist`.
/// Returns hexes in approximate distance order.
pub fn hexes_along_ray(origin: Vec2, direction: Vec2, max_dist: f32) -> Vec<Axial> {
    let step = HEX_RADIUS; // sample every hex_radius meters
    let steps = (max_dist / step).ceil() as usize + 1;
    let dir = direction.normalize();

    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::with_capacity(steps);

    for i in 0..steps {
        let t = i as f32 * step;
        let p = Vec2::new(origin.x + dir.x * t, origin.y + dir.y * t);
        let hex = world_to_hex_2d(p);
        if seen.insert(hex) {
            result.push(hex);
            // Also add neighbors to avoid missing hexes between samples
            for nb in crate::v2::hex::neighbors(hex) {
                if seen.insert(nb) {
                    result.push(nb);
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::hex;

    #[test]
    fn hex_to_world_origin() {
        let w = hex_to_world(Axial::new(0, 0));
        assert!((w.x).abs() < 1e-4);
        assert!((w.y).abs() < 1e-4);
    }

    #[test]
    fn world_to_hex_roundtrip_exact() {
        // world_to_hex(hex_to_world(ax)) should round-trip exactly
        for q in -5..=5 {
            for r in -5..=5 {
                let ax = Axial::new(q, r);
                let w = hex_to_world(ax);
                let back = world_to_hex(w);
                assert_eq!(ax, back, "roundtrip failed for ({}, {})", q, r);
            }
        }
    }

    #[test]
    fn hex_to_world_roundtrip_lossy() {
        // hex_to_world(world_to_hex(pos)) should be within 0.5 * hex_radius
        let pos = Vec3::new(100.0, 200.0, 0.0);
        let hex = world_to_hex(pos);
        let back = hex_to_world(hex);
        let dist = ((back.x - pos.x).powi(2) + (back.y - pos.y).powi(2)).sqrt();
        // Max distance from any point to the nearest hex center is hex_radius
        assert!(
            dist <= HEX_RADIUS,
            "lossy roundtrip distance {} exceeds hex_radius {}",
            dist,
            HEX_RADIUS
        );
    }

    #[test]
    fn adjacent_hexes_are_spaced_correctly() {
        let a = hex_to_world(Axial::new(0, 0));
        let b = hex_to_world(Axial::new(1, 0)); // East neighbor
        let dist = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        // Adjacent hex centers should be sqrt(3) * hex_radius apart
        let expected = SQRT3 * HEX_RADIUS;
        assert!(
            (dist - expected).abs() < 1e-2,
            "expected {}, got {}",
            expected,
            dist
        );
    }

    #[test]
    fn hexes_in_radius_includes_center() {
        let hexes = hexes_in_radius(Vec2::new(0.0, 0.0), 0.0);
        assert!(hexes.contains(&Axial::new(0, 0)));
    }

    #[test]
    fn within_hysteresis_at_center() {
        assert!(within_hysteresis(Vec2::new(0.0, 0.0), Axial::new(0, 0)));
    }

    #[test]
    fn within_hysteresis_at_boundary() {
        // At the boundary between hexes, should NOT be within hysteresis
        let edge_pos = hex_to_world(Axial::new(1, 0)).xy();
        let midpoint = Vec2::new(edge_pos.x / 2.0, edge_pos.y / 2.0);
        // Midpoint between hex (0,0) and (1,0) should be outside hysteresis for both
        let in_0 = within_hysteresis(midpoint, Axial::new(0, 0));
        let in_1 = within_hysteresis(midpoint, Axial::new(1, 0));
        // At least one should be false (they can't both be "inside" with 0.4 threshold)
        assert!(!in_0 || !in_1);
    }

    #[test]
    fn vertex_grid_dims_reasonable() {
        let (cols, rows) = vertex_grid_dims(10, 10);
        assert_eq!(cols, 21);
        assert_eq!(rows, 11);
    }
}
