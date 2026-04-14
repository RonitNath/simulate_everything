use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Shared enums — canonical definitions in protocol crate
// ---------------------------------------------------------------------------

pub use simulate_everything_protocol::{BodyZone, DamageType, ZONE_COUNT};

// ---------------------------------------------------------------------------
// Material and construction enums (engine-only)
// ---------------------------------------------------------------------------

/// Physical substance of a weapon or armor piece. Determines base hardness,
/// density, and interaction properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MaterialType {
    Iron,
    Steel,
    Bronze,
    Leather,
    Wood,
    Bone,
    Cloth,
    Stone,
}

/// How armor is shaped/assembled. Determines coverage pattern, flex behavior,
/// and structural interaction with damage types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ArmorConstruction {
    /// Rigid sheets. Deflects slash (angle matters), vulnerable at joints.
    Plate,
    /// Interlocking rings. Disperses pierce point loads, flexible.
    Chain,
    /// Layered soft material. Absorbs crush energy, light.
    Padded,
    /// Multiple materials bonded. Hybrid properties.
    Layered,
}

// ---------------------------------------------------------------------------
// Armor properties
// ---------------------------------------------------------------------------

/// Properties of an armor entity. One armor piece can cover multiple body zones
/// (e.g., a hauberk covers torso + both arms).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArmorProperties {
    pub material: MaterialType,
    pub construction: ArmorConstruction,
    /// Resistance to penetration. Derived from material.
    pub hardness: f32,
    /// Thickness in mm. Thicker = more resistance, more weight.
    pub thickness: f32,
    /// Fraction of zone surface covered (0.0–1.0). Gaps at < 1.0.
    pub coverage: f32,
    /// Weight in kg. Affects encumbrance/movement speed.
    pub weight: f32,
    /// Which body zones this armor covers.
    pub zones_covered: Vec<BodyZone>,
}

// ---------------------------------------------------------------------------
// Penetration modifier lookup
// ---------------------------------------------------------------------------

/// Returns a multiplier applied to penetration factor when a damage type
/// encounters a specific material shaped in a specific construction.
///
/// Values > 1.0 mean the damage type is effective against this armor combo.
/// Values < 1.0 mean the armor resists this damage type well.
/// 1.0 is neutral.
pub fn penetration_modifier(
    damage: DamageType,
    material: MaterialType,
    construction: ArmorConstruction,
) -> f32 {
    // Construction modifier: how the shape interacts with the damage type
    let construction_mod = match (damage, construction) {
        // Slash vs constructions
        (DamageType::Slash, ArmorConstruction::Plate) => 0.4, // plate deflects slashes
        (DamageType::Slash, ArmorConstruction::Chain) => 0.6, // chain resists slashes
        (DamageType::Slash, ArmorConstruction::Padded) => 1.2, // padding doesn't stop edges
        (DamageType::Slash, ArmorConstruction::Layered) => 0.7, // layers catch edges

        // Pierce vs constructions
        (DamageType::Pierce, ArmorConstruction::Plate) => 0.8, // plate deflects but gaps exist
        (DamageType::Pierce, ArmorConstruction::Chain) => 0.5, // chain disperses point loads
        (DamageType::Pierce, ArmorConstruction::Padded) => 1.3, // padding offers little vs points
        (DamageType::Pierce, ArmorConstruction::Layered) => 0.7,

        // Crush vs constructions
        (DamageType::Crush, ArmorConstruction::Plate) => 0.9, // rigid transmits some force
        (DamageType::Crush, ArmorConstruction::Chain) => 1.1, // chain doesn't absorb blunt
        (DamageType::Crush, ArmorConstruction::Padded) => 0.4, // padding absorbs crush
        (DamageType::Crush, ArmorConstruction::Layered) => 0.6,
    };

    // Material hardness modifier: harder materials resist more uniformly
    let material_mod = match material {
        MaterialType::Steel => 0.7,
        MaterialType::Iron => 0.8,
        MaterialType::Bronze => 0.85,
        MaterialType::Stone => 0.9,
        MaterialType::Bone => 1.0,
        MaterialType::Wood => 1.1,
        MaterialType::Leather => 1.2,
        MaterialType::Cloth => 1.5,
    };

    construction_mod * material_mod
}

