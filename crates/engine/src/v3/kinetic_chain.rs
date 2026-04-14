use serde::{Deserialize, Serialize};

use super::body_model::{BodyModel, BodyPointId};
use super::spatial::Vec3;

// ---------------------------------------------------------------------------
// Attack motion types
// ---------------------------------------------------------------------------

/// The type of physical attack motion, determining which kinetic chain
/// link sequence and target positions to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackMotion {
    /// Overhead downward swing (targets head/upper body).
    Overhead,
    /// Forehand horizontal swing from right to left.
    Forehand,
    /// Backhand horizontal swing from left to right.
    Backhand,
    /// Forward thrust (pierce weapons, longsword mordstreich).
    Thrust,
}

// ---------------------------------------------------------------------------
// Kinetic chain
// ---------------------------------------------------------------------------

/// The 6 sequential links in the kinetic chain, from ground to weapon tip.
const CHAIN_LINKS: [BodyPointId; 6] = [
    BodyPointId::RightFoot,     // 0: rear foot push (ground reaction)
    BodyPointId::RightHip,      // 1: hip rotation toward target
    BodyPointId::LowerSpine,    // 2: torso follows hips (spine twist)
    BodyPointId::RightShoulder, // 3: shoulder accelerates arm
    BodyPointId::RightElbow,    // 4: elbow extends
    BodyPointId::RightHand,     // 5: wrist snap
];

/// Force magnitude for each chain link (Newtons, applied as impulse).
/// Must be large enough to overcome stance spring forces (K=50 * stiffness * mass).
/// At stiffness=0.6 and mass=1.0 (hand), spring ~= 30N per meter displacement.
/// Chain forces need to significantly exceed this for visible acceleration.
const LINK_FORCES: [f32; 6] = [
    400.0, // Ground reaction
    350.0, // Hip rotation
    300.0, // Spine twist
    250.0, // Shoulder
    200.0, // Elbow extension
    150.0, // Wrist snap
];

/// State tracking for a kinetic chain activation during an attack swing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KineticChainState {
    /// Which attack motion is being executed.
    pub motion: AttackMotion,
    /// Index of the currently active link (0-5).
    pub active_link: usize,
    /// Ticks remaining before the next link activates.
    pub link_timer: u16,
    /// Delay between link activations (ticks). Tighter = more skilled.
    pub link_delay: u16,
    /// Direction toward target (radians) at swing start.
    pub target_direction: f32,
    /// Whether the chain has completed all links.
    pub completed: bool,
    /// Force multiplier from combat skill (0.5–1.5). Higher skill = more
    /// efficient force transfer through the kinetic chain.
    pub force_multiplier: f32,
}

impl KineticChainState {
    /// Create a new kinetic chain state for the given motion and skill level.
    ///
    /// `combat_skill` (0.0–1.0) affects link timing: higher skill = tighter
    /// sequential activation = higher peak tip velocity.
    /// `target_direction` is the angle toward the target in radians.
    pub fn new(motion: AttackMotion, combat_skill: f32, target_direction: f32) -> Self {
        let skill = combat_skill.clamp(0.0, 1.0);
        // Skill affects link delay: 3 ticks at skill 0.0, 1 tick at skill 1.0
        let delay = (3.0 - 2.0 * skill).round() as u16;
        let delay = delay.max(1);
        // Skill also affects force transfer efficiency: 0.5x at skill 0.0, 1.5x at skill 1.0
        let force_multiplier = 0.5 + skill;

        Self {
            motion,
            active_link: 0,
            link_timer: 0,
            link_delay: delay,
            target_direction,
            completed: false,
            force_multiplier,
        }
    }

