use super::collision::Geometry;
use super::spatial::{Vec2, Vec3};

// ---------------------------------------------------------------------------
// Tunable constants
// ---------------------------------------------------------------------------

/// Default slowing radius for Arrive behavior (meters).
const DEFAULT_SLOWING_RADIUS: f32 = 50.0;

/// Default desired separation distance between entities (meters).
const DEFAULT_SEPARATION_DISTANCE: f32 = 15.0;

/// Default obstacle detection distance (meters).
const DEFAULT_OBSTACLE_LOOKAHEAD: f32 = 40.0;

/// Default lateral avoidance strength multiplier.
const DEFAULT_AVOIDANCE_STRENGTH: f32 = 1.5;

// ---------------------------------------------------------------------------
// Steering behaviors — each returns an acceleration vector (Vec3)
// ---------------------------------------------------------------------------

/// Seek: steer toward a target position at maximum force.
pub fn seek(pos: Vec3, target: Vec3, max_force: f32) -> Vec3 {
    let desired = target - pos;
    let dist = desired.length();
    if dist < 1e-6 {
        return Vec3::ZERO;
    }
    desired.normalize() * max_force
}

/// Arrive: seek with deceleration near target. Returns zero when within
/// `arrival_tolerance` of the target. Slows linearly within `slowing_radius`.
pub fn arrive(
    pos: Vec3,
    vel: Vec3,
    target: Vec3,
    max_force: f32,
    max_speed: f32,
    slowing_radius: f32,
) -> Vec3 {
    let offset = target - pos;
    let dist = offset.length();
    if dist < 0.5 {
        // Close enough — brake to stop.
        return if vel.length() > 0.1 {
            vel * -1.0 // opposing force to stop
        } else {
            Vec3::ZERO
        };
    }

    let radius = if slowing_radius > 0.0 {
        slowing_radius
    } else {
        DEFAULT_SLOWING_RADIUS
    };

    // Desired speed ramps down linearly inside the slowing radius.
    let desired_speed = if dist < radius {
        max_speed * (dist / radius)
    } else {
        max_speed
    };

    let desired_vel = offset.normalize() * desired_speed;
    let steering = desired_vel - vel;
    clamp_force(steering, max_force)
}

/// Separation: push away from nearby entities to prevent overlap.
/// `neighbors` is a slice of positions of nearby entities.
pub fn separation(pos: Vec3, neighbors: &[Vec3], desired_distance: f32) -> Vec3 {
    let dist = if desired_distance > 0.0 {
        desired_distance
    } else {
        DEFAULT_SEPARATION_DISTANCE
    };

    let mut force = Vec3::ZERO;
    for &other in neighbors {
        let diff = pos - other;
        let d = diff.length();
        if d < 1e-6 || d > dist {
            continue;
        }
        // Inverse-linear: stronger when closer.
        let strength = (dist - d) / dist;
        force = force + diff.normalize() * strength;
    }
    force
}

/// Obstacle Avoidance: steer around static obstacle geometries.
///
/// Uses a simple ahead-vector approach: project the entity's velocity forward,
/// check if any obstacle intersects the lookahead cylinder, and steer laterally.
pub fn obstacle_avoidance(
    pos: Vec3,
    vel: Vec3,
    obstacles: &[(Vec2, Geometry)],
    lookahead: f32,
    max_force: f32,
) -> Vec3 {
    let speed = vel.length();
    if speed < 0.1 {
        return Vec3::ZERO;
    }

    let look = if lookahead > 0.0 {
        lookahead
    } else {
        DEFAULT_OBSTACLE_LOOKAHEAD
    };

    let dir = vel.normalize();
    let ahead = pos + dir * look;
    let half_ahead = pos + dir * (look * 0.5);

    let pos2 = pos.xy();
    let ahead2 = ahead.xy();
    let half2 = half_ahead.xy();

    let mut closest_dist = f32::MAX;
    let mut avoidance = Vec3::ZERO;

    for &(center, ref geom) in obstacles {
        // Simple bounding circle check for all geometry types.
        let radius = geometry_bounding_radius(geom);
        let to_center = center - pos2;

        // Check if ahead point, half-ahead point, or current pos is near the obstacle.
        let dist_ahead = (ahead2 - center).length();
        let dist_half = (half2 - center).length();
        let dist_pos = (pos2 - center).length();
        let min_dist = dist_ahead.min(dist_half).min(dist_pos);

        if min_dist < radius + DEFAULT_SEPARATION_DISTANCE && min_dist < closest_dist {
            closest_dist = min_dist;

            // Steer perpendicular to the obstacle direction.
            let lateral = Vec2::new(-to_center.y, to_center.x).normalize();
            avoidance = Vec3::new(lateral.x, lateral.y, 0.0)
                * max_force
                * DEFAULT_AVOIDANCE_STRENGTH;
        }
    }

    avoidance
}

