use serde::{Deserialize, Serialize};

use super::spatial::Vec2;

// ---------------------------------------------------------------------------
// Tunable constants
// ---------------------------------------------------------------------------

/// Default spacing between entities in a formation (meters).
const DEFAULT_SPACING: f32 = 20.0;

/// Wedge angle between the two legs of the V-shape (radians).
/// ~60 degrees total, 30 degrees per side.
const WEDGE_HALF_ANGLE: f32 = 0.524; // ~30 degrees

// ---------------------------------------------------------------------------
// Formation types
// ---------------------------------------------------------------------------

/// Formation type for a group of entities moving together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormationType {
    /// Single-file or narrow column. Marching, road travel.
    Column,
    /// Wide front, shallow depth. Battle line, maximizing frontage.
    Line,
    /// V-shape, leader at point. Assault, breaking through.
    Wedge,
    /// Hollow rectangle, all-around facing. Defensive formation.
    Square,
    /// Loose spacing, irregular. Ranged units, skirmishers.
    Skirmish,
}

// ---------------------------------------------------------------------------
// Slot computation — pure function, no mutable state
// ---------------------------------------------------------------------------

/// Compute formation slot offsets relative to center-of-mass.
///
/// Returns a Vec<Vec2> of offsets, one per entity. Offsets are in local space
/// (unrotated). Apply `rotate_slots` to orient by facing direction.
///
/// Slot 0 is the leader position (front of column, center of line, point of wedge).
pub fn compute_slots(formation: FormationType, count: usize, spacing: f32) -> Vec<Vec2> {
    let sp = if spacing > 0.0 {
        spacing
    } else {
        DEFAULT_SPACING
    };

    match formation {
        FormationType::Column => compute_column(count, sp),
        FormationType::Line => compute_line(count, sp),
        FormationType::Wedge => compute_wedge(count, sp),
        FormationType::Square => compute_square(count, sp),
        FormationType::Skirmish => compute_skirmish(count, sp),
    }
}

/// Column: entities arranged in a line, one behind the other.
/// Leader at front (positive y = forward in local space).
fn compute_column(count: usize, spacing: f32) -> Vec<Vec2> {
    (0..count)
        .map(|i| {
            // Leader at y=0, others behind.
            Vec2::new(0.0, -(i as f32) * spacing)
        })
        .collect()
}

/// Line: entities spread across the facing direction (perpendicular to movement).
/// Centered on the formation center.
fn compute_line(count: usize, spacing: f32) -> Vec<Vec2> {
    if count == 0 {
        return vec![];
    }
    let half = (count as f32 - 1.0) / 2.0;
    (0..count)
        .map(|i| {
            let x = (i as f32 - half) * spacing;
            Vec2::new(x, 0.0)
        })
        .collect()
}

/// Wedge: V-shape with leader at the point. Alternates left and right.
fn compute_wedge(count: usize, spacing: f32) -> Vec<Vec2> {
    if count == 0 {
        return vec![];
    }

    let mut slots = Vec::with_capacity(count);
    // Leader at the point.
    slots.push(Vec2::ZERO);

    let sin_a = WEDGE_HALF_ANGLE.sin();
    let cos_a = WEDGE_HALF_ANGLE.cos();

    for i in 1..count {
        // Alternate left (odd) and right (even).
        let rank = ((i + 1) / 2) as f32; // 1, 1, 2, 2, 3, 3, ...
        let dist = rank * spacing;
        let sign = if i % 2 == 1 { -1.0 } else { 1.0 }; // left first

        let x = sign * dist * sin_a;
        let y = -dist * cos_a; // behind the leader
        slots.push(Vec2::new(x, y));
    }

    slots
}

