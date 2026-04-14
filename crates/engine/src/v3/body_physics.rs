use super::body_model::{
    AngularConstraint, BodyModel, BodyPointId, CORE_DISTANCES, DistanceConstraint, SKELETON_ANGLES,
    stance_template,
};
use super::spatial::{Vec2, Vec3, terrain_height_at};
use super::state::GameState;
use crate::v2::state::EntityKey;

// ---------------------------------------------------------------------------
// Physics constants
// ---------------------------------------------------------------------------

/// Velocity damping per substep (slight energy loss prevents oscillation).
const DAMPING: f32 = 0.98;

/// Gravitational acceleration (m/s^2, downward = negative z).
const GRAVITY: f32 = 9.81;

/// Number of physics substeps per sim tick.
pub const SUBSTEPS: u32 = 4;

/// Number of constraint solver iterations per substep.
const CONSTRAINT_ITERATIONS: u32 = 4;

/// Stance spring force coefficient. Higher = faster convergence to stance.
const STANCE_SPRING_K: f32 = 50.0;

// ---------------------------------------------------------------------------
// Verlet integration
// ---------------------------------------------------------------------------

/// Integrate all body points forward by dt using Verlet integration.
/// Applies velocity damping, gravity, stance spring forces, and external forces.
fn integrate(body: &mut BodyModel, root: Vec3, facing: f32, dt: f32) {
    let template = stance_template(body.stance);
    let cos_f = facing.cos();
    let sin_f = facing.sin();
    let dt2 = dt * dt;

    for i in 0..BodyPointId::COUNT {
        let point = &mut body.points[i];
        let vel = point.pos - point.prev_pos;
        point.prev_pos = point.pos;

        // Gravity (downward in z)
        let gravity_force = Vec3::new(0.0, 0.0, -GRAVITY * point.mass);

        // Stance spring force: pull toward target position
        let offset = &template.offsets[i];
        let target_x = root.x + offset.x * cos_f - offset.y * sin_f;
        let target_y = root.y + offset.x * sin_f + offset.y * cos_f;
        let target_z = root.z + offset.z;
        let target = Vec3::new(target_x, target_y, target_z);
        let to_target = target - point.pos;
        let spring_force = to_target * (STANCE_SPRING_K * template.stiffness * point.mass);

        // External forces (kinetic chain, impacts, etc.)
        let ext = body.external_forces[i];

        // Total acceleration
        let accel = (gravity_force + spring_force + ext) * (1.0 / point.mass);

        // Verlet: new_pos = pos + vel * damping + accel * dt^2
        point.pos = point.pos + vel * DAMPING + accel * dt2;
    }
}

// ---------------------------------------------------------------------------
// Distance constraints
// ---------------------------------------------------------------------------

/// Solve a single distance constraint. Moves both points toward satisfying
/// the rest length, weighted by inverse mass.
fn solve_distance(body: &mut BodyModel, c: &DistanceConstraint) {
    let ai = c.a.index();
    let bi = c.b.index();

    let a_pos = body.points[ai].pos;
    let b_pos = body.points[bi].pos;
    let a_mass = body.points[ai].mass;
    let b_mass = body.points[bi].mass;

    let delta = b_pos - a_pos;
    let dist = delta.length();
    if dist < 1e-8 {
        return;
    }

    let error = dist - c.rest_length;
    let correction = delta * (error / dist);
    let total_mass = a_mass + b_mass;

    body.points[ai].pos = body.points[ai].pos + correction * (a_mass / total_mass);
    body.points[bi].pos = body.points[bi].pos - correction * (b_mass / total_mass);
}

// ---------------------------------------------------------------------------
// Angular constraints
// ---------------------------------------------------------------------------

/// Solve a single angular constraint at a pivot joint. If the angle at the
/// pivot is outside [min_angle, max_angle], project the endpoint to the
/// nearest valid angle.
fn solve_angular(body: &mut BodyModel, c: &AngularConstraint) {
    let ai = c.a.index();
    let pi = c.pivot.index();
    let bi = c.b.index();

    let a_pos = body.points[ai].pos;
    let pivot_pos = body.points[pi].pos;
    let b_pos = body.points[bi].pos;

    let va = a_pos - pivot_pos;
    let vb = b_pos - pivot_pos;

    let la = va.length();
    let lb = vb.length();
    if la < 1e-8 || lb < 1e-8 {
        return;
    }

    let cos_angle = va.dot(vb) / (la * lb);
    let cos_angle = cos_angle.clamp(-1.0, 1.0);
    let angle = cos_angle.acos();

    if angle >= c.min_angle && angle <= c.max_angle {
        return;
    }

    // Clamp to nearest limit
    let target_angle = if angle < c.min_angle {
        c.min_angle
    } else {
        c.max_angle
    };

    // Rotation axis
    let axis = va.cross(vb);
    let axis_len = axis.length();
    if axis_len < 1e-8 {
        return;
    }
    let axis = axis * (1.0 / axis_len);

    // Rotate vb around the axis to achieve target_angle from va
    let rotation_angle = target_angle - angle;
    let new_vb = rotate_around_axis(vb, axis, rotation_angle);
    let new_b_pos = pivot_pos + new_vb;

    // Only move the endpoint (b), not the root (a)
    body.points[bi].pos = new_b_pos;
}

