/// Damage estimate table: per-agent running statistics of combat outcomes.
///
/// Key: (DamageType, weapon MaterialType, ArmorConstruction, armor MaterialType).
/// Initialized from theoretical material physics (penetration_modifier).
/// Updated empirically from combat observations. Each agent maintains its own
/// table (fog of war — different agents observe different combats).
use std::collections::HashMap;

use super::armor::{self, ArmorConstruction, DamageType, MaterialType};
use super::combat_log::CombatObservation;
use super::wound::{self, Severity};

// ---------------------------------------------------------------------------
// Tunable constants
// ---------------------------------------------------------------------------

/// Ticks before an entry is considered stale (lower confidence in target selection).
const STALE_THRESHOLD: u64 = 500;

/// If observed wound rate diverges from prior estimate by more than this
/// fraction, reset the entry from recent observations only.
const SURPRISE_THRESHOLD: f32 = 0.3;

/// Number of recent observations to keep when surprise-resetting an entry.
const SURPRISE_WINDOW: usize = 10;

// ---------------------------------------------------------------------------
// DamageEstimate
// ---------------------------------------------------------------------------

/// Running statistics for a weapon-armor matchup.
#[derive(Debug, Clone)]
pub struct DamageEstimate {
    /// Fraction of hits that produce a wound (0.0–1.0).
    pub wound_rate: f32,
    /// Average wound severity (0.0–1.0 scale).
    pub avg_severity: f32,
    /// Fraction of hits that stagger the defender.
    pub stagger_rate: f32,
    /// Average stamina cost to the defender per hit.
    pub stamina_drain: f32,
    /// Number of observations backing this estimate.
    pub sample_count: u32,
    /// Tick of last update.
    pub last_updated: u64,
}

impl DamageEstimate {
    fn theoretical(wound_rate: f32) -> Self {
        Self {
            wound_rate,
            avg_severity: wound_rate * 0.5, // rough heuristic
            stagger_rate: wound_rate * 0.3,
            stamina_drain: 1.0 - wound_rate, // blocked hits drain more stamina
            sample_count: 0,
            last_updated: 0,
        }
    }

    /// Whether this estimate is considered stale at the given tick.
    pub fn is_stale(&self, current_tick: u64) -> bool {
        current_tick.saturating_sub(self.last_updated) > STALE_THRESHOLD
    }
}

// ---------------------------------------------------------------------------
// Table key
// ---------------------------------------------------------------------------

/// Key for the damage estimate table.
/// (damage_type, weapon_material, armor_construction, armor_material)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MatchupKey {
    pub damage_type: DamageType,
    pub weapon_material: MaterialType,
    pub armor_construction: ArmorConstruction,
    pub armor_material: MaterialType,
}

// ---------------------------------------------------------------------------
// Observation (input to table update)
// ---------------------------------------------------------------------------

/// A single combat outcome observation, fed to the table after damage resolution.
#[derive(Debug, Clone)]
pub struct MatchupObservation {
    pub key: MatchupKey,
    pub wounded: bool,
    pub severity: f32,
    pub staggered: bool,
    pub stamina_cost: f32,
    pub tick: u64,
}

pub fn observation_to_matchup(obs: &CombatObservation) -> Option<MatchupObservation> {
    let (armor_construction, armor_material) = match (obs.armor_construction, obs.armor_material) {
        (Some(construction), Some(material)) => (construction, material),
        _ => return None,
    };

    Some(MatchupObservation {
        key: MatchupKey {
            damage_type: obs.damage_type,
            weapon_material: obs.weapon_material,
            armor_construction,
            armor_material,
        },
        wounded: obs.penetrated || obs.wound_severity.is_some(),
        severity: severity_weight(obs.wound_severity),
        staggered: obs.stagger,
        stamina_cost: obs.block_stamina_cost.max(obs.residual_force.abs() * 0.02),
        tick: obs.tick,
    })
}