/// Square: hollow rectangle with entities facing outward. Defensive.
/// Entities distributed evenly around the perimeter.
fn compute_square(count: usize, spacing: f32) -> Vec<Vec2> {
    if count == 0 {
        return vec![];
    }
    if count == 1 {
        return vec![Vec2::ZERO];
    }

    // Side length based on count: distribute evenly across 4 sides.
    let per_side = (count as f32 / 4.0).ceil() as usize;
    let half_side = (per_side as f32 * spacing) / 2.0;

    let mut slots = Vec::with_capacity(count);
    let mut placed = 0;

    // Front edge (positive y)
    for i in 0..per_side.min(count - placed) {
        let x = (i as f32 - (per_side as f32 - 1.0) / 2.0) * spacing;
        slots.push(Vec2::new(x, half_side));
        placed += 1;
    }
    // Right edge (positive x)
    for i in 0..per_side.min(count - placed) {
        let y = half_side - (i as f32 + 1.0) * spacing;
        slots.push(Vec2::new(half_side, y));
        placed += 1;
    }
    // Back edge (negative y)
    for i in 0..per_side.min(count - placed) {
        let x = half_side - (i as f32 + 1.0) * spacing;
        slots.push(Vec2::new(x, -half_side));
        placed += 1;
    }
    // Left edge (negative x)
    for i in 0..per_side.min(count - placed) {
        let y = -half_side + (i as f32 + 1.0) * spacing;
        slots.push(Vec2::new(-half_side, y));
        placed += 1;
    }

    slots
}

