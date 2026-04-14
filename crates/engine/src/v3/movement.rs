use serde::{Deserialize, Serialize};

use super::spatial::Vec3;

// ---------------------------------------------------------------------------
// Tunable constants
// ---------------------------------------------------------------------------

/// Friction damping coefficient for wheeled/pulled vehicles.
/// Velocity *= (1 - FRICTION_DAMPING * dt) each tick when no driving force.
const FRICTION_DAMPING: f32 = 0.3;

/// Minimum speed below which velocity is zeroed (prevents drift).
const SPEED_EPSILON: f32 = 0.01;

// ---------------------------------------------------------------------------
// Mobile component
// ---------------------------------------------------------------------------

/// Mobile component for entities that can move. Replaces V2's discrete
/// teleportation with continuous physics-based movement.
///
/// No `max_speed` field. Speed is derived each tick from base capability
/// and current conditions (terrain, wounds, stamina, encumbrance).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mobile {
    /// Current velocity vector.
    pub vel: Vec3,
    /// Maximum acceleration magnitude (steering force cap).
    pub steering_force: f32,
    /// Collision radius in meters (~10m person, ~30m cart).
    pub radius: f32,
    /// Smoothed path: next waypoint first. Consumed as entity arrives.
    pub waypoints: Vec<Vec3>,
}

impl Mobile {
    pub fn new(steering_force: f32, radius: f32) -> Self {
        Self {
            vel: Vec3::ZERO,
            steering_force,
            radius,
            waypoints: Vec::new(),
        }
    }

    pub fn speed(&self) -> f32 {
        self.vel.length()
    }
}

// ---------------------------------------------------------------------------
// Damping model
// ---------------------------------------------------------------------------

/// Entity locomotion type, derived from what the entity is.
/// Determines how velocity decays when no steering force is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocomotionType {
    /// Humans, animals: maintain velocity via inertia. Must actively brake.
    Legged,
    /// Carts, wagons: friction-based. Passively decelerate without driving force.
    Wheeled,
}

/// Apply damping based on locomotion type.
/// Called when no steering force is applied (or force is below threshold).
pub fn apply_damping(vel: &mut Vec3, locomotion: LocomotionType, dt: f32) {
    match locomotion {
        LocomotionType::Legged => {
            // Inertia: maintain velocity. No passive deceleration.
            // Entity must apply opposing steering force to brake.
        }
        LocomotionType::Wheeled => {
            // Friction: passive deceleration.
            let factor = (1.0 - FRICTION_DAMPING * dt).max(0.0);
            *vel = *vel * factor;
        }
    }
    // Zero out tiny velocities to prevent drift.
    if vel.length() < SPEED_EPSILON {
        *vel = Vec3::ZERO;
    }
}

// ---------------------------------------------------------------------------
// Speed derivation
// ---------------------------------------------------------------------------

/// All factors contributing to derived speed. Each in [0.0, 1.0].
#[derive(Debug, Clone, Copy)]
pub struct SpeedFactors {
    /// Base movement capability in m/s (e.g., swordsman 3.0, ox cart 2.0).
    pub base_capability: f32,
    /// Terrain slope factor: 1.0 on flat, < 1.0 going uphill.
    pub slope_factor: f32,
    /// Surface friction factor: 1.0 / GeoMaterial::friction().
    pub surface_factor: f32,
    /// Encumbrance factor: 1.0 - (weight / max_carry), clamped to [0, 1].
    pub encumbrance_factor: f32,
    /// Wound factor: derived from leg wounds. Fracture = 0.0 (immobile).
    pub wound_factor: f32,
    /// Stamina factor: scales max speed at low stamina.
    pub stamina_factor: f32,
}

impl SpeedFactors {
    /// Compute the derived max speed from all multiplicative factors.
    pub fn derived_speed(&self) -> f32 {
        let speed = self.base_capability
            * self.slope_factor
            * self.surface_factor
            * self.encumbrance_factor
            * self.wound_factor
            * self.stamina_factor;
        speed.max(0.0)
    }
}

