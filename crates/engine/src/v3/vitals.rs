use serde::{Deserialize, Serialize};

use super::armor::BodyZone;
use super::wound::{zone_wound_weight, Wound};

// ---------------------------------------------------------------------------
// Tunable constants — blood thresholds
// ---------------------------------------------------------------------------

/// Blood level below which combat effectiveness degrades linearly.
const BLOOD_DEGRADATION_THRESHOLD: f32 = 0.5;
/// Blood level below which the entity collapses (can't act, continues bleeding).
const BLOOD_COLLAPSE_THRESHOLD: f32 = 0.2;

// ---------------------------------------------------------------------------
// Tunable constants — stamina
// ---------------------------------------------------------------------------

/// Base stamina recovery per tick (full recovery from 0 in 20 ticks unwounded).
const STAMINA_RECOVERY_RATE: f32 = 0.05;

/// Per-zone weight for wound penalty on stamina recovery.
const WOUND_PENALTY_TORSO: f32 = 1.0;
const WOUND_PENALTY_LEGS: f32 = 0.5;
const WOUND_PENALTY_HEAD: f32 = 0.3;
const WOUND_PENALTY_ARMS: f32 = 0.2;

/// Stamina drain per tick while sprinting.
const SPRINT_DRAIN: f32 = 0.08;
/// Stamina drain per tick while running.
const RUN_DRAIN: f32 = 0.03;

// ---------------------------------------------------------------------------
// Movement modes
// ---------------------------------------------------------------------------

/// Three explicit movement modes, agent-selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MovementMode {
    /// Sustainable forever, no stamina cost.
    Walk,
    /// Sustained movement at ~1.5× speed. Moderate stamina drain.
    Run,
    /// Tactical burst at ~2× speed. High stamina drain.
    Sprint,
}

// ---------------------------------------------------------------------------
// Vitals
// ---------------------------------------------------------------------------

/// Blood pool, stamina pool, stagger state, and movement mode for an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vitals {
    /// Blood pool: 1.0 = full, 0.0 = dead.
    pub blood: f32,
    /// Stamina pool: 1.0 = full, 0.0 = exhausted.
    pub stamina: f32,
    /// Remaining stagger ticks. While > 0, entity can't attack, block, or move.
    pub stagger_ticks: u16,
    /// Current movement mode.
    pub movement_mode: MovementMode,
}

impl Vitals {
    pub fn new() -> Self {
        Self {
            blood: 1.0,
            stamina: 1.0,
            stagger_ticks: 0,
            movement_mode: MovementMode::Walk,
        }
    }

    /// Combat effectiveness multiplier based on blood level.
    /// 1.0 at full blood, degrades linearly below threshold.
    /// At 0.3 blood → effectiveness 0.6×.
    pub fn effectiveness(&self) -> f32 {
        if self.blood >= BLOOD_DEGRADATION_THRESHOLD {
            1.0
        } else if self.blood <= 0.0 {
            0.0
        } else {
            self.blood / BLOOD_DEGRADATION_THRESHOLD
        }
    }

    /// Entity has collapsed from blood loss. Can't act, continues bleeding.
    pub fn is_collapsed(&self) -> bool {
        self.blood < BLOOD_COLLAPSE_THRESHOLD
    }

    /// Entity is dead.
    pub fn is_dead(&self) -> bool {
        self.blood <= 0.0
    }

    /// Entity is staggered and can't act.
    pub fn is_staggered(&self) -> bool {
        self.stagger_ticks > 0
    }

    /// Apply stagger. Duration proportional to force, clamped [2, 5] ticks.
    pub fn apply_stagger(&mut self, force: f32, threshold: f32, scale: f32) {
        let base = 2.0;
        let duration = base + (force - threshold) * scale;
        let ticks = duration.clamp(2.0, 5.0) as u16;
        // Only overwrite if new stagger is longer than remaining
        if ticks > self.stagger_ticks {
            self.stagger_ticks = ticks;
        }
    }

