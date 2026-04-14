use serde::{Deserialize, Serialize};

use super::armor::DamageType;
use super::armor::MaterialType;
use super::damage::Impact;
use super::spatial::Vec3;
use crate::v2::state::EntityKey;

// ---------------------------------------------------------------------------
// Weapon properties
// ---------------------------------------------------------------------------

/// Properties of a weapon entity. Covers both melee and ranged weapons.
/// Ranged-only fields are zero/false for melee weapons.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeaponProperties {
    pub material: MaterialType,
    pub damage_type: DamageType,
    /// Edge/point quality. Degrades with use (V3.1 — field exists, doesn't change yet).
    pub sharpness: f32,
    /// Resistance to deformation. Derived from material.
    pub hardness: f32,
    /// Weight in kg. Affects swing speed, cooldown, stamina cost.
    pub weight: f32,
    /// Melee range check distance in meters.
    pub reach: f32,
    /// 1 or 2. Equipment validation: 2-handed prevents shield.
    pub hands_required: u8,
    /// Angular coverage when blocking (radians). Shields have wide arcs.
    pub block_arc: f32,
    /// Stamina cost multiplier when blocking (0.0–1.0). Lower = more efficient.
    pub block_efficiency: f32,
    // -- Ranged-only fields (zero/false for melee) --
    /// Launch velocity for spawned projectile (m/s). 0 for melee.
    pub projectile_speed: f32,
    /// true = parabolic (bow), false = flat (crossbow, deferred).
    pub projectile_arc: bool,
    /// Base accuracy before skill modifier (0.0–1.0). 0 for melee.
    pub accuracy_base: f32,
    // -- Attack timing --
    /// Ticks before attack resolves.
    pub windup_ticks: u16,
    /// Fraction of windup at which attack becomes committed (0.0–1.0).
    pub commitment_fraction: f32,
    /// Base cooldown ticks after attack. Modified by weight/stamina at runtime.
    pub base_recovery: f32,
}

impl WeaponProperties {
    /// Whether this weapon fires projectiles.
    pub fn is_ranged(&self) -> bool {
        self.projectile_speed > 0.0
    }
}

// ---------------------------------------------------------------------------
// Attack state
// ---------------------------------------------------------------------------

/// Temporary component on entities currently executing an attack.
/// Removed when attack resolves or cancels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackState {
    /// The entity being attacked.
    pub target: EntityKey,
    /// The weapon entity being used.
    pub weapon: EntityKey,
    /// Ticks elapsed since attack started.
    pub progress: u16,
    /// Past commitment threshold — cannot cancel, only degrade on stagger.
    pub committed: bool,
}