/// Cohesion: steer toward the center of mass of nearby group members.
pub fn cohesion(pos: Vec3, neighbors: &[Vec3], max_force: f32) -> Vec3 {
    if neighbors.is_empty() {
        return Vec3::ZERO;
    }
    let mut center = Vec3::ZERO;
    for &n in neighbors {
        center = center + n;
    }
    center = center * (1.0 / neighbors.len() as f32);
    seek(pos, center, max_force)
}

/// Alignment: match velocity of nearby group members.
pub fn alignment(vel: Vec3, neighbor_vels: &[Vec3], max_force: f32) -> Vec3 {
    if neighbor_vels.is_empty() {
        return Vec3::ZERO;
    }
    let mut avg_vel = Vec3::ZERO;
    for &v in neighbor_vels {
        avg_vel = avg_vel + v;
    }
    avg_vel = avg_vel * (1.0 / neighbor_vels.len() as f32);
    let steering = avg_vel - vel;
    clamp_force(steering, max_force)
}

// ---------------------------------------------------------------------------
// Priority-tier force combination
// ---------------------------------------------------------------------------

/// A steering behavior output tagged with its priority tier.
/// Lower tier number = higher priority.
#[derive(Debug, Clone)]
pub struct SteeringOutput {
    pub tier: u8,
    pub force: Vec3,
    pub weight: f32,
}

