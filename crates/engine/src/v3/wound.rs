use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::armor::{BodyZone, DamageType};
use crate::v2::state::EntityKey;

// ---------------------------------------------------------------------------
// Tunable constants — severity thresholds
// ---------------------------------------------------------------------------

/// Penetration depth thresholds for severity classification.
const SCRATCH_MAX: f32 = 0.3;
const LACERATION_MAX: f32 = 0.7;

/// Per-severity bleed rates (per tick, before clotting).
const BLEED_SCRATCH: f32 = 0.001;
const BLEED_LACERATION: f32 = 0.005;
const BLEED_PUNCTURE: f32 = 0.01;
const BLEED_FRACTURE: f32 = 0.003;

// ---------------------------------------------------------------------------
// Tunable constants — clotting
// ---------------------------------------------------------------------------

/// Clot floor: minimum clot_factor (wound never fully clots below this bleed).
const CLOT_FLOOR_SCRATCH: f32 = 0.0;
const CLOT_FLOOR_LACERATION: f32 = 0.2;
const CLOT_FLOOR_PUNCTURE: f32 = 0.5;
const CLOT_FLOOR_FRACTURE: f32 = 0.1;

/// Clot half-life in ticks: how many ticks for clot_factor to halve toward floor.
const CLOT_HALFLIFE_SCRATCH: f32 = 20.0;
const CLOT_HALFLIFE_LACERATION: f32 = 60.0;
const CLOT_HALFLIFE_PUNCTURE: f32 = 100.0;
const CLOT_HALFLIFE_FRACTURE: f32 = 40.0;

// ---------------------------------------------------------------------------
// Tunable constants — wound severity weights
// ---------------------------------------------------------------------------

/// Numeric weight of each severity for cumulative effect calculations.
const WEIGHT_SCRATCH: f32 = 0.15;
const WEIGHT_LACERATION: f32 = 0.4;
const WEIGHT_PUNCTURE: f32 = 0.7;
const WEIGHT_FRACTURE: f32 = 1.0;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Wound severity, determined by penetration depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Severity {
    Scratch,
    Laceration,
    Puncture,
    Fracture,
}

/// A wound inflicted on an entity. Stored in a SmallVec<[Wound; 4]> per entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wound {
    pub zone: BodyZone,
    pub severity: Severity,
    /// Base bleed rate per tick (before clotting adjustment).
    pub bleed_rate: f32,
    /// How the wound was inflicted — stored for future medical treatment system.
    pub damage_type: DamageType,
    /// Who inflicted this wound — for kill attribution.
    pub attacker_id: EntityKey,
    /// Tick when wound was created — for clotting calculation.
    pub created_at: u64,
}

/// Per-entity wound storage. Inline for 0-4 wounds (typical combat),
/// heap-allocates for heavily wounded soldiers.
pub type WoundList = SmallVec<[Wound; 4]>;

// ---------------------------------------------------------------------------
// Severity classification
// ---------------------------------------------------------------------------

/// Classify penetration depth into severity and base bleed rate.
/// For crush damage, pass `is_crush = true` to get Fracture instead of
/// depth-based severity.
pub fn severity_for_depth(depth: f32, is_crush: bool) -> (Severity, f32) {
    if is_crush {
        return (Severity::Fracture, BLEED_FRACTURE);
    }
    if depth < SCRATCH_MAX {
        (Severity::Scratch, BLEED_SCRATCH)
    } else if depth < LACERATION_MAX {
        (Severity::Laceration, BLEED_LACERATION)
    } else {
        (Severity::Puncture, BLEED_PUNCTURE)
    }
}

// ---------------------------------------------------------------------------
// Clotting
// ---------------------------------------------------------------------------

