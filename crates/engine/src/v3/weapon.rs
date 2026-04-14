use serde::{Deserialize, Serialize};

use super::armor::{DamageType, MaterialType};
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
        use slotmap::SlotMap;
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let target = sm.insert(());
        let weapon = sm.insert(());
        let state = AttackState::new(target, weapon);
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
}