    /// Drain blood by the sum of effective bleed rates from all wounds.
    pub fn tick_bleed(&mut self, wounds: &[Wound], current_tick: u64, dt: f32) {
        let total_bleed: f32 = wounds
            .iter()
            .map(|w| super::wound::effective_bleed(w, current_tick))
            .sum();
        self.blood = (self.blood - total_bleed * dt).max(0.0);
    }

    /// Recover stamina, penalized by wounds. Also ticks down stagger.
    pub fn tick_stamina_recovery(&mut self, wounds: &[Wound], dt: f32) {
        // Tick stagger
        if self.stagger_ticks > 0 {
            self.stagger_ticks = self.stagger_ticks.saturating_sub(1);
        }

        // Wound penalty on stamina recovery
        let penalty = wound_stamina_penalty(wounds);
        let recovery = STAMINA_RECOVERY_RATE * dt * (1.0 - penalty);
        self.stamina = (self.stamina + recovery).min(1.0);
    }

    /// Drain stamina for the current movement mode.
    pub fn tick_movement_drain(&mut self, dt: f32) {
        let drain = stamina_drain_for_mode(self.movement_mode);
        self.stamina = (self.stamina - drain * dt).max(0.0);
    }

    /// Drain stamina by an explicit amount (e.g., blocking).
    pub fn drain_stamina(&mut self, amount: f32) {
        self.stamina = (self.stamina - amount).max(0.0);
    }
}

// ---------------------------------------------------------------------------
// Wound-based stamina penalty
// ---------------------------------------------------------------------------

fn zone_penalty_weight(zone: BodyZone) -> f32 {
    match zone {
        BodyZone::Torso => WOUND_PENALTY_TORSO,
        BodyZone::Legs => WOUND_PENALTY_LEGS,
        BodyZone::Head => WOUND_PENALTY_HEAD,
        BodyZone::LeftArm | BodyZone::RightArm => WOUND_PENALTY_ARMS,
    }
}

/// Compute the stamina recovery penalty from wounds.
/// `wound_penalty = clamp(sum(zone_weight * sum(severity_weights_in_zone)), 0, 1)`
pub fn wound_stamina_penalty(wounds: &[Wound]) -> f32 {
    let mut penalty: f32 = 0.0;
    for zone in BodyZone::ALL {
        let zone_weight = zone_wound_weight(wounds, zone);
        penalty += zone_penalty_weight(zone) * zone_weight;
    }
    penalty.clamp(0.0, 1.0)
}

