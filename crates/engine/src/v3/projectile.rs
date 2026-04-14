use serde::{Deserialize, Serialize};

use super::armor::DamageType;
use super::weapon::WeaponProperties;

// ---------------------------------------------------------------------------
// Projectile component
// ---------------------------------------------------------------------------

/// Component on in-flight projectile entities. Removed on impact (entity
/// becomes inert — retains position, loses Mobile and Projectile components).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Projectile {
    pub damage_type: DamageType,
    /// Edge/point quality of the projectile head.
    pub sharpness: f32,
    /// Material hardness of the projectile head.
    pub hardness: f32,
    /// Mass in kg. For kinetic energy calculation on impact.
    pub mass: f32,
    /// Parabolic (true) or flat (false) trajectory.
    pub arc: bool,
    /// Player who fired. For kill attribution + friendly fire detection.
    pub source_owner: u8,
}

impl Projectile {
    /// Construct an arrow projectile from a bow's weapon properties.
    /// Arrow-specific values (mass, sharpness) are fixed for V3.0 —
    /// future: projectile type on the weapon or ammo entity.
    pub fn arrow(weapon: &WeaponProperties, owner: u8) -> Self {
        Self {
            damage_type: weapon.damage_type,
            sharpness: 0.7,  // iron arrowhead
            hardness: 5.0,   // iron
            mass: 0.05,      // ~50g arrow
            arc: weapon.projectile_arc,
            source_owner: owner,
        }
    }
}

/// Gravity constant for projectile physics (m/s²).
pub const GRAVITY: f32 = 10.0;

/// Number of substeps per tick for projectile integration.
/// At dt=1.0, this gives dt_sub=0.1 (10 substeps), preventing clipping.
pub const PROJECTILE_SUBSTEPS: u32 = 10;

/// Substep dt derived from tick dt and substep count.
pub const PROJECTILE_DT_SUB: f32 = 1.0 / PROJECTILE_SUBSTEPS as f32;

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::weapon::wooden_bow;

    #[test]
    fn arrow_from_bow() {
        let bow = wooden_bow();
        let arrow = Projectile::arrow(&bow, 1);
        assert_eq!(arrow.damage_type, DamageType::Pierce);
        assert!(arrow.arc);
        assert_eq!(arrow.source_owner, 1);
        assert!(arrow.mass > 0.0 && arrow.mass < 0.2);
    }

    #[test]
    fn gravity_constant() {
        assert!((GRAVITY - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn substep_dt() {
        assert!((PROJECTILE_DT_SUB - 0.1).abs() < 1e-6);
    }
}
