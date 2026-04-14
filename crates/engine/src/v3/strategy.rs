/// V3 strategy layer personalities: Spread, Striker, Turtle.
///
/// Three strategy implementations differing only in posture transitions and
/// priority weighting. All read the same StrategicView; personality is the
/// policy applied to the view, not the perception.
use super::agent::{
    EconomicFocus, Posture, StackArchetype, StrategicDirective, StrategyLayer,
};
use super::perception::StrategicView;

// ---------------------------------------------------------------------------
// Tunable thresholds (all three personalities share the same structure)
// ---------------------------------------------------------------------------

/// Thresholds for posture transitions. Each personality defines different values.
struct TransitionThresholds {
    /// Territory contested ratio above which to consolidate.
    contested_ratio: f32,
    /// Strength ratio above which to attack (own/enemy soldiers).
    attack_strength_ratio: f32,
    /// Strength ratio below which to defend.
    defend_strength_ratio: f32,
    /// Economy ratio above which to attack (Turtle only uses this).
    economy_dominance: f32,
}

// ---------------------------------------------------------------------------
// Spread — economy-first
// ---------------------------------------------------------------------------

/// Economy-first personality. Defaults to Growth/Expand. Consolidates when
/// borders are contested. Attacks only when overwhelmingly strong.
pub struct SpreadStrategy {
    posture: Posture,
    focus: EconomicFocus,
    thresholds: TransitionThresholds,
}

impl SpreadStrategy {
    pub fn new() -> Self {
        Self {
            posture: Posture::Expand,
            focus: EconomicFocus::Growth,
            thresholds: TransitionThresholds {
                contested_ratio: 0.3,      // consolidate when 30%+ territory contested
                attack_strength_ratio: 3.0, // only attack at 3:1 advantage
                defend_strength_ratio: 0.5, // defend below 0.5:1
                economy_dominance: 0.0,     // not used by Spread
            },
        }
    }

    fn update_posture(&mut self, view: &StrategicView) {
        let strength_ratio = strength_ratio(view);
        let contested_ratio = contested_ratio(view);

        self.posture = if strength_ratio < self.thresholds.defend_strength_ratio {
            self.focus = EconomicFocus::Military;
            Posture::Defend
        } else if contested_ratio > self.thresholds.contested_ratio {
            self.focus = EconomicFocus::Growth;
            Posture::Consolidate
        } else if strength_ratio > self.thresholds.attack_strength_ratio {
            self.focus = EconomicFocus::Military;
            Posture::Attack
        } else {
            self.focus = EconomicFocus::Growth;
            Posture::Expand
        };
    }
}

impl StrategyLayer for SpreadStrategy {
    fn plan(&mut self, view: &StrategicView) -> Vec<StrategicDirective> {
        self.update_posture(view);
        let mut directives = vec![
            StrategicDirective::SetPosture(self.posture),
            StrategicDirective::SetEconomicFocus(self.focus),
        ];

        // Request balanced archetypes.
        match self.posture {
            Posture::Expand => {
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::Settler,
                    region: default_expansion_target(view),
                });
            }
            Posture::Attack => {
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::LightInfantry,
                    region: default_threat_region(view),
                });
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::Skirmisher,
                    region: default_threat_region(view),
                });
            }
            Posture::Defend | Posture::Consolidate => {
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::Garrison,
                    region: default_controlled_region(view),
                });
            }
        }

        add_threat_priorities(view, &mut directives);
        directives
    }
}

// ---------------------------------------------------------------------------
// Striker — military-first
// ---------------------------------------------------------------------------

/// Military-first personality. Defaults to Military/Attack. Prioritizes
/// regions near enemy territory. Defends only when critically weak.
pub struct StrikerStrategy {
    posture: Posture,
    focus: EconomicFocus,
    thresholds: TransitionThresholds,
}

impl StrikerStrategy {
    pub fn new() -> Self {
        Self {
            posture: Posture::Attack,
            focus: EconomicFocus::Military,
            thresholds: TransitionThresholds {
                contested_ratio: 0.0,      // not used
                attack_strength_ratio: 0.8, // attack even at near-parity
                defend_strength_ratio: 0.3, // only defend when critically weak
                economy_dominance: 0.0,
            },
        }
    }