/// Stamina drain per tick for a movement mode.
pub fn stamina_drain_for_mode(mode: MovementMode) -> f32 {
    match mode {
        MovementMode::Walk => 0.0,
        MovementMode::Run => RUN_DRAIN,
        MovementMode::Sprint => SPRINT_DRAIN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::state::EntityKey;
    use super::super::armor::DamageType;
    use super::super::wound::{Severity, Wound};
    use slotmap::SlotMap;

    fn dummy_attacker() -> EntityKey {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        sm.insert(())
    }

    fn make_wound(zone: BodyZone, severity: Severity, tick: u64) -> Wound {
        let bleed = match severity {
            Severity::Scratch => 0.001,
            Severity::Laceration => 0.005,
            Severity::Puncture => 0.01,
            Severity::Fracture => 0.003,
        };
        Wound {
            zone,
            severity,
            bleed_rate: bleed,
            damage_type: DamageType::Slash,
            attacker_id: dummy_attacker(),
            created_at: tick,
        }
    }

    #[test]
    fn fresh_vitals() {
        let v = Vitals::new();
        assert_eq!(v.blood, 1.0);
        assert_eq!(v.stamina, 1.0);
        assert_eq!(v.stagger_ticks, 0);
        assert!(!v.is_collapsed());
        assert!(!v.is_dead());
        assert!(!v.is_staggered());
        assert!((v.effectiveness() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn effectiveness_degrades_below_threshold() {
        let mut v = Vitals::new();
        v.blood = 0.3;
        let eff = v.effectiveness();
        assert!(
            (eff - 0.6).abs() < 0.01,
            "at 0.3 blood, effectiveness should be 0.6, got {eff}"
        );
    }

    #[test]
    fn effectiveness_at_zero_is_zero() {
        let mut v = Vitals::new();
        v.blood = 0.0;
        assert!((v.effectiveness()).abs() < f32::EPSILON);
    }

    #[test]
    fn collapse_below_threshold() {
        let mut v = Vitals::new();
        v.blood = 0.19;
        assert!(v.is_collapsed());
        v.blood = 0.21;
        assert!(!v.is_collapsed());
    }

    #[test]
    fn death_at_zero() {
        let mut v = Vitals::new();
        v.blood = 0.0;
        assert!(v.is_dead());
        v.blood = 0.01;
        assert!(!v.is_dead());
    }

    #[test]
    fn bleed_drains_blood() {
        let mut v = Vitals::new();
        let wounds = vec![
            make_wound(BodyZone::Torso, Severity::Laceration, 0),
            make_wound(BodyZone::LeftArm, Severity::Scratch, 0),
        ];
        v.tick_bleed(&wounds, 0, 1.0);
        // Expected: 1.0 - (0.005 + 0.001) * 1.0 = 0.994
        assert!(
            (v.blood - 0.994).abs() < 0.001,
            "blood should be ~0.994, got {}",
            v.blood
        );
    }

    #[test]
    fn cumulative_bleed_kills() {
        let mut v = Vitals::new();
        let wounds = vec![
            make_wound(BodyZone::Torso, Severity::Puncture, 0),
            make_wound(BodyZone::Torso, Severity::Puncture, 0),
        ];
        // Two punctures: 0.02/tick. At dt=1.0, 50 ticks to bleed from 1.0 to 0.0
        // (ignoring clotting, which slows it)
        for tick in 0..200 {
            v.tick_bleed(&wounds, tick, 1.0);
        }
        assert!(v.is_dead(), "should be dead after prolonged bleeding");
    }

    #[test]
    fn blood_clamps_at_zero() {
        let mut v = Vitals::new();
        v.blood = 0.001;
        let wounds = vec![make_wound(BodyZone::Torso, Severity::Puncture, 0)];
        v.tick_bleed(&wounds, 0, 1.0);
        assert_eq!(v.blood, 0.0);
    }

    #[test]
    fn stamina_recovery_unwounded() {
        let mut v = Vitals::new();
        v.stamina = 0.0;
        v.tick_stamina_recovery(&[], 1.0);
        assert!(
            (v.stamina - STAMINA_RECOVERY_RATE).abs() < f32::EPSILON,
            "should recover {STAMINA_RECOVERY_RATE} per tick, got {}",
            v.stamina
        );
    }

    #[test]
    fn stamina_recovery_torso_wound_heavy_penalty() {
        let mut v = Vitals::new();
        v.stamina = 0.0;
        let wounds = vec![make_wound(BodyZone::Torso, Severity::Laceration, 0)];
        v.tick_stamina_recovery(&wounds, 1.0);
        // Torso laceration: zone_wound_weight = 0.4, zone_penalty_weight = 1.0
        // penalty = 1.0 * 0.4 = 0.4
        // recovery = 0.05 * 1.0 * (1.0 - 0.4) = 0.03
        assert!(
            v.stamina < STAMINA_RECOVERY_RATE,
            "torso wound should reduce recovery"
        );
        assert!(
            (v.stamina - 0.03).abs() < 0.001,
            "expected ~0.03, got {}",
            v.stamina
        );
    }

    #[test]
    fn stamina_recovery_arm_wound_light_penalty() {
        let mut v = Vitals::new();
        v.stamina = 0.0;
        let wounds = vec![make_wound(BodyZone::LeftArm, Severity::Laceration, 0)];
        v.tick_stamina_recovery(&wounds, 1.0);
        // Arm laceration: zone_wound_weight = 0.4, zone_penalty_weight = 0.2
        // penalty = 0.2 * 0.4 = 0.08
        // recovery = 0.05 * (1.0 - 0.08) = 0.046
        assert!(
            v.stamina > 0.04,
            "arm wound penalty should be light, got {}",
            v.stamina
        );
    }

    #[test]
    fn stamina_clamps_at_one() {
        let mut v = Vitals::new();
        v.stamina = 0.99;
        v.tick_stamina_recovery(&[], 1.0);
        assert_eq!(v.stamina, 1.0);
    }

    #[test]
    fn stagger_clamped() {
        let mut v = Vitals::new();
        v.apply_stagger(100.0, 10.0, 0.1);
        assert!(v.stagger_ticks >= 2 && v.stagger_ticks <= 5);
        assert!(v.is_staggered());
    }

    #[test]
    fn stagger_minimum_two() {
        let mut v = Vitals::new();
        v.apply_stagger(10.0, 10.0, 0.01); // force barely above threshold
        assert_eq!(v.stagger_ticks, 2);
    }

    #[test]
    fn stagger_ticks_down() {
        let mut v = Vitals::new();
        v.stagger_ticks = 3;
        v.tick_stamina_recovery(&[], 1.0);
        assert_eq!(v.stagger_ticks, 2);
        v.tick_stamina_recovery(&[], 1.0);
        assert_eq!(v.stagger_ticks, 1);
        v.tick_stamina_recovery(&[], 1.0);
        assert_eq!(v.stagger_ticks, 0);
        assert!(!v.is_staggered());
    }

    #[test]
    fn walk_no_stamina_drain() {
        let mut v = Vitals::new();
        v.movement_mode = MovementMode::Walk;
        v.tick_movement_drain(1.0);
        assert_eq!(v.stamina, 1.0);
    }

    #[test]
    fn run_drains_stamina() {
        let mut v = Vitals::new();
        v.movement_mode = MovementMode::Run;
        v.tick_movement_drain(1.0);
        assert!(
            (v.stamina - (1.0 - RUN_DRAIN)).abs() < f32::EPSILON,
            "run should drain {RUN_DRAIN}, got stamina {}",
            v.stamina
        );
    }

    #[test]
    fn sprint_drains_stamina_faster_than_run() {
        let mut v_run = Vitals::new();
        v_run.movement_mode = MovementMode::Run;
        v_run.tick_movement_drain(1.0);

        let mut v_sprint = Vitals::new();
        v_sprint.movement_mode = MovementMode::Sprint;
        v_sprint.tick_movement_drain(1.0);

        assert!(v_sprint.stamina < v_run.stamina, "sprint should drain more than run");
    }

    #[test]
    fn stamina_clamps_at_zero_on_drain() {
        let mut v = Vitals::new();
        v.stamina = 0.01;
        v.drain_stamina(0.5);
        assert_eq!(v.stamina, 0.0);
    }

    #[test]
    fn wound_penalty_all_zones() {
        // One fracture per zone should produce near-max penalty
        let wounds: Vec<Wound> = BodyZone::ALL
            .iter()
            .map(|&z| make_wound(z, Severity::Fracture, 0))
            .collect();
        let penalty = wound_stamina_penalty(&wounds);
        // Fracture weight = 1.0
        // Total: 1.0*1.0 + 0.5*1.0 + 0.3*1.0 + 0.2*1.0 + 0.2*1.0 = 2.2 → clamped to 1.0
        assert!(
            (penalty - 1.0).abs() < f32::EPSILON,
            "heavy wounds all zones should max penalty, got {penalty}"
        );
    }
}