    /// Advance the kinetic chain by one tick. Applies forces to the body model
    /// based on the currently active link and attack motion.
    ///
    /// Returns `true` when all links have fired (chain complete).
    pub fn tick(&mut self, body: &mut BodyModel) -> bool {
        if self.completed {
            return true;
        }

        // Apply force for the current active link
        let link_point = CHAIN_LINKS[self.active_link];
        let force_mag = LINK_FORCES[self.active_link] * self.force_multiplier;
        let force_dir = self.force_direction(self.active_link);
        let force = force_dir * force_mag;
        body.apply_force(link_point, force);

        // Advance timer
        self.link_timer += 1;
        if self.link_timer >= self.link_delay {
            self.link_timer = 0;
            self.active_link += 1;
            if self.active_link >= CHAIN_LINKS.len() {
                self.completed = true;
                return true;
            }
        }

        false
    }

    /// Compute the force direction for a given chain link based on attack motion.
    fn force_direction(&self, link_index: usize) -> Vec3 {
        let cos_d = self.target_direction.cos();
        let sin_d = self.target_direction.sin();

        match self.motion {
            AttackMotion::Overhead => {
                match link_index {
                    0 => Vec3::new(0.0, 0.0, 1.0),         // Ground push up
                    1 | 2 => Vec3::new(cos_d, sin_d, 0.0), // Hip/spine toward target
                    3 | 4 | 5 => Vec3::new(cos_d * 0.3, sin_d * 0.3, -1.0).normalize(), // Arm downward + forward
                    _ => Vec3::ZERO,
                }
            }
            AttackMotion::Forehand => {
                // Horizontal right-to-left (perpendicular to facing)
                let perp_x = -sin_d; // perpendicular (left of facing)
                let perp_y = cos_d;
                match link_index {
                    0 => Vec3::new(0.0, 0.0, 0.5), // Ground push
                    1 | 2 => Vec3::new(cos_d * 0.5 + perp_x * 0.5, sin_d * 0.5 + perp_y * 0.5, 0.0),
                    3 | 4 | 5 => Vec3::new(perp_x, perp_y, 0.0), // Sweep left
                    _ => Vec3::ZERO,
                }
            }
            AttackMotion::Backhand => {
                // Horizontal left-to-right
                let perp_x = sin_d; // perpendicular (right of facing)
                let perp_y = -cos_d;
                match link_index {
                    0 => Vec3::new(0.0, 0.0, 0.5),
                    1 | 2 => Vec3::new(cos_d * 0.5 + perp_x * 0.5, sin_d * 0.5 + perp_y * 0.5, 0.0),
                    3 | 4 | 5 => Vec3::new(perp_x, perp_y, 0.0),
                    _ => Vec3::ZERO,
                }
            }
            AttackMotion::Thrust => {
                // Straight forward
                match link_index {
                    0 => Vec3::new(cos_d * 0.3, sin_d * 0.3, 0.3), // Push forward + up
                    1 | 2 | 3 => Vec3::new(cos_d, sin_d, 0.0),     // Everything forward
                    4 | 5 => Vec3::new(cos_d, sin_d, 0.0),         // Arm extends forward
                    _ => Vec3::ZERO,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tip velocity extraction
// ---------------------------------------------------------------------------

/// Extract the sword tip velocity from the body model.
/// Uses the right hand velocity (Verlet-derived) as the weapon tip proxy.
/// When a sword-tip equipment point exists (A3+), this should use that instead.
pub fn tip_velocity(body: &BodyModel) -> Vec3 {
    body.point(BodyPointId::RightHand).velocity()
}

/// Compute tip speed (scalar, m/s) for use in kinetic energy calculations.
pub fn tip_speed(body: &BodyModel) -> f32 {
    tip_velocity(body).length()
}

/// Compute kinetic energy from body-derived tip velocity and weapon mass.
/// Replaces `0.5 * weapon_mass * BASE_SWING_SPEED^2` when body model is active.
pub fn kinetic_energy_from_body(body: &BodyModel, weapon_mass: f32) -> f32 {
    let speed = tip_speed(body);
    0.5 * weapon_mass * speed * speed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::body_model::{BodyModel, StanceId};
    use super::super::body_physics::step_body;
    use super::*;

    fn flat_terrain(_x: f32, _y: f32) -> f32 {
        0.0
    }

    #[test]
    fn kinetic_chain_completes() {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::MidGuard);
        let mut chain = KineticChainState::new(AttackMotion::Forehand, 0.5, 0.0);

        let mut ticks = 0;
        while !chain.completed {
            chain.tick(&mut body);
            step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
            ticks += 1;
            assert!(ticks < 100, "chain should complete in bounded time");
        }

        assert!(chain.completed);
    }

    #[test]
    fn tip_velocity_increases_during_swing() {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::MidGuard);
        let mut chain = KineticChainState::new(AttackMotion::Forehand, 0.8, 0.0);

        // Let body settle first
        for _ in 0..10 {
            step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
        }

        let speed_before = tip_speed(&body);

        // Execute kinetic chain
        for _ in 0..20 {
            chain.tick(&mut body);
            step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
        }

        let speed_after = tip_speed(&body);
        assert!(
            speed_after > speed_before,
            "tip should accelerate during swing: before={speed_before:.2}, after={speed_after:.2}"
        );
    }

    #[test]
    fn higher_skill_produces_faster_tip() {
        fn swing_peak_speed(skill: f32) -> f32 {
            let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::MidGuard);
            let mut chain = KineticChainState::new(AttackMotion::Forehand, skill, 0.0);

            // Settle
            for _ in 0..10 {
                step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
            }

            let mut max_speed: f32 = 0.0;
            for _ in 0..30 {
                chain.tick(&mut body);
                step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
                max_speed = max_speed.max(tip_speed(&body));
            }
            max_speed
        }

        let low_skill = swing_peak_speed(0.2);
        let high_skill = swing_peak_speed(0.8);

        assert!(
            high_skill > low_skill,
            "higher skill should produce faster tip: low={low_skill:.2}, high={high_skill:.2}"
        );
    }

    #[test]
    fn different_motions_produce_different_velocities() {
        fn swing_velocity(motion: AttackMotion) -> Vec3 {
            let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::MidGuard);
            let mut chain = KineticChainState::new(motion, 0.6, 0.0);

            for _ in 0..10 {
                step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
            }

            for _ in 0..20 {
                chain.tick(&mut body);
                step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
            }
            tip_velocity(&body)
        }

        let overhead = swing_velocity(AttackMotion::Overhead);
        let thrust = swing_velocity(AttackMotion::Thrust);

        // Overhead should have significant downward component
        // Thrust should have significant forward component
        // They should differ meaningfully
        let diff = (overhead - thrust).length();
        assert!(
            diff > 0.1,
            "different motions should produce different velocities: overhead={:?}, thrust={:?}",
            overhead,
            thrust,
        );
    }

    #[test]
    fn kinetic_energy_from_body_uses_actual_velocity() {
        let mut body = BodyModel::from_stance(Vec3::ZERO, 0.0, StanceId::MidGuard);
        let mut chain = KineticChainState::new(AttackMotion::Forehand, 0.6, 0.0);

        for _ in 0..10 {
            step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
        }

        for _ in 0..15 {
            chain.tick(&mut body);
            step_body(&mut body, Vec3::ZERO, 0.0, 0.05, flat_terrain);
        }

        let ke = kinetic_energy_from_body(&body, 1.2);
        assert!(ke > 0.0, "should have positive KE during swing");
    }

    #[test]
    fn link_delay_scales_with_skill() {
        let low = KineticChainState::new(AttackMotion::Forehand, 0.0, 0.0);
        let high = KineticChainState::new(AttackMotion::Forehand, 1.0, 0.0);

        assert!(
            low.link_delay > high.link_delay,
            "low skill should have longer delay: low={}, high={}",
            low.link_delay,
            high.link_delay,
        );
    }
}
