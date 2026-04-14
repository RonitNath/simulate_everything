/// Combat observation log: raw record of every damage resolution.
///
/// Every field the 7-step damage pipeline computes is logged here for
/// future NN training data. Written to the replay stream as a parallel
/// event channel. No in-memory retention beyond the damage estimate table's
/// running statistics.
use serde::{Deserialize, Serialize};

use super::armor::{ArmorConstruction, BodyZone, DamageType, MaterialType};
use super::wound::Severity;
use crate::v2::state::EntityKey;

/// Complete record of a single combat resolution. Contains every field
/// the 7-step damage pipeline computes — no curation. Disk is cheap,
/// missing training features are expensive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatObservation {
    pub tick: u64,
    pub attacker: EntityKey,
    pub defender: EntityKey,

    // -- Weapon properties --
    pub damage_type: DamageType,
    pub weapon_material: MaterialType,
    pub weapon_sharpness: f32,
    pub weapon_hardness: f32,
    pub weapon_weight: f32,

    // -- Armor properties (at hit zone) --
    pub armor_construction: Option<ArmorConstruction>,
    pub armor_material: Option<MaterialType>,
    pub armor_hardness: f32,
    pub armor_thickness: f32,
    pub armor_coverage: f32,

    // -- Impact parameters --
    pub hit_zone: BodyZone,
    pub angle_of_incidence: f32,
    pub impact_force: f32,

    // -- Resolution results --
    pub blocked: bool,
    pub block_stamina_cost: f32,
    pub penetrated: bool,
    pub penetration_depth: f32,
    pub residual_force: f32,
    pub wound_severity: Option<Severity>,
    pub bleed_rate: f32,
    pub stagger_force: f32,
    pub stagger: bool,

    // -- Context --
    pub distance: f32,
    pub height_diff: f32,
    pub attacker_skill: f32,
    pub defender_stamina: f32,
    /// 0 = defender facing attacker, PI = rear attack.
    pub defender_facing_offset: f32,
}

/// Accumulator for combat observations during a tick. Drained by the replay
/// system after each tick.
#[derive(Debug, Clone, Default)]
pub struct CombatLog {
    observations: Vec<CombatObservation>,
}

impl CombatLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a combat observation.
    pub fn record(&mut self, obs: CombatObservation) {
        self.observations.push(obs);
    }

    /// Drain all observations (empties the log). Called by replay system.
    pub fn drain(&mut self) -> Vec<CombatObservation> {
        std::mem::take(&mut self.observations)
    }

    /// Number of observations recorded this tick.
    pub fn len(&self) -> usize {
        self.observations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.observations.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn make_keys() -> (EntityKey, EntityKey) {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        (sm.insert(()), sm.insert(()))
    }

    fn sample_obs(attacker: EntityKey, defender: EntityKey, tick: u64) -> CombatObservation {
        CombatObservation {
            tick,
            attacker,
            defender,
            damage_type: DamageType::Slash,
            weapon_material: MaterialType::Iron,
            weapon_sharpness: 0.8,
            weapon_hardness: 5.0,
            weapon_weight: 1.2,
            armor_construction: Some(ArmorConstruction::Plate),
            armor_material: Some(MaterialType::Iron),
            armor_hardness: 5.0,
            armor_thickness: 2.0,
            armor_coverage: 0.9,
            hit_zone: BodyZone::Torso,
            angle_of_incidence: 0.5,
            impact_force: 30.0,
            blocked: false,
            block_stamina_cost: 0.0,
            penetrated: true,
            penetration_depth: 1.5,
            residual_force: 10.0,
            wound_severity: Some(Severity::Laceration),
            bleed_rate: 0.1,
            stagger_force: 3.0,
            stagger: false,
            distance: 1.0,
            height_diff: 0.0,
            attacker_skill: 0.5,
            defender_stamina: 0.8,
            defender_facing_offset: 0.0,
        }
    }

    #[test]
    fn record_and_drain() {
        let (a, d) = make_keys();
        let mut log = CombatLog::new();
        assert!(log.is_empty());

        log.record(sample_obs(a, d, 1));
        log.record(sample_obs(a, d, 2));
        assert_eq!(log.len(), 2);

        let drained = log.drain();
        assert_eq!(drained.len(), 2);
        assert!(log.is_empty());
    }

    #[test]
    fn drain_empties_log() {
        let (a, d) = make_keys();
        let mut log = CombatLog::new();
        log.record(sample_obs(a, d, 1));
        let _ = log.drain();
        let second = log.drain();
        assert!(second.is_empty());
    }

    #[test]
    fn observation_serializes() {
        let (a, d) = make_keys();
        let obs = sample_obs(a, d, 42);
        // Verify serde roundtrip works (important for replay storage).
        let json = serde_json::to_string(&obs).expect("serialize");
        let _: CombatObservation = serde_json::from_str(&json).expect("deserialize");
    }
}