    fn update_posture(&mut self, view: &StrategicView) {
        let strength_ratio = strength_ratio(view);

        self.posture = if strength_ratio < self.thresholds.defend_strength_ratio {
            self.focus = EconomicFocus::Growth; // need to rebuild
            Posture::Defend
        } else {
            self.focus = EconomicFocus::Military;
            Posture::Attack
        };
    }
}

impl StrategyLayer for StrikerStrategy {
    fn plan(&mut self, view: &StrategicView) -> Vec<StrategicDirective> {
        self.update_posture(view);
        let mut directives = vec![
            StrategicDirective::SetPosture(self.posture),
            StrategicDirective::SetEconomicFocus(self.focus),
        ];

        match self.posture {
            Posture::Attack => {
                // Offensive archetypes.
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::HeavyInfantry,
                    region: default_threat_region(view),
                });
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::Cavalry,
                    region: default_threat_region(view),
                });
            }
            Posture::Defend => {
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::Garrison,
                    region: default_controlled_region(view),
                });
            }
            _ => {}
        }

        add_threat_priorities(view, &mut directives);
        directives
    }
}

// ---------------------------------------------------------------------------
// Turtle — infrastructure-first
// ---------------------------------------------------------------------------

/// Infrastructure-first personality. Defaults to Infrastructure/Defend.
/// Attacks very late, only when economy is dominant.
pub struct TurtleStrategy {
    posture: Posture,
    focus: EconomicFocus,
    thresholds: TransitionThresholds,
}

impl TurtleStrategy {
    pub fn new() -> Self {
        Self {
            posture: Posture::Defend,
            focus: EconomicFocus::Infrastructure,
            thresholds: TransitionThresholds {
                contested_ratio: 0.0,
                attack_strength_ratio: 2.0, // need solid advantage
                defend_strength_ratio: 0.4,
                economy_dominance: 2.0, // attack when economy is 2x
            },
        }
    }

    fn update_posture(&mut self, view: &StrategicView) {
        let strength_ratio = strength_ratio(view);

        self.posture = if strength_ratio > self.thresholds.attack_strength_ratio {
            self.focus = EconomicFocus::Military;
            Posture::Attack
        } else if strength_ratio < self.thresholds.defend_strength_ratio {
            self.focus = EconomicFocus::Infrastructure;
            Posture::Defend
        } else {
            self.focus = EconomicFocus::Infrastructure;
            Posture::Defend
        };
    }
}

impl StrategyLayer for TurtleStrategy {
    fn plan(&mut self, view: &StrategicView) -> Vec<StrategicDirective> {
        self.update_posture(view);
        let mut directives = vec![
            StrategicDirective::SetPosture(self.posture),
            StrategicDirective::SetEconomicFocus(self.focus),
        ];

        match self.posture {
            Posture::Attack => {
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::HeavyInfantry,
                    region: default_threat_region(view),
                });
            }
            Posture::Defend => {
                // Defensive archetypes.
                directives.push(StrategicDirective::RequestStack {
                    archetype: StackArchetype::Garrison,
                    region: default_controlled_region(view),
                });
            }
            _ => {}
        }

        add_threat_priorities(view, &mut directives);
        directives
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Own soldiers / enemy soldiers ratio. Returns f32::MAX if no enemies visible.
fn strength_ratio(view: &StrategicView) -> f32 {
    let own = view.relative_strength.own_soldiers as f32;
    let enemy = view.relative_strength.enemy_soldiers as f32;
    if enemy < 0.5 { return f32::MAX; }
    own / enemy
}

/// Fraction of territory that is contested.
fn contested_ratio(view: &StrategicView) -> f32 {
    if view.territory.is_empty() { return 0.0; }
    let contested: u32 = view.territory.iter()
        .filter(|r| r.status == super::perception::TerritoryStatus::Contested)
        .map(|r| r.hex_count)
        .sum();
    let total: u32 = view.territory.iter().map(|r| r.hex_count).sum();
    if total == 0 { return 0.0; }
    contested as f32 / total as f32
}