// ---------------------------------------------------------------------------
// Starting armor profiles
// ---------------------------------------------------------------------------

/// Leather cuirass: soft, light, low hardness. Padded construction.
pub fn leather_cuirass() -> ArmorProperties {
    ArmorProperties {
        material: MaterialType::Leather,
        construction: ArmorConstruction::Padded,
        hardness: 2.0,
        thickness: 4.0,
        coverage: 0.85,
        weight: 3.0,
        zones_covered: vec![BodyZone::Torso],
    }
}

/// Bronze breastplate: hard, rigid, heavy. Plate construction.
pub fn bronze_breastplate() -> ArmorProperties {
    ArmorProperties {
        material: MaterialType::Bronze,
        construction: ArmorConstruction::Plate,
        hardness: 6.0,
        thickness: 2.0,
        coverage: 0.9,
        weight: 8.0,
        zones_covered: vec![BodyZone::Torso],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_zone_all_count() {
        assert_eq!(BodyZone::ALL.len(), ZONE_COUNT);
    }

    #[test]
    fn penetration_modifier_slash_vs_plate_is_low() {
        // Plate deflects slashes well
        let m = penetration_modifier(
            DamageType::Slash,
            MaterialType::Iron,
            ArmorConstruction::Plate,
        );
        assert!(m < 0.5, "slash vs iron plate should be very low: {m}");
    }

    #[test]
    fn penetration_modifier_crush_vs_padded_is_low() {
        // Padding absorbs crush
        let m = penetration_modifier(
            DamageType::Crush,
            MaterialType::Leather,
            ArmorConstruction::Padded,
        );
        assert!(m < 0.6, "crush vs leather padded should be low: {m}");
    }

    #[test]
    fn penetration_modifier_pierce_vs_padded_is_high() {
        // Padding doesn't stop piercing
        let m = penetration_modifier(
            DamageType::Pierce,
            MaterialType::Cloth,
            ArmorConstruction::Padded,
        );
        assert!(m > 1.5, "pierce vs cloth padded should be high: {m}");
    }

    #[test]
    fn penetration_modifier_all_combos_positive() {
        for damage in [DamageType::Slash, DamageType::Pierce, DamageType::Crush] {
            for material in [
                MaterialType::Iron,
                MaterialType::Steel,
                MaterialType::Bronze,
                MaterialType::Leather,
                MaterialType::Wood,
                MaterialType::Bone,
                MaterialType::Cloth,
                MaterialType::Stone,
            ] {
                for construction in [
                    ArmorConstruction::Plate,
                    ArmorConstruction::Chain,
                    ArmorConstruction::Padded,
                    ArmorConstruction::Layered,
                ] {
                    let m = penetration_modifier(damage, material, construction);
                    assert!(
                        m > 0.0,
                        "modifier must be positive: {damage:?} vs {material:?} {construction:?} = {m}"
                    );
                }
            }
        }
    }

    #[test]
    fn steel_resists_more_than_cloth() {
        let steel = penetration_modifier(
            DamageType::Slash,
            MaterialType::Steel,
            ArmorConstruction::Plate,
        );
        let cloth = penetration_modifier(
            DamageType::Slash,
            MaterialType::Cloth,
            ArmorConstruction::Plate,
        );
        assert!(
            steel < cloth,
            "steel plate should resist slash better than cloth plate"
        );
    }

    #[test]
    fn leather_cuirass_profile() {
        let armor = leather_cuirass();
        assert_eq!(armor.material, MaterialType::Leather);
        assert_eq!(armor.construction, ArmorConstruction::Padded);
        assert_eq!(armor.zones_covered, vec![BodyZone::Torso]);
        assert!(armor.weight < 5.0);
    }

    #[test]
    fn bronze_breastplate_profile() {
        let armor = bronze_breastplate();
        assert_eq!(armor.material, MaterialType::Bronze);
        assert_eq!(armor.construction, ArmorConstruction::Plate);
        assert!(armor.hardness > leather_cuirass().hardness);
        assert!(armor.weight > leather_cuirass().weight);
    }
}