/// Compute slope factor from terrain gradient.
/// `gradient` is rise/run (from Heightfield::slope_at). Positive = uphill.
pub fn slope_factor(gradient: f32) -> f32 {
    // Uphill penalty: steeper = slower. Downhill gives no bonus (capped at 1.0).
    // slope_penalty = 2.0 means a 50% grade halves your speed.
    let penalty = 2.0;
    (1.0 - penalty * gradient.max(0.0)).clamp(0.0, 1.0)
}

/// Compute surface factor from material friction.
/// Lower friction = faster (rock 0.9 → factor ~1.11, capped at 1.0).
/// Higher friction = slower (sand 1.3 → factor ~0.77).
pub fn surface_factor(friction: f32) -> f32 {
    if friction < 1e-6 {
        return 1.0;
    }
    (1.0 / friction).clamp(0.0, 1.0)
}

/// Compute encumbrance factor from carried weight vs maximum carry capacity.
pub fn encumbrance_factor(weight: f32, max_carry: f32) -> f32 {
    if max_carry <= 0.0 {
        return 1.0;
    }
    (1.0 - weight / max_carry).clamp(0.0, 1.0)
}

/// Compute wound factor from leg wound severity weight.
/// Uses the wound system's `zone_wound_weight` for BodyZone::Legs.
///
/// leg_wound_weight: sum of severity weights for all leg wounds.
///   - 0.0 = no leg wounds → factor 1.0
///   - >= 1.0 (fracture) → factor 0.0 (immobile)
///   - laceration (~0.4) → factor ~0.6
pub fn wound_factor(leg_wound_weight: f32) -> f32 {
    (1.0 - leg_wound_weight).clamp(0.0, 1.0)
}