/// Default region for threats (first threat position, or origin).
fn default_threat_region(view: &StrategicView) -> crate::v2::hex::Axial {
    use super::hex::world_to_hex;
    view.threats.first()
        .map(|t| world_to_hex(t.position))
        .unwrap_or(crate::v2::hex::Axial::new(0, 0))
}

/// Default controlled region center.
fn default_controlled_region(view: &StrategicView) -> crate::v2::hex::Axial {
    view.territory.iter()
        .find(|r| r.status == super::perception::TerritoryStatus::Controlled)
        .map(|r| r.center)
        .unwrap_or(crate::v2::hex::Axial::new(0, 0))
}

/// Default expansion target.
fn default_expansion_target(view: &StrategicView) -> crate::v2::hex::Axial {
    view.territory.iter()
        .find(|r| r.status == super::perception::TerritoryStatus::Unknown)
        .map(|r| r.center)
        .unwrap_or(crate::v2::hex::Axial::new(5, 5))
}

/// Add PrioritizeRegion directives for each visible threat.
fn add_threat_priorities(view: &StrategicView, directives: &mut Vec<StrategicDirective>) {
    for threat in &view.threats {
        let hex = super::hex::world_to_hex(threat.position);
        let priority = threat.entity_count as f32 / 10.0; // scale by threat size
        directives.push(StrategicDirective::PrioritizeRegion {
            center: hex,
            priority: priority.min(1.0),
        });
    }
}

// ---------------------------------------------------------------------------
// Null strategy — does nothing (for testing)
// ---------------------------------------------------------------------------

/// Strategy layer that emits no directives. For testing baseline behavior.
pub struct NullStrategy;

impl StrategyLayer for NullStrategy {
    fn plan(&mut self, _view: &StrategicView) -> Vec<StrategicDirective> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::perception::*;
    use super::super::state::StackId;
    use crate::v2::hex::Axial;

    fn base_view() -> StrategicView {
        StrategicView {
            territory: vec![
                Region { center: Axial::new(0, 0), hex_count: 20, status: TerritoryStatus::Controlled },
            ],
            relative_strength: StrengthAssessment {
                own_stacks: 2,
                enemy_stacks: 2,
                own_soldiers: 10,
                enemy_soldiers: 10,
                equipment_quality_ratio: 1.0,
            },
            economy: EconomySnapshot {
                food_surplus: 5.0,
                material_stockpile: 100.0,
                production_capacity: 3,
                growth_trend: 1.0,
            },
            threats: Vec::new(),
            stack_readiness: Vec::new(),
        }
    }

    fn view_with_strength(own: u32, enemy: u32) -> StrategicView {
        let mut v = base_view();
        v.relative_strength.own_soldiers = own;
        v.relative_strength.enemy_soldiers = enemy;
        v
    }

    fn view_with_contested(contested_count: u32, controlled_count: u32) -> StrategicView {
        let mut v = base_view();
        v.territory = vec![
            Region { center: Axial::new(0, 0), hex_count: controlled_count, status: TerritoryStatus::Controlled },
            Region { center: Axial::new(5, 5), hex_count: contested_count, status: TerritoryStatus::Contested },
        ];
        v
    }

    fn extract_posture(directives: &[StrategicDirective]) -> Option<Posture> {
        directives.iter().find_map(|d| match d {
            StrategicDirective::SetPosture(p) => Some(*p),
            _ => None,
        })
    }

    fn extract_focus(directives: &[StrategicDirective]) -> Option<EconomicFocus> {
        directives.iter().find_map(|d| match d {
            StrategicDirective::SetEconomicFocus(f) => Some(*f),
            _ => None,
        })
    }

    // --- Spread ---

    #[test]
    fn spread_defaults_to_expand_growth() {
        let mut s = SpreadStrategy::new();
        let view = base_view();
        let directives = s.plan(&view);

        assert_eq!(extract_posture(&directives), Some(Posture::Expand));
        assert_eq!(extract_focus(&directives), Some(EconomicFocus::Growth));
    }

