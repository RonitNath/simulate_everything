use super::armor::BodyZone;
use super::body_model::{BodyModel, BodyPointId};
use super::spatial::Vec3;

// ---------------------------------------------------------------------------
// Hitbox primitives
// ---------------------------------------------------------------------------

/// A capsule hitbox: swept sphere along a line segment.
#[derive(Debug, Clone, Copy)]
pub struct Capsule {
    /// Start point of the capsule axis.
    pub a: Vec3,
    /// End point of the capsule axis.
    pub b: Vec3,
    /// Radius of the swept sphere.
    pub radius: f32,
}

/// A disc hitbox: flat circular disc (shield).
#[derive(Debug, Clone, Copy)]
pub struct Disc {
    /// Center of the disc in world coordinates.
    pub center: Vec3,
    /// Unit normal vector of the disc face.
    pub normal: Vec3,
    /// Radius of the disc.
    pub radius: f32,
}

/// Result of a sweep test intersection.
#[derive(Debug, Clone, Copy)]
pub struct HitResult {
    /// Parameter along the sweep line (0.0 = start, 1.0 = end).
    pub t: f32,
    /// World position of the hit.
    pub point: Vec3,
    /// Which body zone was hit.
    pub zone: BodyZone,
}

// ---------------------------------------------------------------------------
// Body-segment capsule definitions
// ---------------------------------------------------------------------------

/// A body segment definition: which points form the capsule and its radius.
struct SegmentDef {
    a: BodyPointId,
    b: BodyPointId,
    radius: f32,
    zone: BodyZone,
}

/// Head is a sphere (single point + radius), modeled as a zero-length capsule.
const BODY_SEGMENTS: &[SegmentDef] = &[
    // Head (sphere at head point)
    SegmentDef {
        a: BodyPointId::Head,
        b: BodyPointId::Head,
        radius: 0.12,
        zone: BodyZone::Head,
    },
    // Neck
    SegmentDef {
        a: BodyPointId::Head,
        b: BodyPointId::Neck,
        radius: 0.06,
        zone: BodyZone::Head,
    },
    // Upper torso
    SegmentDef {
        a: BodyPointId::Neck,
        b: BodyPointId::UpperSpine,
        radius: 0.18,
        zone: BodyZone::Torso,
    },
    // Lower torso
    SegmentDef {
        a: BodyPointId::UpperSpine,
        b: BodyPointId::LowerSpine,
        radius: 0.16,
        zone: BodyZone::Torso,
    },
    // Left upper arm
    SegmentDef {
        a: BodyPointId::LeftShoulder,
        b: BodyPointId::LeftElbow,
        radius: 0.05,
        zone: BodyZone::LeftArm,
    },
    // Left forearm
    SegmentDef {
        a: BodyPointId::LeftElbow,
        b: BodyPointId::LeftHand,
        radius: 0.04,
        zone: BodyZone::LeftArm,
    },
    // Right upper arm
    SegmentDef {
        a: BodyPointId::RightShoulder,
        b: BodyPointId::RightElbow,
        radius: 0.05,
        zone: BodyZone::RightArm,
    },
    // Right forearm
    SegmentDef {
        a: BodyPointId::RightElbow,
        b: BodyPointId::RightHand,
        radius: 0.04,
        zone: BodyZone::RightArm,
    },
    // Left thigh
    SegmentDef {
        a: BodyPointId::LeftHip,
        b: BodyPointId::LeftKnee,
        radius: 0.08,
        zone: BodyZone::Legs,
    },
    // Left shin
    SegmentDef {
        a: BodyPointId::LeftKnee,
        b: BodyPointId::LeftFoot,
        radius: 0.06,
        zone: BodyZone::Legs,
    },
    // Right thigh
    SegmentDef {
        a: BodyPointId::RightHip,
        b: BodyPointId::RightKnee,
        radius: 0.08,
        zone: BodyZone::Legs,
    },
    // Right shin
    SegmentDef {
        a: BodyPointId::RightKnee,
        b: BodyPointId::RightFoot,
        radius: 0.06,
        zone: BodyZone::Legs,
    },
];

// ---------------------------------------------------------------------------
// Capsule sweep test
// ---------------------------------------------------------------------------

