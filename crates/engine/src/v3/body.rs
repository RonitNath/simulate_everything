use std::f32::consts::FRAC_PI_2;

use super::armor::BodyZone;

// ---------------------------------------------------------------------------
// Tunable constants — zone hit thresholds
// ---------------------------------------------------------------------------

/// Weighted threshold lookup for body zones. Each entry is (upper_bound, zone).
/// Ordered ascending. Total range [0.0, 1.0).
const ZONE_THRESHOLDS: [(f32, BodyZone); 5] = [
    (0.08, BodyZone::Head),
    (0.20, BodyZone::LeftArm),
    (0.32, BodyZone::RightArm),
    (0.72, BodyZone::Torso),
    (1.00, BodyZone::Legs),
];

// ---------------------------------------------------------------------------
// Tunable constants — per-zone surface normal offsets
// ---------------------------------------------------------------------------

/// Offset from entity facing to the zone's outward-facing surface normal.
/// Torso faces forward (0), arms face sideways (±π/2), head/legs forward.
fn zone_normal_offset(zone: BodyZone) -> f32 {
    match zone {
        BodyZone::Head => 0.0,
        BodyZone::Torso => 0.0,
        BodyZone::LeftArm => FRAC_PI_2,
        BodyZone::RightArm => -FRAC_PI_2,
        BodyZone::Legs => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Zone lookup
// ---------------------------------------------------------------------------

/// Map a hit location value in [0.0, 1.0) to a body zone via weighted
/// threshold lookup. Zones are weighted by body proportion: torso is the
/// largest target, head the smallest.
///
/// The function signature is the stable API — implementation can swap to
/// continuous distribution later without changing callers.
pub fn zone_for_location(hit: f32) -> BodyZone {
    for &(upper, zone) in &ZONE_THRESHOLDS {
        if hit < upper {
            return zone;
        }
    }
    // Clamp: values >= 1.0 hit legs (last zone)
    BodyZone::Legs
}

// ---------------------------------------------------------------------------
// Surface angle
// ---------------------------------------------------------------------------

/// Compute the angle of incidence between an attack and a body zone's surface.
///
/// Returns a value in [0, π/2]:
/// - 0 = glancing blow (parallel to armor surface → ricochet)
/// - π/2 = perpendicular hit (dead-on → maximum penetration)
///
/// `attack_direction` and `defender_facing` are in radians. The result feeds
/// into `sin(angle)` in the resistance formula.
pub fn surface_angle(attack_direction: f32, defender_facing: f32, zone: BodyZone) -> f32 {
    let offset = zone_normal_offset(zone);
    let surface_normal = defender_facing + offset;

    // Angle between attack direction and surface normal
    let mut diff = (attack_direction - surface_normal) % std::f32::consts::TAU;
    if diff < 0.0 {
        diff += std::f32::consts::TAU;
    }
    if diff > std::f32::consts::PI {
        diff = std::f32::consts::TAU - diff;
    }

    // Convert from angle-from-normal to angle-from-surface:
    // perpendicular to normal (diff ≈ π) means parallel to surface → glancing
    // parallel to normal (diff ≈ 0 or π) means perpendicular to surface → penetrating
    //
    // Actually: attack coming FROM the opposite direction of the normal means head-on.
    // diff ≈ π means attacker faces opposite to defender normal → perpendicular hit.
    // diff ≈ 0 means attacker faces same direction as defender → glancing/behind.
    //
    // angle_from_surface = π/2 when hit is perpendicular (diff = π)
    // angle_from_surface = 0 when hit is glancing (diff = 0 or π/2)
    //
    // We want: perpendicular = π/2, glancing = 0.
    // Map: angle_from_surface = (diff / π) * (π/2) is wrong.
    //
    // Correct physics: the angle from the surface plane.
    // If the attack direction is opposite to the surface normal, the angle of
    // incidence from the surface is π/2 (perpendicular).
    // If the attack direction is perpendicular to the normal (tangent to surface),
    // the angle from surface is 0 (glancing).
    //
    // angle_from_surface = |diff - π|  mapped to [0, π/2]
    // When diff = π → angle_from_surface = π/2 (head-on)
    // When diff = π/2 → angle_from_surface = 0 (glancing)
    // When diff = 0 → behind the defender, treat as glancing
    //
    // Simplified: angle_from_surface = clamp(diff - π/2, 0, π/2)
    (diff - FRAC_PI_2).clamp(0.0, FRAC_PI_2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn zone_lookup_head() {
        assert_eq!(zone_for_location(0.0), BodyZone::Head);
        assert_eq!(zone_for_location(0.07), BodyZone::Head);
    }

    #[test]
    fn zone_lookup_left_arm() {
        assert_eq!(zone_for_location(0.08), BodyZone::LeftArm);
        assert_eq!(zone_for_location(0.15), BodyZone::LeftArm);
    }

    #[test]
    fn zone_lookup_right_arm() {
        assert_eq!(zone_for_location(0.20), BodyZone::RightArm);
        assert_eq!(zone_for_location(0.31), BodyZone::RightArm);
    }

    #[test]
    fn zone_lookup_torso() {
        assert_eq!(zone_for_location(0.32), BodyZone::Torso);
        assert_eq!(zone_for_location(0.50), BodyZone::Torso);
        assert_eq!(zone_for_location(0.71), BodyZone::Torso);
    }

    #[test]
    fn zone_lookup_legs() {
        assert_eq!(zone_for_location(0.72), BodyZone::Legs);
        assert_eq!(zone_for_location(0.99), BodyZone::Legs);
    }

    #[test]
    fn zone_lookup_clamps_above_one() {
        assert_eq!(zone_for_location(1.0), BodyZone::Legs);
        assert_eq!(zone_for_location(1.5), BodyZone::Legs);
    }

    #[test]
    fn zone_lookup_covers_full_range() {
        // Every 0.01 step maps to a valid zone
        let mut i = 0.0;
        while i < 1.0 {
            let _zone = zone_for_location(i);
            i += 0.01;
        }
    }

    #[test]
    fn surface_angle_head_on_is_perpendicular() {
        // Attacker facing north (π), defender facing south (0) → head-on
        let angle = surface_angle(PI, 0.0, BodyZone::Torso);
        assert!(
            (angle - FRAC_PI_2).abs() < 0.01,
            "head-on should be ~π/2, got {angle}"
        );
    }

    #[test]
    fn surface_angle_flanking_is_glancing() {
        // Attacker facing east (π/2), defender facing south (0) → side hit on torso
        let angle = surface_angle(FRAC_PI_2, 0.0, BodyZone::Torso);
        assert!(
            angle < 0.01,
            "pure flank on torso should be glancing, got {angle}"
        );
    }

    #[test]
    fn surface_angle_arm_flank_is_perpendicular() {
        // Attacker from the side → perpendicular hit on the arm that faces sideways
        // LeftArm normal offset = π/2, so surface normal = defender_facing + π/2
        // Attacker at π (facing south into a north-facing defender's left arm)
        // defender_facing = 0, left arm normal = π/2
        // diff = π - π/2 = π/2... that's actually glancing.
        //
        // For a perpendicular hit on left arm: attacker should be at
        // opposite of arm normal = π/2 + π = 3π/2
        let angle = surface_angle(3.0 * FRAC_PI_2, 0.0, BodyZone::LeftArm);
        assert!(
            (angle - FRAC_PI_2).abs() < 0.01,
            "perpendicular to left arm, got {angle}"
        );
    }

    #[test]
    fn surface_angle_always_in_range() {
        for attack in [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2, 2.0 * PI] {
            for defend in [0.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2] {
                for zone in BodyZone::ALL {
                    let a = surface_angle(attack, defend, zone);
                    assert!(
                        (0.0..=FRAC_PI_2 + 0.001).contains(&a),
                        "out of range: attack={attack}, defend={defend}, zone={zone:?}, angle={a}"
                    );
                }
            }
        }
    }
}