fn severity_weight(severity: Option<Severity>) -> f32 {
    severity
        .map(wound::wound_severity_weight)
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// DamageEstimateTable
// ---------------------------------------------------------------------------

/// Per-agent table of weapon-armor matchup estimates.
pub struct DamageEstimateTable {
    estimates: HashMap<MatchupKey, DamageEstimate>,
    /// Ring buffer of recent observations per key, for surprise-detection resets.
    recent: HashMap<MatchupKey, Vec<MatchupObservation>>,
}

impl DamageEstimateTable {
    /// Initialize with theoretical estimates from material physics.
    /// All agents start with the same physics-derived estimates.
    pub fn from_physics() -> Self {
        let mut estimates = HashMap::new();

        // Weapon materials relevant for combat (weapons are made of these).
        let weapon_materials = [
            MaterialType::Iron,
            MaterialType::Steel,
            MaterialType::Bronze,
            MaterialType::Wood,
        ];

        // Armor materials (armor is made of these).
        let armor_materials = [
            MaterialType::Iron,
            MaterialType::Steel,
            MaterialType::Bronze,
            MaterialType::Leather,
        ];

        let constructions = [
            ArmorConstruction::Plate,
            ArmorConstruction::Chain,
            ArmorConstruction::Padded,
            ArmorConstruction::Layered,
        ];

        let damage_types = [DamageType::Slash, DamageType::Pierce, DamageType::Crush];

        for &dt in &damage_types {
            for &wm in &weapon_materials {
                for &ac in &constructions {
                    for &am in &armor_materials {
                        // Use the existing penetration_modifier as the theoretical
                        // basis. Higher modifier = more penetration = higher wound rate.
                        let pen_mod = armor::penetration_modifier(dt, am, ac);

                        // Convert penetration modifier to wound rate estimate.
                        // pen_mod < 1.0 = armor resists well (low wound rate).
                        // pen_mod > 1.0 = weapon effective (high wound rate).
                        // Clamp to 0.05..0.95 — nothing is certain.
                        let wound_rate = (pen_mod * 0.5).clamp(0.05, 0.95);

                        let key = MatchupKey {
                            damage_type: dt,
                            weapon_material: wm,
                            armor_construction: ac,
                            armor_material: am,
                        };

                        estimates.insert(key, DamageEstimate::theoretical(wound_rate));
                    }
                }
            }
        }

        Self {
            estimates,
            recent: HashMap::new(),
        }
    }

    /// Create an empty table (for testing).
    pub fn empty() -> Self {
        Self {
            estimates: HashMap::new(),
            recent: HashMap::new(),
        }
    }

    /// Look up the estimate for a matchup. Returns None if no data.
    pub fn get(&self, key: &MatchupKey) -> Option<&DamageEstimate> {
        self.estimates.get(key)
    }

    /// Number of entries in the table.
    pub fn len(&self) -> usize {
        self.estimates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.estimates.is_empty()
    }

    /// Update the table with a combat observation.
    /// Uses incremental running average for wound_rate, severity, stagger, stamina.
    /// Detects surprise (divergence from prior estimate) and resets if needed.
    pub fn observe(&mut self, obs: MatchupObservation) {
        let tick = obs.tick;

        // Store in recent buffer.
        let recent_buf = self.recent.entry(obs.key).or_default();
        if recent_buf.len() >= SURPRISE_WINDOW * 2 {
            // Keep only the last SURPRISE_WINDOW entries.
            let drain_to = recent_buf.len() - SURPRISE_WINDOW;
            recent_buf.drain(..drain_to);
        }
        recent_buf.push(obs.clone());

        let entry = self.estimates.entry(obs.key).or_insert_with(|| {
            DamageEstimate::theoretical(0.5) // no physics data, use neutral
        });

        let _prior_wound_rate = entry.wound_rate;

        // Incremental update.
        let n = entry.sample_count as f32 + 1.0;
        let wounded_f = if obs.wounded { 1.0 } else { 0.0 };
        let staggered_f = if obs.staggered { 1.0 } else { 0.0 };

        entry.wound_rate += (wounded_f - entry.wound_rate) / n;
        entry.avg_severity += (obs.severity - entry.avg_severity) / n;
        entry.stagger_rate += (staggered_f - entry.stagger_rate) / n;
        entry.stamina_drain += (obs.stamina_cost - entry.stamina_drain) / n;
        entry.sample_count += 1;
        entry.last_updated = tick;

        // Surprise detection: compare recent window against overall running
        // average. If they diverge significantly, conditions have changed —
        // reset from the recent window only.
        if entry.sample_count > SURPRISE_WINDOW as u32 * 2
            && let Some(recent) = self.recent.get(&obs.key)
            && recent.len() >= SURPRISE_WINDOW
        {
            let window = &recent[recent.len() - SURPRISE_WINDOW..];
            let recent_wound_rate =
                window.iter().filter(|o| o.wounded).count() as f32 / SURPRISE_WINDOW as f32;

            if (recent_wound_rate - entry.wound_rate).abs() > SURPRISE_THRESHOLD {
                let new_severity: f32 =
                    window.iter().map(|o| o.severity).sum::<f32>() / SURPRISE_WINDOW as f32;
                let new_stagger: f32 =
                    window.iter().filter(|o| o.staggered).count() as f32 / SURPRISE_WINDOW as f32;
                let new_stamina: f32 =
                    window.iter().map(|o| o.stamina_cost).sum::<f32>() / SURPRISE_WINDOW as f32;

                entry.wound_rate = recent_wound_rate;
                entry.avg_severity = new_severity;
                entry.stagger_rate = new_stagger;
                entry.stamina_drain = new_stamina;
                entry.sample_count = SURPRISE_WINDOW as u32;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> MatchupKey {
        MatchupKey {
            damage_type: DamageType::Slash,
            weapon_material: MaterialType::Iron,
            armor_construction: ArmorConstruction::Plate,
            armor_material: MaterialType::Iron,
        }
    }

    fn test_obs(key: MatchupKey, wounded: bool, tick: u64) -> MatchupObservation {
        MatchupObservation {
            key,
            wounded,
            severity: if wounded { 0.5 } else { 0.0 },
            staggered: false,
            stamina_cost: 0.3,
            tick,
        }
    }

    fn combat_obs(
        armor_construction: Option<ArmorConstruction>,
        armor_material: Option<MaterialType>,
        wound_severity: Option<Severity>,
    ) -> CombatObservation {
        use crate::v2::state::EntityKey;
        use slotmap::SlotMap;

        let mut sm = SlotMap::<EntityKey, ()>::with_key();
        let attacker = sm.insert(());
        let defender = sm.insert(());
        CombatObservation {
            tick: 7,
            attacker,
            defender,
            damage_type: DamageType::Slash,
            weapon_material: MaterialType::Iron,
            weapon_sharpness: 0.8,
            weapon_hardness: 5.0,
            weapon_weight: 1.2,
            armor_construction,
            armor_material,
            armor_hardness: 4.0,
            armor_thickness: 2.0,
            armor_coverage: 0.8,
            hit_zone: super::super::armor::BodyZone::Torso,
            angle_of_incidence: 0.3,
            impact_force: 20.0,
            attack_motion: super::super::martial::AttackMotion::Forehand,
            blocked: false,
            block_maneuver: None,
            block_stamina_cost: 0.4,
            penetrated: wound_severity.is_some(),
            penetration_depth: 0.8,
            residual_force: 10.0,
            wound_severity,
            bleed_rate: 0.1,
            stagger_force: 2.0,
            stagger: true,
            distance: 1.0,
            height_diff: 0.0,
            attacker_skill: 0.5,
            defender_skill: 0.4,
            defender_stamina: 0.7,
            defender_facing_offset: 0.0,
        }
    }

    #[test]
    fn from_physics_populates_entries() {
        let table = DamageEstimateTable::from_physics();
        // 3 damage types × 4 weapon materials × 4 constructions × 4 armor materials = 192
        assert_eq!(table.len(), 192);
    }

    #[test]
    fn from_physics_all_entries_have_valid_rates() {
        let table = DamageEstimateTable::from_physics();
        for est in table.estimates.values() {
            assert!(
                est.wound_rate >= 0.05 && est.wound_rate <= 0.95,
                "wound_rate {} out of range",
                est.wound_rate
            );
            assert!(est.sample_count == 0, "theoretical should have 0 samples");
        }
    }

    #[test]
    fn slash_vs_plate_low_wound_rate() {
        let table = DamageEstimateTable::from_physics();
        let key = MatchupKey {
            damage_type: DamageType::Slash,
            weapon_material: MaterialType::Iron,
            armor_construction: ArmorConstruction::Plate,
            armor_material: MaterialType::Steel,
        };
        let est = table.get(&key).unwrap();
        assert!(
            est.wound_rate < 0.3,
            "slash vs steel plate should have low wound rate: {}",
            est.wound_rate
        );
    }

    #[test]
    fn crush_vs_padded_low_wound_rate() {
        let table = DamageEstimateTable::from_physics();
        let key = MatchupKey {
            damage_type: DamageType::Crush,
            weapon_material: MaterialType::Iron,
            armor_construction: ArmorConstruction::Padded,
            armor_material: MaterialType::Leather,
        };
        let est = table.get(&key).unwrap();
        assert!(
            est.wound_rate < 0.4,
            "crush vs leather padded should have low wound rate: {}",
            est.wound_rate
        );
    }

    #[test]
    fn pierce_vs_padded_high_wound_rate() {
        let table = DamageEstimateTable::from_physics();
        let key = MatchupKey {
            damage_type: DamageType::Pierce,
            weapon_material: MaterialType::Steel,
            armor_construction: ArmorConstruction::Padded,
            armor_material: MaterialType::Leather,
        };
        let est = table.get(&key).unwrap();
        assert!(
            est.wound_rate > 0.3,
            "pierce vs leather padded should have high wound rate: {}",
            est.wound_rate
        );
    }

    #[test]
    fn observe_updates_running_stats() {
        let mut table = DamageEstimateTable::empty();
        let key = test_key();

        // Feed 10 observations, all wounding.
        for t in 0..10 {
            table.observe(test_obs(key, true, t));
        }

        let est = table.get(&key).unwrap();
        assert!(
            est.wound_rate > 0.9,
            "should converge near 1.0: {}",
            est.wound_rate
        );
        assert_eq!(est.sample_count, 10);
    }

    #[test]
    fn observe_mixed_outcomes() {
        let mut table = DamageEstimateTable::empty();
        let key = test_key();

        // Feed 5 wounds and 5 non-wounds.
        for t in 0..10 {
            table.observe(test_obs(key, t % 2 == 0, t));
        }

        let est = table.get(&key).unwrap();
        assert!(
            (est.wound_rate - 0.5).abs() < 0.15,
            "50/50 mix should be near 0.5: {}",
            est.wound_rate
        );
    }

    #[test]
    fn staleness_detection() {
        let table = DamageEstimateTable::from_physics();
        let key = test_key();
        let est = table.get(&key).unwrap();

        // At tick 0, theoretical entries (last_updated=0) are stale at tick 501+.
        assert!(!est.is_stale(0));
        assert!(!est.is_stale(STALE_THRESHOLD));
        assert!(est.is_stale(STALE_THRESHOLD + 1));
    }

    #[test]
    fn staleness_resets_on_observation() {
        let mut table = DamageEstimateTable::from_physics();
        let key = test_key();

        table.observe(test_obs(key, true, 1000));
        let est = table.get(&key).unwrap();
        assert_eq!(est.last_updated, 1000);
        assert!(!est.is_stale(1000));
        assert!(!est.is_stale(1500));
        assert!(est.is_stale(1501));
    }

    #[test]
    fn empty_table_returns_none() {
        let table = DamageEstimateTable::empty();
        assert!(table.get(&test_key()).is_none());
        assert!(table.is_empty());
    }

    #[test]
    fn surprise_resets_entry() {
        let mut table = DamageEstimateTable::empty();
        let key = test_key();

        // Build up an estimate of ~0.0 wound rate from 20 non-wounding hits.
        for t in 0..20 {
            table.observe(test_obs(key, false, t));
        }
        let est = table.get(&key).unwrap();
        assert!(est.wound_rate < 0.1, "should be near 0: {}", est.wound_rate);

        // Now feed 15 wounding hits in a row — surprise!
        for t in 20..35 {
            table.observe(test_obs(key, true, t));
        }
        let est = table.get(&key).unwrap();
        // After surprise reset, wound_rate should reflect the recent window.
        assert!(
            est.wound_rate > 0.5,
            "after surprise reset, wound_rate should reflect recent: {}",
            est.wound_rate
        );
    }

    #[test]
    fn observation_to_matchup_converts_armored_hit() {
        let obs = combat_obs(
            Some(ArmorConstruction::Plate),
            Some(MaterialType::Iron),
            Some(Severity::Puncture),
        );
        let matchup = observation_to_matchup(&obs).expect("armored matchup");
        assert_eq!(matchup.key.armor_construction, ArmorConstruction::Plate);
        assert_eq!(matchup.key.armor_material, MaterialType::Iron);
        assert!(matchup.wounded);
        assert!(matchup.severity > 0.0);
        assert!(matchup.staggered);
    }

    #[test]
    fn observation_to_matchup_skips_unarmored_targets() {
        let obs = combat_obs(None, None, Some(Severity::Scratch));
        assert!(observation_to_matchup(&obs).is_none());
    }
}