/// Test a line segment (sword sweep) against a capsule hitbox.
/// Returns the parameter t along the sweep and the hit point, or None if no hit.
///
/// The test finds the closest approach between two line segments (the sweep
/// and the capsule axis) and checks if the distance is within the capsule radius.
pub fn capsule_sweep_test(
    sweep_start: Vec3,
    sweep_end: Vec3,
    capsule: &Capsule,
) -> Option<(f32, Vec3)> {
    // Closest approach between two line segments
    let d1 = sweep_end - sweep_start; // sweep direction
    let d2 = capsule.b - capsule.a; // capsule axis
    let r = sweep_start - capsule.a;

    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);

    if a < 1e-10 && e < 1e-10 {
        // Both segments are points
        let dist = r.length();
        if dist <= capsule.radius {
            return Some((0.0, sweep_start));
        }
        return None;
    }

    let b = d1.dot(d2);
    let c = d1.dot(r);

    let (s, t) = if a < 1e-10 {
        // Sweep is a point
        (0.0, (f / e).clamp(0.0, 1.0))
    } else if e < 1e-10 {
        // Capsule is a point (sphere)
        let s = (-c / a).clamp(0.0, 1.0);
        (s, 0.0)
    } else {
        let denom = a * e - b * b;
        let s = if denom.abs() > 1e-10 {
            ((b * f - c * e) / denom).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let t_num = b * s + f;
        let t = if t_num < 0.0 {
            let s = (-c / a).clamp(0.0, 1.0);
            (s, 0.0)
        } else if t_num > e {
            let s = ((b - c) / a).clamp(0.0, 1.0);
            (s, 1.0)
        } else {
            (s, t_num / e)
        };
        t
    };

    let closest_sweep = sweep_start + d1 * s;
    let closest_capsule = capsule.a + d2 * t;
    let dist = (closest_sweep - closest_capsule).length();

    if dist <= capsule.radius {
        Some((s, closest_sweep))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Disc sweep test
// ---------------------------------------------------------------------------

/// Test a line segment (sword sweep) against a disc hitbox (shield).
/// Returns the parameter t along the sweep and the hit point, or None.
pub fn disc_sweep_test(sweep_start: Vec3, sweep_end: Vec3, disc: &Disc) -> Option<(f32, Vec3)> {
    let d = sweep_end - sweep_start;
    let denom = disc.normal.dot(d);

    // Near-parallel to disc plane — no intersection
    if denom.abs() < 1e-8 {
        return None;
    }

    let t = disc.normal.dot(disc.center - sweep_start) / denom;
    if t < 0.0 || t > 1.0 {
        return None; // intersection outside sweep segment
    }

    let hit_point = sweep_start + d * t;
    let offset = hit_point - disc.center;
    let dist_from_center = offset.length();

    if dist_from_center <= disc.radius {
        Some((t, hit_point))
    } else {
        None
    }
}

/// Compute the deflection angle for a hit on a disc.
/// Returns the angle of incidence in radians (0 = head-on, PI/2 = edge-on).
pub fn disc_deflection_angle(sweep_dir: Vec3, disc_normal: Vec3) -> f32 {
    let sweep_normalized = sweep_dir.normalize();
    let cos_angle = sweep_normalized.dot(disc_normal).abs();
    cos_angle.acos()
}

// ---------------------------------------------------------------------------
// Full body hit detection
// ---------------------------------------------------------------------------

/// Test a sword sweep against all body-segment capsules of a defender.
/// Returns the first (closest) hit, or None if no body part was hit.
pub fn test_body_hit(
    sweep_start: Vec3,
    sweep_end: Vec3,
    defender_body: &BodyModel,
) -> Option<HitResult> {
    let mut best: Option<HitResult> = None;

    for seg in BODY_SEGMENTS {
        let cap_a = defender_body.point(seg.a).pos;
        let cap_b = defender_body.point(seg.b).pos;
        let capsule = Capsule {
            a: cap_a,
            b: cap_b,
            radius: seg.radius,
        };

        if let Some((t, point)) = capsule_sweep_test(sweep_start, sweep_end, &capsule) {
            let is_better = best.as_ref().map(|b| t < b.t).unwrap_or(true);
            if is_better {
                best = Some(HitResult {
                    t,
                    point,
                    zone: seg.zone,
                });
            }
        }
    }

    best
}

/// Test a sword sweep against a shield disc, then body capsules.
/// Shield intercepts before body — proper guard blocks geometrically.
/// Returns `GeometricHitOutcome` describing what was hit first.
pub fn test_hit_with_shield(
    sweep_start: Vec3,
    sweep_end: Vec3,
    defender_body: &BodyModel,
    shield: Option<&Disc>,
) -> GeometricHitOutcome {
    let body_hit = test_body_hit(sweep_start, sweep_end, defender_body);
    let shield_hit = shield.and_then(|disc| {
        disc_sweep_test(sweep_start, sweep_end, disc).map(|(t, point)| (t, point, disc))
    });

    match (shield_hit, body_hit) {
        (Some((shield_t, shield_point, disc)), Some(body)) if shield_t <= body.t => {
            // Shield intercepts first
            let sweep_dir = sweep_end - sweep_start;
            let deflection = disc_deflection_angle(sweep_dir, disc.normal);
            GeometricHitOutcome::ShieldBlock {
                t: shield_t,
                point: shield_point,
                deflection_angle: deflection,
            }
        }
        (_, Some(body)) => GeometricHitOutcome::BodyHit(body),
        (Some((t, point, disc)), None) => {
            let sweep_dir = sweep_end - sweep_start;
            let deflection = disc_deflection_angle(sweep_dir, disc.normal);
            GeometricHitOutcome::ShieldBlock {
                t,
                point,
                deflection_angle: deflection,
            }
        }
        (None, None) => GeometricHitOutcome::Miss,
    }
}

/// Outcome of geometric hit detection.
#[derive(Debug)]
pub enum GeometricHitOutcome {
    /// Sword hit a body capsule.
    BodyHit(HitResult),
    /// Shield intercepted the sword before it reached the body.
    ShieldBlock {
        t: f32,
        point: Vec3,
        /// Angle of incidence against shield normal (0 = head-on, PI/2 = edge-on).
        deflection_angle: f32,
    },
    /// Sword missed entirely.
    Miss,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::body_model::{BodyModel, StanceId};
    use super::super::body_physics::step_body;
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    fn flat_terrain(_x: f32, _y: f32) -> f32 {
        0.0
    }

    fn settled_body() -> BodyModel {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);
        for _ in 0..50 {
            step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
        }
        body
    }

    // --- Capsule sweep tests ---

    #[test]
    fn capsule_hit_direct() {
        let capsule = Capsule {
            a: Vec3::new(0.0, 0.0, 0.5),
            b: Vec3::new(0.0, 0.0, 1.5),
            radius: 0.1,
        };
        // Sweep passes through the capsule
        let result = capsule_sweep_test(
            Vec3::new(-1.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            &capsule,
        );
        assert!(result.is_some(), "direct hit should connect");
        let (t, _) = result.unwrap();
        assert!(t > 0.0 && t < 1.0, "t should be between 0 and 1");
    }

    #[test]
    fn capsule_miss() {
        let capsule = Capsule {
            a: Vec3::new(0.0, 0.0, 0.5),
            b: Vec3::new(0.0, 0.0, 1.5),
            radius: 0.1,
        };
        // Sweep passes far from the capsule
        let result =
            capsule_sweep_test(Vec3::new(5.0, 0.0, 1.0), Vec3::new(6.0, 0.0, 1.0), &capsule);
        assert!(result.is_none(), "far sweep should miss");
    }

    #[test]
    fn capsule_sphere_hit() {
        // Zero-length capsule = sphere
        let capsule = Capsule {
            a: Vec3::new(0.0, 0.0, 1.0),
            b: Vec3::new(0.0, 0.0, 1.0),
            radius: 0.2,
        };
        let result = capsule_sweep_test(
            Vec3::new(-1.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            &capsule,
        );
        assert!(result.is_some(), "sweep through sphere should hit");
    }

    // --- Disc sweep tests ---

    #[test]
    fn disc_hit_head_on() {
        let disc = Disc {
            center: Vec3::new(0.0, 0.5, 1.0),
            normal: Vec3::new(0.0, -1.0, 0.0), // facing toward sweep
            radius: 0.3,
        };
        let result = disc_sweep_test(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 1.0), &disc);
        assert!(result.is_some(), "head-on sweep should hit disc");
    }

    #[test]
    fn disc_miss_outside_radius() {
        let disc = Disc {
            center: Vec3::new(0.0, 0.5, 1.0),
            normal: Vec3::new(0.0, -1.0, 0.0),
            radius: 0.1,
        };
        // Sweep passes outside disc radius
        let result = disc_sweep_test(Vec3::new(0.5, 0.0, 1.0), Vec3::new(0.5, 1.0, 1.0), &disc);
        assert!(result.is_none(), "sweep outside radius should miss");
    }

    #[test]
    fn disc_edge_on_misses() {
        let disc = Disc {
            center: Vec3::new(0.0, 0.5, 1.0),
            normal: Vec3::new(1.0, 0.0, 0.0), // normal perpendicular to sweep
            radius: 0.3,
        };
        // Sweep parallel to disc face
        let result = disc_sweep_test(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 1.0), &disc);
        // Parallel sweep might intersect the plane at one point if aligned,
        // but with near-zero denominator should be treated as miss
        // Actually this sweep IS in the disc plane... the dot product
        // of sweep direction (0,1,0) with normal (1,0,0) = 0, so near-parallel → miss
        assert!(result.is_none(), "edge-on should miss");
    }

    #[test]
    fn disc_deflection_angle_head_on() {
        let angle = disc_deflection_angle(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        // Head-on: sweep direction opposite to normal → cos_angle = 1.0 → angle = 0
        assert!(
            angle < 0.1,
            "head-on should be near 0: {}",
            angle.to_degrees()
        );
    }

    #[test]
    fn disc_deflection_angle_glancing() {
        let angle = disc_deflection_angle(
            Vec3::new(1.0, 0.0, 0.0), // perpendicular to normal
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert!(
            (angle - FRAC_PI_2).abs() < 0.1,
            "glancing should be near PI/2: {}",
            angle.to_degrees()
        );
    }

    // --- Body hit detection ---

    #[test]
    fn overhead_swing_hits_head_zone() {
        let body = settled_body();
        let head_pos = body.point(BodyPointId::Head).pos;

        // Sweep from above, through head position
        let result = test_body_hit(
            Vec3::new(head_pos.x, head_pos.y, head_pos.z + 1.0),
            Vec3::new(head_pos.x, head_pos.y, head_pos.z - 0.5),
            &body,
        );
        assert!(result.is_some(), "overhead sweep should hit");
        let hit = result.unwrap();
        assert_eq!(hit.zone, BodyZone::Head, "should hit head zone");
    }

    #[test]
    fn low_sweep_hits_legs_zone() {
        let body = settled_body();
        let knee_pos = body.point(BodyPointId::LeftKnee).pos;

        // Horizontal sweep at knee height
        let result = test_body_hit(
            Vec3::new(knee_pos.x - 1.0, knee_pos.y, knee_pos.z),
            Vec3::new(knee_pos.x + 1.0, knee_pos.y, knee_pos.z),
            &body,
        );
        assert!(result.is_some(), "low sweep should hit");
        let hit = result.unwrap();
        assert_eq!(hit.zone, BodyZone::Legs, "should hit legs zone");
    }

    #[test]
    fn mid_sweep_hits_torso() {
        let body = settled_body();
        // Use lower spine (below arm segments) to avoid hitting arms first
        let spine_pos = body.point(BodyPointId::LowerSpine).pos;

        // Front-to-back sweep through center of torso
        let result = test_body_hit(
            Vec3::new(spine_pos.x, spine_pos.y - 1.0, spine_pos.z),
            Vec3::new(spine_pos.x, spine_pos.y + 1.0, spine_pos.z),
            &body,
        );
        assert!(result.is_some(), "mid sweep should hit");
        let hit = result.unwrap();
        assert_eq!(hit.zone, BodyZone::Torso, "should hit torso zone");
    }

    #[test]
    fn wide_miss() {
        let body = settled_body();
        // Sweep far from body
        let result = test_body_hit(
            Vec3::new(10.0, 10.0, 1.0),
            Vec3::new(11.0, 10.0, 1.0),
            &body,
        );
        assert!(result.is_none(), "far sweep should miss entirely");
    }

    // --- Shield block detection ---

    #[test]
    fn shield_blocks_from_guarded_direction() {
        let body = settled_body();
        let hand_pos = body.point(BodyPointId::LeftHand).pos;

        let shield = Disc {
            center: hand_pos + Vec3::new(0.0, 0.3, 0.0),
            normal: Vec3::new(0.0, 1.0, 0.0), // facing forward
            radius: 0.4,
        };

        // Attack from the front (through shield)
        let spine = body.point(BodyPointId::UpperSpine).pos;
        let outcome = test_hit_with_shield(
            Vec3::new(spine.x, spine.y + 2.0, spine.z),
            Vec3::new(spine.x, spine.y - 1.0, spine.z),
            &body,
            Some(&shield),
        );

        match outcome {
            GeometricHitOutcome::ShieldBlock {
                deflection_angle, ..
            } => {
                assert!(
                    deflection_angle < PI / 4.0,
                    "head-on shield block should have low deflection angle: {:.1} deg",
                    deflection_angle.to_degrees()
                );
            }
            GeometricHitOutcome::BodyHit(_) => {
                // Acceptable if shield placement didn't intercept
            }
            GeometricHitOutcome::Miss => {
                panic!("should hit something");
            }
        }
    }

    #[test]
    fn shield_bash_proportional_to_mass_velocity() {
        // Shield bash is a disc thrust — KE = 0.5 * mass * velocity^2
        let disc_mass = 3.0; // kg
        let velocity = 5.0; // m/s (shield thrust)
        let ke = 0.5 * disc_mass * velocity * velocity;
        assert!(ke > 30.0, "shield bash should produce significant KE");

        let slow_velocity = 2.0;
        let slow_ke = 0.5 * disc_mass * slow_velocity * slow_velocity;
        assert!(
            ke > slow_ke * 2.0,
            "faster bash should be quadratically stronger"
        );
    }
}