impl AttackState {
    pub fn new(target: EntityKey, weapon: EntityKey) -> Self {
        Self {
            target,
            weapon,
            progress: 0,
            committed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Cooldown state
// ---------------------------------------------------------------------------

/// Tracks recovery time after an attack resolves. Entity cannot start a
/// new attack while ticks_remaining > 0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownState {
    pub ticks_remaining: u16,
}

impl CooldownState {
    pub fn new(ticks: u16) -> Self {
        Self {
            ticks_remaining: ticks,
        }
    }

    /// Tick down cooldown. Returns true when cooldown has expired.
    pub fn tick(&mut self) -> bool {
        if self.ticks_remaining > 0 {
            self.ticks_remaining -= 1;
        }
        self.ticks_remaining == 0
    }

    pub fn is_ready(&self) -> bool {
        self.ticks_remaining == 0
    }
}

// ---------------------------------------------------------------------------
// Attack tick result
// ---------------------------------------------------------------------------

/// Result of advancing an AttackState by one tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackTick {
    /// Attack is still winding up. Not yet committed.
    InProgress,
    /// Attack just crossed the commitment threshold this tick.
    Committed,
    /// Windup complete — ready to resolve.
    Ready,
}

// ---------------------------------------------------------------------------
// Stagger result
// ---------------------------------------------------------------------------

/// Result of a stagger hitting an entity mid-attack.
#[derive(Debug, Clone, Copy)]
pub enum StaggerResult {
    /// Attack was uncommitted — cancelled entirely. Begin cooldown.
    Cancelled,
    /// Attack was committed — resolves with degraded parameters.
    Degraded {
        /// Multiply dispersion by this (>1.0 = less accurate).
        accuracy_penalty: f32,
        /// Multiply swing speed / draw force by this (<1.0 = weaker).
        force_penalty: f32,
    },
}

// ---------------------------------------------------------------------------
// Melee resolution constants
// ---------------------------------------------------------------------------

/// Reference weight for cooldown scaling. A weapon at this weight has
/// no weight-based cooldown modifier.
const WEIGHT_REF: f32 = 1.0;

/// Dispersion multiplier applied when resolving a committed attack under stagger.
const STAGGER_ACCURACY_PENALTY: f32 = 2.0;

/// Swing speed multiplier applied when resolving a committed attack under stagger.
const STAGGER_FORCE_PENALTY: f32 = 0.5;

/// Base swing speed for melee weapons (m/s). Actual swing speed is
/// `BASE_SWING_SPEED / (weight / WEIGHT_REF)`.
const BASE_SWING_SPEED: f32 = 10.0;

/// Default cross-section by damage type. Simplified single float per type.
fn cross_section_for(damage_type: DamageType) -> f32 {
    match damage_type {
        DamageType::Slash => 0.5,  // blade edge
        DamageType::Pierce => 0.05, // point
        DamageType::Crush => 1.0,  // weapon face
    }
}

// ---------------------------------------------------------------------------
// Melee resolution functions
// ---------------------------------------------------------------------------

/// Advance an AttackState by one tick. Returns the state transition.
pub fn tick_attack(state: &mut AttackState, weapon: &WeaponProperties) -> AttackTick {
    state.progress += 1;

    // Check commitment threshold
    let commit_tick = (weapon.windup_ticks as f32 * weapon.commitment_fraction) as u16;
    if !state.committed && state.progress >= commit_tick {
        state.committed = true;
        if state.progress >= weapon.windup_ticks {
            return AttackTick::Ready;
        }
        return AttackTick::Committed;
    }

    if state.progress >= weapon.windup_ticks {
        AttackTick::Ready
    } else {
        AttackTick::InProgress
    }
}

/// Handle a stagger event on an entity mid-attack.
pub fn handle_stagger(state: &AttackState) -> StaggerResult {
    if state.committed {
        StaggerResult::Degraded {
            accuracy_penalty: STAGGER_ACCURACY_PENALTY,
            force_penalty: STAGGER_FORCE_PENALTY,
        }
    } else {
        StaggerResult::Cancelled
    }
}

/// Attempt to resolve a melee attack. Returns None if the target is out of reach (whiff).
///
/// `attacker_pos` and `target_pos` are current positions at resolution time.
/// `attacker_facing` is radians.
/// `stagger_penalties` is Some if the attacker was staggered during committed phase.
pub fn resolve_melee(
    weapon: &WeaponProperties,
    attacker_key: EntityKey,
    attacker_pos: Vec3,
    target_pos: Vec3,
    stagger_penalties: Option<&StaggerResult>,
    tick: u64,
) -> Option<Impact> {
    // Range check at resolution time (target may have moved during windup)
    let delta = target_pos - attacker_pos;
    let distance_2d = delta.xy().length();
    if distance_2d > weapon.reach {
        return None; // whiff
    }

    // Compute swing speed from weapon weight
    let mut swing_speed = BASE_SWING_SPEED / (weapon.weight / WEIGHT_REF);

    // Apply stagger penalties if committed-staggered
    let mut dispersion_mult = 1.0_f32;
    if let Some(StaggerResult::Degraded {
        accuracy_penalty,
        force_penalty,
    }) = stagger_penalties
    {
        dispersion_mult = *accuracy_penalty;
        swing_speed *= force_penalty;
    }

    // Height difference: positive = attacker higher
    let height_diff = attacker_pos.z - target_pos.z;

    // Attack direction: from attacker toward target (2D angle)
    let attack_direction = delta.xy().y.atan2(delta.xy().x);

    // Kinetic energy: 0.5 * mass * speed²
    let kinetic_energy = 0.5 * weapon.weight * swing_speed * swing_speed;

    Some(Impact {
        kinetic_energy,
        sharpness: weapon.sharpness,
        cross_section: cross_section_for(weapon.damage_type) * dispersion_mult,
        damage_type: weapon.damage_type,
        attack_direction,
        attacker_id: attacker_key,
        height_diff,
        tick,
    })
}

/// Compute cooldown ticks after an attack resolves.
/// Heavier weapons and lower stamina = longer recovery.
pub fn compute_cooldown(weapon: &WeaponProperties, stamina: f32) -> u16 {
    let stamina_clamped = stamina.max(0.1); // prevent division by near-zero
    let ticks = weapon.base_recovery * (weapon.weight / WEIGHT_REF) * (1.0 / stamina_clamped);
    ticks.round().max(1.0) as u16
}

// ---------------------------------------------------------------------------
// Starting weapon profiles
// ---------------------------------------------------------------------------

/// Iron sword: slash, medium reach, one-handed.
pub fn iron_sword() -> WeaponProperties {
    WeaponProperties {
        material: MaterialType::Iron,
        damage_type: DamageType::Slash,
        sharpness: 0.8,
        hardness: 5.0,
        weight: 1.2,
        reach: 1.5,
        hands_required: 1,
        block_arc: 0.5,        // ~29 degrees — narrow parry
        block_efficiency: 0.6, // moderate stamina cost
        projectile_speed: 0.0,
        projectile_arc: false,
        accuracy_base: 0.0,
        windup_ticks: 4,
        commitment_fraction: 0.5,
        base_recovery: 3.0,
    }
}

/// Wooden bow: pierce (ranged), two-handed, arc trajectory.
pub fn wooden_bow() -> WeaponProperties {
    WeaponProperties {
        material: MaterialType::Wood,
        damage_type: DamageType::Pierce,
        sharpness: 0.0, // projectile carries sharpness, not the bow
        hardness: 2.0,
        weight: 0.8,
        reach: 0.0,  // no melee capability
        hands_required: 2,
        block_arc: 0.0,
        block_efficiency: 0.0,
        projectile_speed: 50.0, // m/s
        projectile_arc: true,
        accuracy_base: 0.7,
        windup_ticks: 6, // draw time
        commitment_fraction: 0.7, // committed once bow is mostly drawn
        base_recovery: 4.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn make_keys() -> (EntityKey, EntityKey, EntityKey) {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        (sm.insert(()), sm.insert(()), sm.insert(()))
    }

    // --- W1 tests (preserved) ---

    #[test]
    fn iron_sword_is_melee() {
        let sword = iron_sword();
        assert!(!sword.is_ranged());
        assert_eq!(sword.hands_required, 1);
        assert!(sword.reach > 0.0);
    }

    #[test]
    fn wooden_bow_is_ranged() {
        let bow = wooden_bow();
        assert!(bow.is_ranged());
        assert_eq!(bow.hands_required, 2);
        assert!(bow.projectile_speed > 0.0);
        assert!(bow.projectile_arc);
    }

    #[test]
    fn attack_state_initial() {
        let (target, weapon_key, _) = make_keys();
        let state = AttackState::new(target, weapon_key);
        assert_eq!(state.progress, 0);
        assert!(!state.committed);
    }

    #[test]
    fn weapon_timing_sane() {
        let sword = iron_sword();
        assert!(sword.windup_ticks > 0);
        assert!(sword.commitment_fraction > 0.0 && sword.commitment_fraction < 1.0);
        assert!(sword.base_recovery > 0.0);

        let bow = wooden_bow();
        assert!(bow.windup_ticks > sword.windup_ticks, "bow draw should be slower");
        assert!(bow.commitment_fraction > sword.commitment_fraction, "bow commits later");
    }

    // --- W2 tests: attack tick progression ---

    #[test]
    fn tick_attack_progresses_to_committed() {
        let (target, weapon_key, _) = make_keys();
        let sword = iron_sword();
        // commitment at tick 2 (windup=4, fraction=0.5)
        let mut state = AttackState::new(target, weapon_key);

        let r1 = tick_attack(&mut state, &sword);
        assert_eq!(r1, AttackTick::InProgress);
        assert!(!state.committed);

        let r2 = tick_attack(&mut state, &sword);
        assert_eq!(r2, AttackTick::Committed);
        assert!(state.committed);
    }

    #[test]
    fn tick_attack_progresses_to_ready() {
        let (target, weapon_key, _) = make_keys();
        let sword = iron_sword();
        let mut state = AttackState::new(target, weapon_key);

        // Tick through entire windup
        let mut last = AttackTick::InProgress;
        for _ in 0..sword.windup_ticks {
            last = tick_attack(&mut state, &sword);
        }
        assert_eq!(last, AttackTick::Ready);
        assert!(state.committed);
    }

    // --- W2 tests: melee resolution ---

    #[test]
    fn melee_sword_produces_slash_impact() {
        let (attacker, _, _) = make_keys();
        let sword = iron_sword();
        let attacker_pos = Vec3::new(0.0, 0.0, 0.0);
        let target_pos = Vec3::new(1.0, 0.0, 0.0); // within reach (1.5m)

        let impact = resolve_melee(&sword, attacker, attacker_pos, target_pos, None, 100);
        assert!(impact.is_some(), "should resolve within reach");
        let impact = impact.unwrap();
        assert_eq!(impact.damage_type, DamageType::Slash);
        assert!(impact.kinetic_energy > 0.0);
        assert_eq!(impact.attacker_id, attacker);
    }

    #[test]
    fn melee_whiff_out_of_reach() {
        let (attacker, _, _) = make_keys();
        let sword = iron_sword();
        let attacker_pos = Vec3::new(0.0, 0.0, 0.0);
        let target_pos = Vec3::new(5.0, 0.0, 0.0); // far beyond reach

        let impact = resolve_melee(&sword, attacker, attacker_pos, target_pos, None, 100);
        assert!(impact.is_none(), "should whiff when out of reach");
    }

    #[test]
    fn melee_whiff_at_exact_boundary() {
        let (attacker, _, _) = make_keys();
        let sword = iron_sword();
        let attacker_pos = Vec3::new(0.0, 0.0, 0.0);
        // Just beyond reach
        let target_pos = Vec3::new(sword.reach + 0.01, 0.0, 0.0);

        let impact = resolve_melee(&sword, attacker, attacker_pos, target_pos, None, 100);
        assert!(impact.is_none(), "should whiff just beyond reach");
    }

    // --- W2 tests: stagger interaction ---

    #[test]
    fn stagger_uncommitted_cancels() {
        let (target, weapon_key, _) = make_keys();
        let state = AttackState::new(target, weapon_key);
        // progress=0, committed=false
        match handle_stagger(&state) {
            StaggerResult::Cancelled => {} // expected
            _ => panic!("uncommitted attack should cancel on stagger"),
        }
    }

    #[test]
    fn stagger_committed_degrades() {
        let (target, weapon_key, _) = make_keys();
        let mut state = AttackState::new(target, weapon_key);
        state.committed = true;
        match handle_stagger(&state) {
            StaggerResult::Degraded {
                accuracy_penalty,
                force_penalty,
            } => {
                assert!(accuracy_penalty > 1.0, "should increase dispersion");
                assert!(force_penalty < 1.0, "should reduce swing speed");
            }
            StaggerResult::Cancelled => panic!("committed attack should degrade, not cancel"),
        }
    }

    #[test]
    fn stagger_degraded_impact_weaker() {
        let (attacker, _, _) = make_keys();
        let sword = iron_sword();
        let attacker_pos = Vec3::new(0.0, 0.0, 0.0);
        let target_pos = Vec3::new(1.0, 0.0, 0.0);

        let normal = resolve_melee(&sword, attacker, attacker_pos, target_pos, None, 100)
            .unwrap();

        let stagger = StaggerResult::Degraded {
            accuracy_penalty: STAGGER_ACCURACY_PENALTY,
            force_penalty: STAGGER_FORCE_PENALTY,
        };
        let degraded =
            resolve_melee(&sword, attacker, attacker_pos, target_pos, Some(&stagger), 100)
                .unwrap();

        assert!(
            degraded.kinetic_energy < normal.kinetic_energy,
            "staggered attack should have less KE: normal={}, degraded={}",
            normal.kinetic_energy,
            degraded.kinetic_energy
        );
        assert!(
            degraded.cross_section > normal.cross_section,
            "staggered attack should have wider cross-section (worse accuracy)"
        );
    }

    // --- W2 tests: cooldown ---

    #[test]
    fn cooldown_increases_with_weight() {
        let sword = iron_sword(); // weight 1.2
        let stamina = 1.0;
        let cd_light = compute_cooldown(&sword, stamina);

        let mut heavy = iron_sword();
        heavy.weight = 3.0;
        let cd_heavy = compute_cooldown(&heavy, stamina);

        assert!(
            cd_heavy > cd_light,
            "heavier weapon should have longer cooldown: light={cd_light}, heavy={cd_heavy}"
        );
    }

    #[test]
    fn cooldown_decreases_with_stamina() {
        let sword = iron_sword();
        let cd_full = compute_cooldown(&sword, 1.0);
        let cd_low = compute_cooldown(&sword, 0.3);

        assert!(
            cd_low > cd_full,
            "lower stamina should have longer cooldown: full={cd_full}, low={cd_low}"
        );
    }

    #[test]
    fn cooldown_at_least_one() {
        let mut light = iron_sword();
        light.weight = 0.01;
        light.base_recovery = 0.01;
        let cd = compute_cooldown(&light, 1.0);
        assert!(cd >= 1, "cooldown must be at least 1 tick");
    }

    #[test]
    fn cooldown_state_ticks_down() {
        let mut cd = CooldownState::new(3);
        assert!(!cd.is_ready());
        assert!(!cd.tick());
        assert!(!cd.tick());
        assert!(cd.tick()); // 3rd tick → ready
        assert!(cd.is_ready());
    }

    // --- W2 tests: height difference ---

    #[test]
    fn melee_height_diff_positive_when_attacker_higher() {
        let (attacker, _, _) = make_keys();
        let sword = iron_sword();
        let attacker_pos = Vec3::new(0.0, 0.0, 5.0); // elevated
        let target_pos = Vec3::new(1.0, 0.0, 0.0);

        let impact = resolve_melee(&sword, attacker, attacker_pos, target_pos, None, 100)
            .unwrap();
        assert!(
            impact.height_diff > 0.0,
            "attacker higher should give positive height_diff"
        );
    }
}