/// Combine steering outputs using priority tiers with weighted sum within tiers.
///
/// The highest-priority (lowest tier number) tier that produces nonzero output
/// wins. Lower-priority tiers are discarded entirely.
pub fn combine_steering(outputs: &[SteeringOutput], max_force: f32) -> Vec3 {
    if outputs.is_empty() {
        return Vec3::ZERO;
    }

    // Find the highest-priority tier with nonzero force.
    let mut min_tier = u8::MAX;
    for o in outputs {
        if o.force.length_squared() > 1e-10 && o.tier < min_tier {
            min_tier = o.tier;
        }
    }

    if min_tier == u8::MAX {
        return Vec3::ZERO;
    }

    // Weighted sum within the winning tier.
    let mut combined = Vec3::ZERO;
    let mut total_weight = 0.0;
    for o in outputs {
        if o.tier == min_tier && o.force.length_squared() > 1e-10 {
            combined = combined + o.force * o.weight;
            total_weight += o.weight;
        }
    }

    if total_weight > 0.0 {
        combined = combined * (1.0 / total_weight);
    }

    clamp_force(combined, max_force)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn clamp_force(force: Vec3, max: f32) -> Vec3 {
    let mag = force.length();
    if mag > max && mag > 1e-10 {
        force.normalize() * max
    } else {
        force
    }
}

/// Approximate bounding radius for obstacle geometry.
fn geometry_bounding_radius(geom: &Geometry) -> f32 {
    match geom {
        Geometry::Circle(c) => c.radius,
        Geometry::Segment(s) => {
            let half_len = ((s.end.x - s.start.x).powi(2) + (s.end.y - s.start.y).powi(2))
                .sqrt()
                / 2.0;
            half_len + s.thickness
        }
        Geometry::Rect(r) => (r.half_extents.x.powi(2) + r.half_extents.y.powi(2)).sqrt(),
        Geometry::Triangle(t) => {
            let cx = (t.a.x + t.b.x + t.c.x) / 3.0;
            let cy = (t.a.y + t.b.y + t.c.y) / 3.0;
            let da = ((t.a.x - cx).powi(2) + (t.a.y - cy).powi(2)).sqrt();
            let db = ((t.b.x - cx).powi(2) + (t.b.y - cy).powi(2)).sqrt();
            let dc = ((t.c.x - cx).powi(2) + (t.c.y - cy).powi(2)).sqrt();
            da.max(db).max(dc)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 0.01;

    #[test]
    fn seek_toward_target() {
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let target = Vec3::new(100.0, 0.0, 0.0);
        let f = seek(pos, target, 5.0);
        assert!(f.x > 0.0, "should steer toward target");
        assert!((f.length() - 5.0).abs() < EPS, "force should be max");
    }

    #[test]
    fn seek_at_target_returns_zero() {
        let pos = Vec3::new(50.0, 50.0, 0.0);
        let f = seek(pos, pos, 5.0);
        assert!(f.length() < EPS);
    }

    #[test]
    fn arrive_decelerates_near_target() {
        let pos = Vec3::new(10.0, 0.0, 0.0);
        let vel = Vec3::new(3.0, 0.0, 0.0);
        let target = Vec3::new(15.0, 0.0, 0.0);
        let f_near = arrive(pos, vel, target, 5.0, 3.0, 50.0);

        let pos_far = Vec3::new(-100.0, 0.0, 0.0);
        let f_far = arrive(pos_far, vel, target, 5.0, 3.0, 50.0);

        // Near target, desired speed is lower → steering force is smaller or braking.
        // Far from target, desired speed is max → stronger forward force.
        // The near force should have a smaller x-component than the far force.
        assert!(
            f_near.x < f_far.x,
            "near force x={} should be less than far force x={}",
            f_near.x,
            f_far.x
        );
    }

    #[test]
    fn arrive_brakes_at_target() {
        let target = Vec3::new(0.0, 0.0, 0.0);
        let pos = Vec3::new(0.2, 0.0, 0.0); // very close
        let vel = Vec3::new(2.0, 0.0, 0.0); // moving fast
        let f = arrive(pos, vel, target, 5.0, 3.0, 50.0);
        // Should produce opposing force
        assert!(f.x < 0.0, "should brake: force x={}", f.x);
    }

    #[test]
    fn separation_pushes_apart() {
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let neighbors = vec![Vec3::new(5.0, 0.0, 0.0)];
        let f = separation(pos, &neighbors, 15.0);
        assert!(f.x < 0.0, "should push away from neighbor");
    }

    #[test]
    fn separation_ignores_distant() {
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let neighbors = vec![Vec3::new(100.0, 0.0, 0.0)];
        let f = separation(pos, &neighbors, 15.0);
        assert!(f.length() < EPS, "should ignore distant neighbor");
    }

    #[test]
    fn separation_no_overlap() {
        // Two entities at same position: separation should produce nonzero force
        // due to the epsilon check (returns zero for coincident).
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let neighbors = vec![Vec3::new(0.0, 0.0, 0.0)];
        let f = separation(pos, &neighbors, 15.0);
        // Coincident is handled by d < 1e-6 check — returns zero (degenerate).
        assert!(f.length() < EPS);
    }

    #[test]
    fn cohesion_toward_group_center() {
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let neighbors = vec![
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(10.0, 10.0, 0.0),
        ];
        let f = cohesion(pos, &neighbors, 5.0);
        assert!(f.x > 0.0, "should steer toward group center");
        assert!(f.y > 0.0, "should steer toward group center y");
    }

    #[test]
    fn alignment_matches_group_velocity() {
        let vel = Vec3::new(1.0, 0.0, 0.0);
        let neighbor_vels = vec![
            Vec3::new(0.0, 3.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
        ];
        let f = alignment(vel, &neighbor_vels, 5.0);
        // Group moves in +y, entity moves in +x. Should steer toward +y, away from +x.
        assert!(f.y > 0.0, "should steer toward group velocity direction");
        assert!(f.x < 0.0, "should reduce divergent velocity");
    }

    #[test]
    fn combine_highest_priority_wins() {
        let outputs = vec![
            SteeringOutput {
                tier: 3,
                force: Vec3::new(0.0, 10.0, 0.0),
                weight: 1.0,
            },
            SteeringOutput {
                tier: 1,
                force: Vec3::new(5.0, 0.0, 0.0),
                weight: 1.0,
            },
        ];
        let f = combine_steering(&outputs, 10.0);
        // Tier 1 wins, tier 3 discarded.
        assert!(f.x > 0.0, "tier 1 force should dominate");
        assert!(f.y.abs() < EPS, "tier 3 force should be discarded");
    }

    #[test]
    fn combine_weighted_sum_within_tier() {
        let outputs = vec![
            SteeringOutput {
                tier: 2,
                force: Vec3::new(4.0, 0.0, 0.0),
                weight: 1.0,
            },
            SteeringOutput {
                tier: 2,
                force: Vec3::new(0.0, 4.0, 0.0),
                weight: 1.0,
            },
        ];
        let f = combine_steering(&outputs, 10.0);
        // Equal weight: average of (4,0,0) and (0,4,0) = (2,2,0)
        assert!((f.x - 2.0).abs() < EPS);
        assert!((f.y - 2.0).abs() < EPS);
    }

    #[test]
    fn combine_empty_returns_zero() {
        let f = combine_steering(&[], 10.0);
        assert!(f.length() < EPS);
    }

    #[test]
    fn combine_all_zero_returns_zero() {
        let outputs = vec![SteeringOutput {
            tier: 1,
            force: Vec3::ZERO,
            weight: 1.0,
        }];
        let f = combine_steering(&outputs, 10.0);
        assert!(f.length() < EPS);
    }

    #[test]
    fn combine_clamped_to_max_force() {
        let outputs = vec![SteeringOutput {
            tier: 1,
            force: Vec3::new(100.0, 0.0, 0.0),
            weight: 1.0,
        }];
        let f = combine_steering(&outputs, 5.0);
        assert!(
            (f.length() - 5.0).abs() < EPS,
            "should clamp to max_force: {}",
            f.length()
        );
    }

    #[test]
    fn obstacle_avoidance_steers_around() {
        use super::super::collision::{Circle, Geometry};

        let pos = Vec3::new(0.0, 0.0, 0.0);
        let vel = Vec3::new(3.0, 0.0, 0.0); // moving right
        let obstacle_center = Vec2::new(20.0, 0.0); // directly ahead
        let obstacle = Geometry::Circle(Circle {
            center: obstacle_center,
            radius: 10.0,
        });

        let f = obstacle_avoidance(pos, vel, &[(obstacle_center, obstacle)], 40.0, 5.0);
        // Should produce lateral force (y component) to avoid.
        assert!(
            f.y.abs() > 0.1,
            "should steer laterally to avoid obstacle: f={:?}",
            f
        );
    }

    #[test]
    fn obstacle_avoidance_no_obstacle_returns_zero() {
        let pos = Vec3::new(0.0, 0.0, 0.0);
        let vel = Vec3::new(3.0, 0.0, 0.0);
        let f = obstacle_avoidance(pos, vel, &[], 40.0, 5.0);
        assert!(f.length() < EPS);
    }

    #[test]
    fn obstacle_avoidance_stationary_returns_zero() {
        use super::super::collision::{Circle, Geometry};

        let pos = Vec3::new(0.0, 0.0, 0.0);
        let vel = Vec3::ZERO; // not moving
        let obstacle = Geometry::Circle(Circle {
            center: Vec2::new(10.0, 0.0),
            radius: 5.0,
        });
        let f = obstacle_avoidance(pos, vel, &[(Vec2::new(10.0, 0.0), obstacle)], 40.0, 5.0);
        assert!(f.length() < EPS, "stationary entity should not avoid");
    }
}
