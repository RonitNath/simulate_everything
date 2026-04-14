# V3 W1: Weapon/Armor Data Structs

Source spec: `docs/specs/v3-W-weapons.md`
Wave: 0 (no dependencies)

## Scope

Define all data types for the W domain. No simulation logic (that's W2/W3).
Tests validate construction, validation, serialization, and penetration lookup.

## Files

### `crates/engine/src/v3/armor.rs`
- `MaterialType` enum: Iron, Steel, Bronze, Leather, Wood, Bone, Cloth, Stone
- `ArmorConstruction` enum: Plate, Chain, Padded, Layered
- `DamageType` enum: Slash, Pierce, Crush
- `ArmorProperties` struct: material, construction, hardness, thickness, coverage, weight, zones_covered
- `BodyZone` enum: Head, Torso, LeftArm, RightArm, Legs (needed for zones_covered)
- `penetration_modifier(DamageType, MaterialType, ArmorConstruction) -> f32` lookup
- Tests: modifier table coverage, construction/material combos

### `crates/engine/src/v3/weapon.rs`
- `WeaponProperties` struct: all fields from spec
- `AttackState` struct: target, weapon, progress, committed
- `fn is_ranged(&self) -> bool` helper
- Starting weapon profiles: `iron_sword()`, `wooden_bow()`
- Tests: profile validation, ranged detection

### `crates/engine/src/v3/projectile.rs`
- `Projectile` struct: damage_type, sharpness, hardness, mass, arc, source_owner
- `fn from_weapon(weapon: &WeaponProperties, owner: u8) -> Projectile` constructor
- Tests: arrow construction from bow profile

### `crates/engine/src/v3/equipment.rs`
- `Equipment` struct: weapon, shield, back, armor_slots
- `ZONE_COUNT` const (5)
- `fn validate(&self, weapons: impl Fn(EntityKey) -> Option<&WeaponProperties>) -> Result<(), EquipError>`
- `EquipError` enum: HandsConflict, InvalidSlot
- Shield dual-role: document which properties are active per slot
- Starting armor profiles: `leather_cuirass()`, `bronze_breastplate()`
- Tests: hands_required validation, shield slot rules

### `crates/engine/src/v3/mod.rs`
- Add `pub mod armor;`, `pub mod weapon;`, `pub mod projectile;`, `pub mod equipment;`

## Conventions
- Use `EntityKey` from `crate::v2::state`
- Derive Serialize/Deserialize on all public types (for replay)
- Derive Debug, Clone on structs; Debug, Clone, Copy, PartialEq, Eq on enums
- No allocation in constructors
- BodyZone here is a lightweight enum; D domain will extend with zone lookup logic