    #[test]
    fn spread_consolidates_when_contested() {
        let mut s = SpreadStrategy::new();
        let view = view_with_contested(40, 60); // 40% contested

        let directives = s.plan(&view);
        assert_eq!(extract_posture(&directives), Some(Posture::Consolidate));
    }

    #[test]
    fn spread_attacks_when_overwhelmingly_strong() {
        let mut s = SpreadStrategy::new();
        let view = view_with_strength(40, 10); // 4:1 ratio

        let directives = s.plan(&view);
        assert_eq!(extract_posture(&directives), Some(Posture::Attack));
    }

    #[test]
    fn spread_defends_when_weak() {
        let mut s = SpreadStrategy::new();
        let view = view_with_strength(3, 10); // 0.3:1 ratio

        let directives = s.plan(&view);
        assert_eq!(extract_posture(&directives), Some(Posture::Defend));
    }

    // --- Striker ---

    #[test]
    fn striker_defaults_to_attack_military() {
        let mut s = StrikerStrategy::new();
        let view = base_view();
        let directives = s.plan(&view);

        assert_eq!(extract_posture(&directives), Some(Posture::Attack));
        assert_eq!(extract_focus(&directives), Some(EconomicFocus::Military));
    }

    #[test]
    fn striker_defends_only_when_critical() {
        let mut s = StrikerStrategy::new();
        let view = view_with_strength(2, 10); // 0.2:1 — critical

        let directives = s.plan(&view);
        assert_eq!(extract_posture(&directives), Some(Posture::Defend));
    }

    #[test]
    fn striker_attacks_at_near_parity() {
        let mut s = StrikerStrategy::new();
        let view = view_with_strength(8, 10); // 0.8:1 — near parity

        let directives = s.plan(&view);
        assert_eq!(extract_posture(&directives), Some(Posture::Attack));
    }

    // --- Turtle ---

    #[test]
    fn turtle_defaults_to_defend_infrastructure() {
        let mut s = TurtleStrategy::new();
        let view = base_view();
        let directives = s.plan(&view);

        assert_eq!(extract_posture(&directives), Some(Posture::Defend));
        assert_eq!(extract_focus(&directives), Some(EconomicFocus::Infrastructure));
    }

    #[test]
    fn turtle_attacks_when_dominant() {
        let mut s = TurtleStrategy::new();
        let view = view_with_strength(30, 10); // 3:1

        let directives = s.plan(&view);
        assert_eq!(extract_posture(&directives), Some(Posture::Attack));
    }

    #[test]
    fn turtle_stays_defensive_at_parity() {
        let mut s = TurtleStrategy::new();
        let view = view_with_strength(10, 10); // 1:1

        let directives = s.plan(&view);
        assert_eq!(extract_posture(&directives), Some(Posture::Defend));
    }

    // --- Cross-personality ---

    #[test]
    fn all_personalities_emit_posture_and_focus() {
        let view = base_view();
        for (name, mut strategy) in [
            ("Spread", Box::new(SpreadStrategy::new()) as Box<dyn StrategyLayer>),
            ("Striker", Box::new(StrikerStrategy::new()) as Box<dyn StrategyLayer>),
            ("Turtle", Box::new(TurtleStrategy::new()) as Box<dyn StrategyLayer>),
        ] {
            let directives = strategy.plan(&view);
            assert!(extract_posture(&directives).is_some(), "{name} must emit posture");
            assert!(extract_focus(&directives).is_some(), "{name} must emit focus");
        }
    }

    #[test]
    fn no_enemies_no_panic() {
        let mut view = base_view();
        view.relative_strength.enemy_soldiers = 0;
        view.relative_strength.enemy_stacks = 0;

        let mut spread = SpreadStrategy::new();
        let mut striker = StrikerStrategy::new();
        let mut turtle = TurtleStrategy::new();

        let _ = spread.plan(&view);
        let _ = striker.plan(&view);
        let _ = turtle.plan(&view);
    }
}