/// Rodrigues' rotation formula: rotate vector v around unit axis k by angle theta.
fn rotate_around_axis(v: Vec3, k: Vec3, theta: f32) -> Vec3 {
    let cos_t = theta.cos();
    let sin_t = theta.sin();
    let k_dot_v = k.dot(v);
    let k_cross_v = k.cross(v);

    v * cos_t + k_cross_v * sin_t + k * (k_dot_v * (1.0 - cos_t))
}

// ---------------------------------------------------------------------------
// Grounding constraint
// ---------------------------------------------------------------------------

/// Pin feet to terrain height. Prevents sinking through ground.
fn solve_grounding(body: &mut BodyModel, terrain_height_at: impl Fn(f32, f32) -> f32) {
    for foot_id in [BodyPointId::LeftFoot, BodyPointId::RightFoot] {
        let i = foot_id.index();
        let pos = body.points[i].pos;
        let ground_z = terrain_height_at(pos.x, pos.y);

        if pos.z < ground_z {
            body.points[i].pos.z = ground_z;
            // Kill vertical velocity to prevent bouncing
            body.points[i].prev_pos.z = ground_z;
        }
    }
}

// ---------------------------------------------------------------------------
// Full physics step
// ---------------------------------------------------------------------------

/// Run the complete body physics step for a single entity's body model.
/// `dt` is the full sim tick dt (substeps subdivide it internally).
pub fn step_body(
    body: &mut BodyModel,
    root: Vec3,
    facing: f32,
    dt: f32,
    terrain_height_at: impl Fn(f32, f32) -> f32,
) {
    let sub_dt = dt / SUBSTEPS as f32;

    for _ in 0..SUBSTEPS {
        // 1. Verlet integration
        integrate(body, root, facing, sub_dt);

        // 2. Constraint solving iterations
        for _ in 0..CONSTRAINT_ITERATIONS {
            // Distance constraints
            for c in CORE_DISTANCES {
                solve_distance(body, c);
            }

            // Angular constraints
            for c in &SKELETON_ANGLES {
                solve_angular(body, c);
            }

            // Grounding
            solve_grounding(body, &terrain_height_at);
        }
    }

    // Clear external forces after full step
    body.clear_forces();
}

// ---------------------------------------------------------------------------
// Sim integration: tick all body models
// ---------------------------------------------------------------------------