/// Compute stamina factor. Low stamina reduces max achievable speed.
/// stamina in [0, 1]. Factor scales linearly with a floor.
pub fn stamina_factor(stamina: f32) -> f32 {
    // Floor of 0.3 — even exhausted entities can walk slowly.
    let floor = 0.3;
    floor + (1.0 - floor) * stamina.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Velocity integration
// ---------------------------------------------------------------------------

/// Integrate velocity and position for one tick.
///
/// 1. Apply steering acceleration (clamped to mobile.steering_force).
/// 2. Update velocity.
/// 3. Clamp speed to derived_speed.
/// 4. Update position.
///
/// Returns the new position.
pub fn integrate(
    pos: Vec3,
    mobile: &mut Mobile,
    steering_accel: Vec3,
    derived_speed: f32,
    dt: f32,
) -> Vec3 {
    // Clamp steering acceleration to max force.
    let accel = {
        let mag = steering_accel.length();
        if mag > mobile.steering_force && mag > 1e-10 {
            steering_accel.normalize() * mobile.steering_force
        } else {
            steering_accel
        }
    };

    // Update velocity.
    mobile.vel = mobile.vel + accel * dt;

    // Clamp speed to derived max.
    let speed = mobile.vel.length();
    if speed > derived_speed && speed > 1e-10 {
        mobile.vel = mobile.vel.normalize() * derived_speed;
    }

    // Zero out tiny velocities.
    if mobile.vel.length() < SPEED_EPSILON {
        mobile.vel = Vec3::ZERO;
    }

    // Update position.
    pos + mobile.vel * dt
}

// ---------------------------------------------------------------------------
// Waypoint consumption
// ---------------------------------------------------------------------------

/// Check if the entity has reached its current waypoint (within tolerance).
/// If so, remove it and return true.
pub fn consume_waypoint(pos: Vec3, mobile: &mut Mobile, tolerance: f32) -> bool {
    if let Some(&wp) = mobile.waypoints.first() {
        let dist = (wp - pos).length();
        if dist < tolerance {
            mobile.waypoints.remove(0);
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 0.01;

    // --- Speed factor tests ---

    #[test]
    fn derived_speed_all_ones() {
        let f = SpeedFactors {
            base_capability: 3.0,
            slope_factor: 1.0,
            surface_factor: 1.0,
            encumbrance_factor: 1.0,
            wound_factor: 1.0,
            stamina_factor: 1.0,
        };
        assert!((f.derived_speed() - 3.0).abs() < EPS);
    }

    #[test]
    fn derived_speed_compounds_multiplicatively() {
        let f = SpeedFactors {
            base_capability: 3.0,
            slope_factor: 0.5,
            surface_factor: 0.8,
            encumbrance_factor: 0.7,
            wound_factor: 0.6,
            stamina_factor: 0.9,
        };
        let expected = 3.0 * 0.5 * 0.8 * 0.7 * 0.6 * 0.9;
        assert!(
            (f.derived_speed() - expected).abs() < EPS,
            "expected {expected}, got {}",
            f.derived_speed()
        );
    }

    #[test]
    fn derived_speed_zero_with_fracture() {
        let f = SpeedFactors {
            base_capability: 3.0,
            slope_factor: 1.0,
            surface_factor: 1.0,
            encumbrance_factor: 1.0,
            wound_factor: 0.0, // fracture
            stamina_factor: 1.0,
        };
        assert!(f.derived_speed().abs() < EPS);
    }

    #[test]
    fn wounded_encumbered_uphill_much_slower() {
        let healthy = SpeedFactors {
            base_capability: 3.0,
            slope_factor: 1.0,
            surface_factor: 1.0,
            encumbrance_factor: 1.0,
            wound_factor: 1.0,
            stamina_factor: 1.0,
        };
        let impaired = SpeedFactors {
            base_capability: 3.0,
            slope_factor: 0.5,       // steep uphill
            surface_factor: 0.77,    // sand
            encumbrance_factor: 0.6, // heavy armor
            wound_factor: 0.6,       // laceration
            stamina_factor: 0.5,     // tired
        };
        assert!(
            impaired.derived_speed() < healthy.derived_speed() * 0.2,
            "impaired {} should be much slower than healthy {}",
            impaired.derived_speed(),
            healthy.derived_speed()
        );
    }

    #[test]
    fn slope_factor_flat() {
        assert!((slope_factor(0.0) - 1.0).abs() < EPS);
    }

    #[test]
    fn slope_factor_steep_uphill() {
        let f = slope_factor(0.5); // 50% grade
        assert!(f < 0.1, "steep uphill should nearly halt: {f}");
    }

    #[test]
    fn slope_factor_downhill_no_bonus() {
        let f = slope_factor(-0.3);
        assert!((f - 1.0).abs() < EPS, "downhill should not exceed 1.0: {f}");
    }

    #[test]
    fn surface_factor_rock_fast() {
        let f = surface_factor(0.9);
        assert!(f > surface_factor(1.3), "rock should be faster than sand");
    }

    #[test]
    fn encumbrance_factor_overloaded() {
        let f = encumbrance_factor(150.0, 100.0);
        assert!(f.abs() < EPS, "overloaded should be immobile");
    }

    #[test]
    fn wound_factor_fracture_immobile() {
        let f = wound_factor(1.0); // fracture weight = 1.0
        assert!(f.abs() < EPS, "fracture should be immobile");
    }

    #[test]
    fn wound_factor_laceration_slowed() {
        let f = wound_factor(0.4); // laceration weight
        assert!((f - 0.6).abs() < EPS, "laceration should reduce to 0.6: {f}");
    }

    #[test]
    fn stamina_factor_full() {
        assert!((stamina_factor(1.0) - 1.0).abs() < EPS);
    }

    #[test]
    fn stamina_factor_exhausted_has_floor() {
        let f = stamina_factor(0.0);
        assert!(f > 0.0, "exhausted should still have floor: {f}");
        assert!((f - 0.3).abs() < EPS, "floor should be 0.3: {f}");
    }

    // --- Integration tests ---

    #[test]
    fn entity_reaches_waypoint_no_oscillation() {
        let mut mobile = Mobile::new(2.0, 10.0);
        let target = Vec3::new(100.0, 0.0, 0.0);
        mobile.waypoints.push(target);

        let mut pos = Vec3::ZERO;
        let derived = 3.0;
        let dt = 1.0;

        // Simulate for many ticks — entity should converge, not oscillate.
        let mut prev_dist = (target - pos).length();
        let mut oscillation_count = 0;

        for _ in 0..200 {
            let steering = if let Some(&wp) = mobile.waypoints.first() {
                super::super::steering::arrive(
                    pos,
                    mobile.vel,
                    wp,
                    mobile.steering_force,
                    derived,
                    50.0,
                )
            } else {
                Vec3::ZERO
            };

            pos = integrate(pos, &mut mobile, steering, derived, dt);
            consume_waypoint(pos, &mut mobile, 2.0);

            let dist = (target - pos).length();
            if dist > prev_dist + 0.5 {
                oscillation_count += 1;
            }
            prev_dist = dist;

            if mobile.waypoints.is_empty() {
                break;
            }
        }

        assert!(
            mobile.waypoints.is_empty(),
            "should have consumed waypoint, dist={}",
            (target - pos).length()
        );
        assert!(
            oscillation_count < 3,
            "too many oscillations: {oscillation_count}"
        );
    }

    #[test]
    fn arrive_decelerates_smoothly() {
        let mut mobile = Mobile::new(2.0, 10.0);
        let target = Vec3::new(50.0, 0.0, 0.0);
        mobile.waypoints.push(target);

        let mut pos = Vec3::ZERO;
        let derived = 3.0;
        let dt = 1.0;

        let mut speeds = Vec::new();

        for _ in 0..100 {
            let steering = if let Some(&wp) = mobile.waypoints.first() {
                super::super::steering::arrive(
                    pos,
                    mobile.vel,
                    wp,
                    mobile.steering_force,
                    derived,
                    50.0,
                )
            } else {
                Vec3::ZERO
            };

            pos = integrate(pos, &mut mobile, steering, derived, dt);
            speeds.push(mobile.speed());
            consume_waypoint(pos, &mut mobile, 2.0);

            if mobile.waypoints.is_empty() {
                break;
            }
        }

        // Speed should generally decrease as entity approaches target.
        // Check that max speed occurs in the first half.
        if speeds.len() > 4 {
            let mid = speeds.len() / 2;
            let max_first_half = speeds[..mid]
                .iter()
                .copied()
                .fold(0.0f32, f32::max);
            let max_second_half = speeds[mid..]
                .iter()
                .copied()
                .fold(0.0f32, f32::max);
            assert!(
                max_first_half >= max_second_half - 0.5,
                "should decelerate: first_half_max={max_first_half}, second_half_max={max_second_half}"
            );
        }
    }

    #[test]
    fn separation_prevents_overlap() {
        use super::super::steering;

        let mut pos_a = Vec3::new(0.0, 0.0, 0.0);
        let mut pos_b = Vec3::new(8.0, 0.0, 0.0);
        let mut mobile_a = Mobile::new(2.0, 5.0);
        let mut mobile_b = Mobile::new(2.0, 5.0);

        let derived = 3.0;
        let dt = 1.0;
        let sep_dist = 15.0;

        for _ in 0..50 {
            let sep_a = steering::separation(pos_a, &[pos_b], sep_dist);
            let sep_b = steering::separation(pos_b, &[pos_a], sep_dist);

            pos_a = integrate(pos_a, &mut mobile_a, sep_a, derived, dt);
            pos_b = integrate(pos_b, &mut mobile_b, sep_b, derived, dt);
        }

        let dist = (pos_b - pos_a).length();
        assert!(
            dist >= sep_dist - 1.0,
            "entities should separate: dist={dist}, target={sep_dist}"
        );
    }

    #[test]
    fn obstacle_avoidance_steers_around_structure() {
        use super::super::collision::{Circle, Geometry};
        use super::super::steering;

        let mut pos = Vec3::new(0.0, 0.0, 0.0);
        let mut mobile = Mobile::new(3.0, 10.0);
        let target = Vec3::new(100.0, 0.0, 0.0);
        mobile.waypoints.push(target);

        let obstacle_center = super::super::spatial::Vec2::new(30.0, 0.0);
        let obstacle = Geometry::Circle(Circle {
            center: obstacle_center,
            radius: 15.0,
        });
        let obstacles = vec![(obstacle_center, obstacle)];

        let derived = 3.0;
        let dt = 1.0;

        let mut min_dist_to_obstacle = f32::MAX;

        for _ in 0..100 {
            // Priority tiers: obstacle avoidance (1), seek (3)
            let avoid = steering::obstacle_avoidance(
                pos,
                mobile.vel,
                &obstacles,
                40.0,
                mobile.steering_force,
            );
            let nav = steering::seek(pos, target, mobile.steering_force);

            let outputs = vec![
                steering::SteeringOutput {
                    tier: 1,
                    force: avoid,
                    weight: 1.0,
                },
                steering::SteeringOutput {
                    tier: 3,
                    force: nav,
                    weight: 1.0,
                },
            ];

            let combined = steering::combine_steering(&outputs, mobile.steering_force);
            pos = integrate(pos, &mut mobile, combined, derived, dt);

            let d = (pos.xy() - obstacle_center).length();
            if d < min_dist_to_obstacle {
                min_dist_to_obstacle = d;
            }

            consume_waypoint(pos, &mut mobile, 5.0);
            if mobile.waypoints.is_empty() {
                break;
            }
        }

        // Entity should not have gone through the obstacle.
        assert!(
            min_dist_to_obstacle > 10.0,
            "entity passed too close to obstacle: min_dist={min_dist_to_obstacle}"
        );
    }

    #[test]
    fn legged_maintains_velocity_no_force() {
        let mut vel = Vec3::new(3.0, 0.0, 0.0);
        let original_speed = vel.length();
        apply_damping(&mut vel, LocomotionType::Legged, 1.0);
        assert!(
            (vel.length() - original_speed).abs() < EPS,
            "legged should maintain velocity: {}",
            vel.length()
        );
    }

    #[test]
    fn wheeled_decelerates_no_force() {
        let mut vel = Vec3::new(3.0, 0.0, 0.0);
        let original_speed = vel.length();
        apply_damping(&mut vel, LocomotionType::Wheeled, 1.0);
        assert!(
            vel.length() < original_speed,
            "wheeled should decelerate: {} vs {}",
            vel.length(),
            original_speed
        );
    }

    #[test]
    fn wheeled_stops_after_many_ticks() {
        let mut vel = Vec3::new(3.0, 0.0, 0.0);
        for _ in 0..20 {
            apply_damping(&mut vel, LocomotionType::Wheeled, 1.0);
        }
        assert!(
            vel.length() < SPEED_EPSILON,
            "wheeled should stop: {}",
            vel.length()
        );
    }

    // --- Waypoint consumption ---

    #[test]
    fn waypoint_consumed_when_close() {
        let mut mobile = Mobile::new(2.0, 10.0);
        mobile.waypoints.push(Vec3::new(10.0, 0.0, 0.0));
        let pos = Vec3::new(9.5, 0.0, 0.0);
        assert!(consume_waypoint(pos, &mut mobile, 1.0));
        assert!(mobile.waypoints.is_empty());
    }

    #[test]
    fn waypoint_not_consumed_when_far() {
        let mut mobile = Mobile::new(2.0, 10.0);
        mobile.waypoints.push(Vec3::new(100.0, 0.0, 0.0));
        let pos = Vec3::ZERO;
        assert!(!consume_waypoint(pos, &mut mobile, 1.0));
        assert_eq!(mobile.waypoints.len(), 1);
    }
}