/// Skirmish: loose irregular spacing. Entities spread out to minimize
/// area-of-effect vulnerability. Uses a grid with jitter derived from index.
fn compute_skirmish(count: usize, spacing: f32) -> Vec<Vec2> {
    if count == 0 {
        return vec![];
    }
    if count == 1 {
        return vec![Vec2::ZERO];
    }

    // Wider spacing than normal formations.
    let wide = spacing * 1.5;
    let cols = (count as f32).sqrt().ceil() as usize;
    let rows = (count + cols - 1) / cols;

    (0..count)
        .map(|i| {
            let row = i / cols;
            let col = i % cols;
            let half_cols = (cols as f32 - 1.0) / 2.0;
            let half_rows = (rows as f32 - 1.0) / 2.0;
            // Simple deterministic jitter from index.
            let jx = ((i * 7 + 3) % 5) as f32 / 5.0 - 0.5;
            let jy = ((i * 11 + 7) % 5) as f32 / 5.0 - 0.5;
            Vec2::new(
                (col as f32 - half_cols) * wide + jx * spacing * 0.3,
                -(row as f32 - half_rows) * wide + jy * spacing * 0.3,
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Rotation
// ---------------------------------------------------------------------------

/// Rotate slot offsets by the formation's facing angle (radians).
/// 0 = facing positive y (north), angles increase clockwise.
pub fn rotate_slots(slots: &[Vec2], facing: f32) -> Vec<Vec2> {
    let cos_f = facing.cos();
    let sin_f = facing.sin();
    slots
        .iter()
        .map(|s| {
            Vec2::new(
                s.x * cos_f - s.y * sin_f,
                s.x * sin_f + s.y * cos_f,
            )
        })
        .collect()
}

/// Compute world-space slot positions given formation center and facing.
pub fn world_slots(
    formation: FormationType,
    count: usize,
    spacing: f32,
    center: Vec2,
    facing: f32,
) -> Vec<Vec2> {
    let local = compute_slots(formation, count, spacing);
    let rotated = rotate_slots(&local, facing);
    rotated
        .into_iter()
        .map(|s| Vec2::new(center.x + s.x, center.y + s.y))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 0.1;

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPS
    }

    // --- Column ---

    #[test]
    fn column_leader_at_front() {
        let slots = compute_slots(FormationType::Column, 5, 20.0);
        assert_eq!(slots.len(), 5);
        // Leader at y=0 (front).
        assert!(approx_eq(slots[0].x, 0.0));
        assert!(approx_eq(slots[0].y, 0.0));
    }

    #[test]
    fn column_entities_line_up_behind() {
        let slots = compute_slots(FormationType::Column, 5, 20.0);
        // All x should be 0 (single file).
        for s in &slots {
            assert!(approx_eq(s.x, 0.0), "column should be single file: x={}", s.x);
        }
        // Each subsequent entity further back.
        for i in 1..slots.len() {
            assert!(
                slots[i].y < slots[i - 1].y,
                "entity {} should be behind {}: y={} vs y={}",
                i,
                i - 1,
                slots[i].y,
                slots[i - 1].y
            );
        }
    }

    #[test]
    fn column_spacing_correct() {
        let spacing = 25.0;
        let slots = compute_slots(FormationType::Column, 3, spacing);
        for i in 1..slots.len() {
            let dist = (slots[i].y - slots[i - 1].y).abs();
            assert!(
                approx_eq(dist, spacing),
                "spacing between {} and {}: {} (expected {})",
                i - 1,
                i,
                dist,
                spacing
            );
        }
    }

    // --- Line ---

    #[test]
    fn line_spread_across_facing() {
        let slots = compute_slots(FormationType::Line, 5, 20.0);
        assert_eq!(slots.len(), 5);
        // All y should be 0 (all at the same depth).
        for s in &slots {
            assert!(approx_eq(s.y, 0.0), "line should be at same depth: y={}", s.y);
        }
    }

    #[test]
    fn line_centered() {
        let slots = compute_slots(FormationType::Line, 5, 20.0);
        // Center of mass x should be ~0.
        let avg_x: f32 = slots.iter().map(|s| s.x).sum::<f32>() / slots.len() as f32;
        assert!(approx_eq(avg_x, 0.0), "line should be centered: avg_x={avg_x}");
    }

    #[test]
    fn line_single_entity_at_origin() {
        let slots = compute_slots(FormationType::Line, 1, 20.0);
        assert_eq!(slots.len(), 1);
        assert!(approx_eq(slots[0].x, 0.0));
        assert!(approx_eq(slots[0].y, 0.0));
    }

    // --- Wedge ---

    #[test]
    fn wedge_leader_at_point() {
        let slots = compute_slots(FormationType::Wedge, 5, 20.0);
        assert_eq!(slots.len(), 5);
        assert!(approx_eq(slots[0].x, 0.0));
        assert!(approx_eq(slots[0].y, 0.0));
    }

    #[test]
    fn wedge_forms_v_shape() {
        let slots = compute_slots(FormationType::Wedge, 5, 20.0);
        // Entities behind the leader (negative y).
        for i in 1..slots.len() {
            assert!(
                slots[i].y < -EPS,
                "entity {} should be behind leader: y={}",
                i,
                slots[i].y
            );
        }
        // Alternating left/right: slot[1] left, slot[2] right (or vice versa).
        // At minimum, they should be on opposite sides of x=0.
        if slots.len() >= 3 {
            assert!(
                slots[1].x * slots[2].x < 0.0,
                "entities should be on opposite sides: x1={}, x2={}",
                slots[1].x,
                slots[2].x
            );
        }
    }

    #[test]
    fn wedge_symmetric() {
        let slots = compute_slots(FormationType::Wedge, 7, 20.0);
        // Pairs at same rank should be symmetric about x=0.
        // slot[1] and slot[2] at rank 1.
        if slots.len() >= 3 {
            assert!(
                approx_eq(slots[1].x.abs(), slots[2].x.abs()),
                "rank 1 not symmetric: {} vs {}",
                slots[1].x,
                slots[2].x
            );
            assert!(
                approx_eq(slots[1].y, slots[2].y),
                "rank 1 y mismatch: {} vs {}",
                slots[1].y,
                slots[2].y
            );
        }
    }

    // --- Rotation ---

    #[test]
    fn rotation_zero_preserves_slots() {
        let slots = compute_slots(FormationType::Column, 3, 20.0);
        let rotated = rotate_slots(&slots, 0.0);
        for (s, r) in slots.iter().zip(rotated.iter()) {
            assert!(approx_eq(s.x, r.x));
            assert!(approx_eq(s.y, r.y));
        }
    }

    #[test]
    fn rotation_90_swaps_axes() {
        use std::f32::consts::FRAC_PI_2;
        let slots = vec![Vec2::new(0.0, 10.0)]; // 10m forward
        let rotated = rotate_slots(&slots, FRAC_PI_2); // 90 degrees
        // After 90° CCW rotation: (0, 10) → (-10, 0).
        assert!(
            approx_eq(rotated[0].x, -10.0),
            "x should be ~-10: {}",
            rotated[0].x
        );
        assert!(
            approx_eq(rotated[0].y, 0.0),
            "y should be ~0: {}",
            rotated[0].y
        );
    }

    #[test]
    fn rotation_changes_facing() {
        use std::f32::consts::PI;
        let slots = compute_slots(FormationType::Column, 3, 20.0);
        let rotated_0 = rotate_slots(&slots, 0.0);
        let rotated_pi = rotate_slots(&slots, PI);
        // 180° rotation: all y's should flip sign.
        for (r0, rp) in rotated_0.iter().zip(rotated_pi.iter()) {
            assert!(
                approx_eq(r0.y, -rp.y),
                "180° should flip y: {} vs {}",
                r0.y,
                rp.y
            );
        }
    }

    // --- World slots ---

    #[test]
    fn world_slots_offset_by_center() {
        let center = Vec2::new(100.0, 200.0);
        let slots = world_slots(FormationType::Line, 3, 20.0, center, 0.0);
        // Center of mass should be near the formation center.
        let avg_x: f32 = slots.iter().map(|s| s.x).sum::<f32>() / slots.len() as f32;
        let avg_y: f32 = slots.iter().map(|s| s.y).sum::<f32>() / slots.len() as f32;
        assert!(approx_eq(avg_x, center.x));
        assert!(approx_eq(avg_y, center.y));
    }

    // --- Edge cases ---

    // --- Square ---

    #[test]
    fn square_distributes_around_perimeter() {
        let slots = compute_slots(FormationType::Square, 8, 20.0);
        assert_eq!(slots.len(), 8);
        // Should have entities on multiple sides (both positive and negative x/y).
        let has_pos_x = slots.iter().any(|s| s.x > EPS);
        let has_neg_x = slots.iter().any(|s| s.x < -EPS);
        assert!(has_pos_x && has_neg_x, "square should span both x sides");
    }

    // --- Skirmish ---

    #[test]
    fn skirmish_has_2d_spread() {
        let skirmish = compute_slots(FormationType::Skirmish, 9, 20.0);
        assert_eq!(skirmish.len(), 9);
        let width = skirmish.iter().map(|s| s.x).fold(f32::NEG_INFINITY, f32::max)
            - skirmish.iter().map(|s| s.x).fold(f32::INFINITY, f32::min);
        let height = skirmish.iter().map(|s| s.y).fold(f32::NEG_INFINITY, f32::max)
            - skirmish.iter().map(|s| s.y).fold(f32::INFINITY, f32::min);
        // Skirmish should spread in both dimensions (unlike Line which is 1D).
        assert!(width > 10.0, "skirmish should have x spread: {width}");
        assert!(height > 10.0, "skirmish should have y spread: {height}");
    }

    // --- Edge cases ---

    #[test]
    fn zero_count_returns_empty() {
        assert!(compute_slots(FormationType::Column, 0, 20.0).is_empty());
        assert!(compute_slots(FormationType::Line, 0, 20.0).is_empty());
        assert!(compute_slots(FormationType::Wedge, 0, 20.0).is_empty());
        assert!(compute_slots(FormationType::Square, 0, 20.0).is_empty());
        assert!(compute_slots(FormationType::Skirmish, 0, 20.0).is_empty());
    }

    #[test]
    fn single_entity_all_formations() {
        for ft in [
            FormationType::Column, FormationType::Line, FormationType::Wedge,
            FormationType::Square, FormationType::Skirmish,
        ] {
            let slots = compute_slots(ft, 1, 20.0);
            assert_eq!(slots.len(), 1, "{ft:?} should produce 1 slot");
            assert!(approx_eq(slots[0].x, 0.0));
            assert!(approx_eq(slots[0].y, 0.0));
        }
    }
}