/// Compute the clotting factor for a wound of the given severity at the given
/// age (ticks since creation).
///
/// Formula: `floor + (1.0 - floor) * 2^(-age / half_life)`
///
/// Returns a value in [floor, 1.0]. Multiply by base bleed_rate to get
/// effective bleed per tick.
pub fn clot_factor(severity: Severity, age: u64) -> f32 {
    let (floor, half_life) = match severity {
        Severity::Scratch => (CLOT_FLOOR_SCRATCH, CLOT_HALFLIFE_SCRATCH),
        Severity::Laceration => (CLOT_FLOOR_LACERATION, CLOT_HALFLIFE_LACERATION),
        Severity::Puncture => (CLOT_FLOOR_PUNCTURE, CLOT_HALFLIFE_PUNCTURE),
        Severity::Fracture => (CLOT_FLOOR_FRACTURE, CLOT_HALFLIFE_FRACTURE),
    };
    let decay = 2.0_f32.powf(-(age as f32) / half_life);
    floor + (1.0 - floor) * decay
}

/// Effective bleed rate for a wound at the current tick, accounting for clotting.
pub fn effective_bleed(wound: &Wound, current_tick: u64) -> f32 {
    let age = current_tick.saturating_sub(wound.created_at);
    wound.bleed_rate * clot_factor(wound.severity, age)
}

// ---------------------------------------------------------------------------
// Wound severity weights (for cumulative effects)
// ---------------------------------------------------------------------------

/// Numeric weight of a wound severity for cumulative effect calculation.
/// Three scratches (3 × 0.15 = 0.45) produce meaningful degradation.
pub fn wound_severity_weight(severity: Severity) -> f32 {
    match severity {
        Severity::Scratch => WEIGHT_SCRATCH,
        Severity::Laceration => WEIGHT_LACERATION,
        Severity::Puncture => WEIGHT_PUNCTURE,
        Severity::Fracture => WEIGHT_FRACTURE,
    }
}