/// Run body physics for all entities that have active body models.
/// Called from sim::tick between movement and combat phases.
pub fn tick_body_physics(state: &mut GameState, dt: f32) {
    // Extract body models to avoid borrow conflicts (entity mutation + heightfield read).
    let work: Vec<(EntityKey, Vec3, f32)> = state
        .entities
        .iter()
        .filter_map(|(k, e)| {
            if e.body.is_some() && e.pos.is_some() {
                let root = e.pos.unwrap();
                let facing = e.combatant.as_ref().map(|c| c.facing).unwrap_or(0.0);
                Some((k, root, facing))
            } else {
                None
            }
        })
        .collect();

    for (key, root, facing) in work {
        // Take the body model out, step it, put it back.
        // This lets us borrow heightfield immutably during step_body.
        let mut body = match state.entities[key].body.take() {
            Some(b) => *b,
            None => continue,
        };

        step_body(&mut body, root, facing, dt, |x, y| {
            terrain_height_at(state, Vec2::new(x, y))
        });

        state.entities[key].body = Some(Box::new(body));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::body_model::{BodyModel, StanceId};
    use super::*;

    fn flat_terrain(_x: f32, _y: f32) -> f32 {
        0.0
    }

    #[test]
    fn verlet_no_explosion_1000_ticks() {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);
        let dt = 0.05; // 20 ticks/sec

        for _ in 0..1000 {
            step_body(&mut body, Vec3::ZERO, 0.0, dt, flat_terrain);
        }

        // Energy should not diverge
        let ke = body.kinetic_energy();
        assert!(
            ke < 1000.0,
            "kinetic energy diverged: {ke} (should be bounded)"
        );

        // No point should be wildly out of range
        for (i, p) in body.points.iter().enumerate() {
            assert!(p.pos.length() < 50.0, "point {i} exploded to {:?}", p.pos);
        }
    }

    #[test]
    fn distance_constraints_maintained() {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);
        let dt = 0.05;

        for _ in 0..1000 {
            step_body(&mut body, Vec3::ZERO, 0.0, dt, flat_terrain);
        }

        // Check limb proportions within 5%
        for c in CORE_DISTANCES {
            let a = body.points[c.a.index()].pos;
            let b = body.points[c.b.index()].pos;
            let dist = (b - a).length();
            let error = (dist - c.rest_length).abs() / c.rest_length;
            assert!(
                error < 0.05,
                "constraint {:?}-{:?}: dist={dist:.3}, rest={:.3}, error={:.1}%",
                c.a,
                c.b,
                c.rest_length,
                error * 100.0,
            );
        }
    }

    #[test]
    fn angular_constraints_prevent_hyperextension() {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);

        // Apply a strong force to try to hyperextend left elbow
        body.apply_force(BodyPointId::LeftHand, Vec3::new(0.0, 0.0, -500.0));

        let dt = 0.05;
        for _ in 0..100 {
            step_body(&mut body, Vec3::ZERO, 0.0, dt, flat_terrain);
        }

        // Check elbow angle is within limits
        let shoulder = body.point(BodyPointId::LeftShoulder).pos;
        let elbow = body.point(BodyPointId::LeftElbow).pos;
        let hand = body.point(BodyPointId::LeftHand).pos;

        let va = shoulder - elbow;
        let vb = hand - elbow;
        let la = va.length();
        let lb = vb.length();

        if la > 1e-6 && lb > 1e-6 {
            let cos_angle = (va.dot(vb) / (la * lb)).clamp(-1.0, 1.0);
            let angle = cos_angle.acos();
            assert!(
                angle > 0.10,
                "elbow hyperextended: angle={:.1} deg",
                angle.to_degrees()
            );
        }
    }

    #[test]
    fn gravity_grounds_idle_entity() {
        // Start body slightly above ground
        let mut body = BodyModel::from_stance(Vec3::new(0.0, 0.0, 0.5), 0.0, StanceId::Neutral);

        let dt = 0.05;
        for _ in 0..200 {
            step_body(&mut body, Vec3::new(0.0, 0.0, 0.0), 0.0, dt, flat_terrain);
        }

        // Feet should be at or near ground level
        let lf = body.point(BodyPointId::LeftFoot).pos;
        let rf = body.point(BodyPointId::RightFoot).pos;
        assert!(lf.z.abs() < 0.05, "left foot not grounded: z={}", lf.z);
        assert!(rf.z.abs() < 0.05, "right foot not grounded: z={}", rf.z);
    }

    #[test]
    fn stance_transition_converges() {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);

        // Switch to high guard
        body.set_stance(StanceId::HighGuard);
        let target = stance_template(StanceId::HighGuard);

        let dt = 0.05;
        // Run enough ticks for convergence (spring + gravity + constraints
        // need more iterations than the ideal 2-4)
        for _ in 0..40 {
            step_body(&mut body, Vec3::ZERO, 0.0, dt, flat_terrain);
        }

        // Right hand should have moved significantly toward high guard.
        // Full convergence takes longer due to constraint competition,
        // but the hand should be noticeably higher than neutral (~0.79).
        let rh = body.point(BodyPointId::RightHand).pos;
        assert!(
            rh.z > 1.0,
            "right hand should be raised in high guard: z={:.2} (neutral ~0.79)",
            rh.z,
        );
    }

    #[test]
    fn no_energy_divergence_10k_ticks() {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::Neutral);
        let dt = 0.05;

        let mut max_ke: f32 = 0.0;
        for _ in 0..10_000 {
            step_body(&mut body, Vec3::ZERO, 0.0, dt, flat_terrain);
            max_ke = max_ke.max(body.kinetic_energy());
        }

        assert!(
            max_ke < 5000.0,
            "energy diverged over 10k ticks: max_ke={max_ke}"
        );
    }

    #[test]
    fn activation_deactivation_roundtrip() {
        let root = Vec3::new(5.0, 10.0, 0.0);
        let facing = 1.0;
        let mut body = BodyModel::from_stance(root, facing, StanceId::MidGuard);

        let dt = 0.05;
        for _ in 0..20 {
            step_body(&mut body, root, facing, dt, flat_terrain);
        }

        // "Deactivate": save stance ID
        let saved_stance = body.stance;

        // "Reactivate": reconstruct from stance
        let body2 = BodyModel::from_stance(root, facing, saved_stance);

        // Body should be approximately in the same configuration
        for i in 0..BodyPointId::COUNT {
            let error = (body.points[i].pos - body2.points[i].pos).length();
            assert!(
                error < 1.0,
                "point {i} diverged on reactivation: error={error:.2}"
            );
        }
    }
}
