use serde::{Deserialize, Serialize};

use super::armor::{ArmorProperties, BodyZone, ZONE_COUNT};
use super::weapon::WeaponProperties;
use crate::v2::state::EntityKey;

// ---------------------------------------------------------------------------
// Equipment component
// ---------------------------------------------------------------------------

/// Equipment slots on an entity that can carry weapons and armor.
/// Weapon/armor entities are `contained_in` the wearer; this component
/// references them by EntityKey.
///
/// Shield dual-role:
/// - `shield` slot (weapon slot): WeaponProperties active (block_arc,
///   block_efficiency used by D2's block check). ArmorProperties ignored.
/// - `back` slot: ArmorProperties active (covers torso as passive armor).
///   WeaponProperties ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Equipment {
    /// Active weapon (melee or ranged).
    pub weapon: Option<EntityKey>,
    /// Shield in weapon slot (active blocking).
    pub shield: Option<EntityKey>,
    /// Shield slung on back (passive torso armor) or other item.
    pub back: Option<EntityKey>,
    /// Per-zone armor references. Multiple slots can point to the same
    /// EntityKey (one armor piece covering multiple zones).
    pub armor_slots: [Option<EntityKey>; ZONE_COUNT],
}

impl Equipment {
    pub fn empty() -> Self {
        Self {
            weapon: None,
            shield: None,
            back: None,
            armor_slots: [None; ZONE_COUNT],
        }
    }
}

// ---------------------------------------------------------------------------
// Equipment validation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquipError {
    /// Weapon requires 2 hands but shield slot is occupied.
    HandsConflict,
    /// Armor entity doesn't cover the target zone.
    ZoneNotCovered,
}

/// Validate that equipment slots are consistent.
/// `weapon_props` resolves an EntityKey to its WeaponProperties (if it's a weapon).
pub fn validate_equipment<F>(equipment: &Equipment, weapon_props: F) -> Result<(), EquipError>
where
    F: Fn(EntityKey) -> Option<WeaponProperties>,
{
    // Check hands_required vs shield
    if let Some(weapon_key) = equipment.weapon {
        if let Some(props) = weapon_props(weapon_key) {
            if props.hands_required >= 2 && equipment.shield.is_some() {
                return Err(EquipError::HandsConflict);
            }
        }
    }
    Ok(())
}

/// Equip an armor entity to the appropriate zones on an Equipment component.
/// The armor's `zones_covered` determines which slots are filled.
pub fn equip_armor(
    equipment: &mut Equipment,
    armor_key: EntityKey,
    armor: &ArmorProperties,
) {
    for zone in &armor.zones_covered {
        let idx = zone_index(*zone);
        equipment.armor_slots[idx] = Some(armor_key);
    }
}

/// Unequip an armor entity from all zones it occupies.
pub fn unequip_armor(equipment: &mut Equipment, armor_key: EntityKey) {
    for slot in &mut equipment.armor_slots {
        if *slot == Some(armor_key) {
            *slot = None;
        }
    }
}

/// Map a BodyZone to its index in the armor_slots array.
pub fn zone_index(zone: BodyZone) -> usize {
    match zone {
        BodyZone::Head => 0,
        BodyZone::Torso => 1,
        BodyZone::LeftArm => 2,
        BodyZone::RightArm => 3,
        BodyZone::Legs => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::armor;
    use super::super::weapon;
    use slotmap::SlotMap;

    fn make_key(sm: &mut SlotMap<EntityKey, ()>) -> EntityKey {
        sm.insert(())
    }

    #[test]
    fn empty_equipment_valid() {
        let eq = Equipment::empty();
        assert!(validate_equipment(&eq, |_| None).is_ok());
    }

    #[test]
    fn one_handed_weapon_with_shield_ok() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let sword_key = make_key(&mut sm);
        let shield_key = make_key(&mut sm);
        let sword = weapon::iron_sword();

        let eq = Equipment {
            weapon: Some(sword_key),
            shield: Some(shield_key),
            back: None,
            armor_slots: [None; ZONE_COUNT],
        };

        assert!(validate_equipment(&eq, |k| {
            if k == sword_key { Some(sword.clone()) } else { None }
        }).is_ok());
    }

    #[test]
    fn two_handed_weapon_rejects_shield() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let bow_key = make_key(&mut sm);
        let shield_key = make_key(&mut sm);
        let bow = weapon::wooden_bow();

        let eq = Equipment {
            weapon: Some(bow_key),
            shield: Some(shield_key),
            back: None,
            armor_slots: [None; ZONE_COUNT],
        };

        assert_eq!(
            validate_equipment(&eq, |k| {
                if k == bow_key { Some(bow.clone()) } else { None }
            }),
            Err(EquipError::HandsConflict)
        );
    }

    #[test]
    fn equip_armor_fills_zones() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let armor_key = make_key(&mut sm);
        let cuirass = armor::leather_cuirass();

        let mut eq = Equipment::empty();
        equip_armor(&mut eq, armor_key, &cuirass);

        assert_eq!(eq.armor_slots[zone_index(BodyZone::Torso)], Some(armor_key));
        // Other zones unaffected
        assert_eq!(eq.armor_slots[zone_index(BodyZone::Head)], None);
        assert_eq!(eq.armor_slots[zone_index(BodyZone::Legs)], None);
    }

    #[test]
    fn unequip_armor_clears_zones() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let armor_key = make_key(&mut sm);
        let cuirass = armor::leather_cuirass();

        let mut eq = Equipment::empty();
        equip_armor(&mut eq, armor_key, &cuirass);
        unequip_armor(&mut eq, armor_key);

        assert_eq!(eq.armor_slots[zone_index(BodyZone::Torso)], None);
    }

    #[test]
    fn multizone_armor() {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let armor_key = make_key(&mut sm);
        let hauberk = ArmorProperties {
            material: armor::MaterialType::Iron,
            construction: armor::ArmorConstruction::Chain,
            hardness: 5.0,
            thickness: 1.5,
            coverage: 0.9,
            weight: 10.0,
            zones_covered: vec![BodyZone::Torso, BodyZone::LeftArm, BodyZone::RightArm],
        };

        let mut eq = Equipment::empty();
        equip_armor(&mut eq, armor_key, &hauberk);

        assert_eq!(eq.armor_slots[zone_index(BodyZone::Torso)], Some(armor_key));
        assert_eq!(eq.armor_slots[zone_index(BodyZone::LeftArm)], Some(armor_key));
        assert_eq!(eq.armor_slots[zone_index(BodyZone::RightArm)], Some(armor_key));
        assert_eq!(eq.armor_slots[zone_index(BodyZone::Head)], None);
    }

    #[test]
    fn zone_index_all_unique() {
        let indices: Vec<usize> = BodyZone::ALL.iter().map(|z| zone_index(*z)).collect();
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                assert_ne!(indices[i], indices[j], "duplicate zone index");
            }
        }
        // All indices within bounds
        for &idx in &indices {
            assert!(idx < ZONE_COUNT);
        }
    }
}