/// Total severity weight of all wounds in a specific zone.
pub fn zone_wound_weight(wounds: &[Wound], zone: BodyZone) -> f32 {
    wounds
        .iter()
        .filter(|w| w.zone == zone)
        .map(|w| wound_severity_weight(w.severity))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    fn dummy_attacker() -> EntityKey {
        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        sm.insert(())
    }

    fn make_wound(zone: BodyZone, severity: Severity, tick: u64) -> Wound {
        let (_, bleed) = match severity {
            Severity::Scratch => (severity, BLEED_SCRATCH),
            Severity::Laceration => (severity, BLEED_LACERATION),
            Severity::Puncture => (severity, BLEED_PUNCTURE),
            Severity::Fracture => (severity, BLEED_FRACTURE),
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
    fn severity_scratch() {
        let (sev, bleed) = severity_for_depth(0.1, false);
        assert_eq!(sev, Severity::Scratch);
        assert!((bleed - BLEED_SCRATCH).abs() < f32::EPSILON);
    }

    #[test]
    fn severity_laceration() {
        let (sev, bleed) = severity_for_depth(0.5, false);
        assert_eq!(sev, Severity::Laceration);
        assert!((bleed - BLEED_LACERATION).abs() < f32::EPSILON);
    }

    #[test]
    fn severity_puncture() {
        let (sev, bleed) = severity_for_depth(1.0, false);
        assert_eq!(sev, Severity::Puncture);
        assert!((bleed - BLEED_PUNCTURE).abs() < f32::EPSILON);
    }

    #[test]
    fn severity_deep_puncture() {
        // Beyond PUNCTURE_MAX still maps to Puncture
        let (sev, _) = severity_for_depth(2.0, false);
        assert_eq!(sev, Severity::Puncture);
    }

    #[test]
    fn severity_crush_always_fracture() {
        let (sev, bleed) = severity_for_depth(0.1, true);
        assert_eq!(sev, Severity::Fracture);
        assert!((bleed - BLEED_FRACTURE).abs() < f32::EPSILON);
    }

    #[test]
    fn clot_factor_fresh_wound_is_one() {
        for sev in [
            Severity::Scratch,
            Severity::Laceration,
            Severity::Puncture,
            Severity::Fracture,
        ] {
            let cf = clot_factor(sev, 0);
            assert!(
                (cf - 1.0).abs() < 0.001,
                "fresh {sev:?} should have clot_factor ~1.0, got {cf}"
            );
        }
    }

    #[test]
    fn clot_scratch_near_zero_at_age_50() {
        let cf = clot_factor(Severity::Scratch, 50);
        // half_life=20, floor=0.0: 2^(-50/20) = 2^(-2.5) ≈ 0.177
        // With floor 0.0: cf ≈ 0.177
        assert!(
            cf < 0.2,
            "scratch at age 50 should be mostly clotted, got {cf}"
        );
    }

    #[test]
    fn clot_scratch_fully_clotted_at_age_200() {
        let cf = clot_factor(Severity::Scratch, 200);
        assert!(
            cf < 0.01,
            "scratch at age 200 should be fully clotted, got {cf}"
        );
    }

    #[test]
    fn clot_puncture_still_significant_at_age_50() {
        let cf = clot_factor(Severity::Puncture, 50);
        // half_life=100, floor=0.5: 0.5 + 0.5 * 2^(-50/100) = 0.5 + 0.5*0.707 ≈ 0.854
        assert!(
            cf > 0.5,
            "puncture at age 50 should still be significant, got {cf}"
        );
    }

    #[test]
    fn clot_puncture_never_below_floor() {
        let cf = clot_factor(Severity::Puncture, 10000);
        assert!(
            cf >= CLOT_FLOOR_PUNCTURE - f32::EPSILON,
            "puncture should never clot below floor, got {cf}"
        );
    }

    #[test]
    fn effective_bleed_decreases_over_time() {
        let wound = make_wound(BodyZone::Torso, Severity::Laceration, 0);
        let bleed_0 = effective_bleed(&wound, 0);
        let bleed_100 = effective_bleed(&wound, 100);
        assert!(
            bleed_100 < bleed_0,
            "bleed should decrease: {bleed_0} → {bleed_100}"
        );
    }

    #[test]
    fn wound_severity_weight_ordering() {
        assert!(
            wound_severity_weight(Severity::Scratch) < wound_severity_weight(Severity::Laceration)
        );
        assert!(
            wound_severity_weight(Severity::Laceration) < wound_severity_weight(Severity::Puncture)
        );
        assert!(
            wound_severity_weight(Severity::Puncture) < wound_severity_weight(Severity::Fracture)
        );
    }

    #[test]
    fn three_scratches_meaningful_degradation() {
        let wounds: Vec<Wound> = (0..3)
            .map(|_| make_wound(BodyZone::LeftArm, Severity::Scratch, 0))
            .collect();
        let weight = zone_wound_weight(&wounds, BodyZone::LeftArm);
        assert!(
            weight > 0.3,
            "three scratches should produce meaningful weight, got {weight}"
        );
    }

    #[test]
    fn zone_wound_weight_filters_zone() {
        let wounds = vec![
            make_wound(BodyZone::Torso, Severity::Laceration, 0),
            make_wound(BodyZone::Head, Severity::Scratch, 0),
            make_wound(BodyZone::Torso, Severity::Scratch, 0),
        ];
        let torso_weight = zone_wound_weight(&wounds, BodyZone::Torso);
        let head_weight = zone_wound_weight(&wounds, BodyZone::Head);
        assert!(torso_weight > head_weight);
    }

    #[test]
    fn wound_list_inline_capacity() {
        let mut wl = WoundList::new();
        for i in 0..4 {
            wl.push(make_wound(BodyZone::Torso, Severity::Scratch, i));
        }
        // SmallVec<[Wound; 4]> should not heap-allocate for 4 wounds
        assert!(!wl.spilled(), "4 wounds should fit inline");
    }
}
