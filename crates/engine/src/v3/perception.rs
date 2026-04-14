/// Perception layer: builds a StrategicView from game state with fog of war.
/// Strategy never reads raw game state — it reads this abstraction.
use serde::{Deserialize, Serialize};

use super::spatial::Vec3;
use super::state::GameState;
use crate::v2::hex::Axial;

// ---------------------------------------------------------------------------
// StrategicView
// ---------------------------------------------------------------------------

/// Abstracted game state as seen by the strategy layer. Fog of war applied.
/// All personalities read the same view; personality is the policy, not perception.
#[derive(Debug, Clone)]
pub struct StrategicView {
    /// Clusters of hexes: controlled, contested, unknown.
    pub territory: Vec<Region>,
    /// Aggregate own vs visible enemy strength.
    pub relative_strength: StrengthAssessment,
    /// Economic snapshot: food, materials, production capacity.
    pub economy: EconomySnapshot,
    /// Enemy concentrations visible to this player.
    pub threats: Vec<ThreatCluster>,
    /// Per-stack readiness assessment.
    pub stack_readiness: Vec<StackHealth>,
}

/// A cluster of hexes with a territorial status.
#[derive(Debug, Clone)]
pub struct Region {
    pub center: Axial,
    pub hex_count: u32,
    pub status: TerritoryStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerritoryStatus {
    Controlled,
    Contested,
    Unknown,
}

/// Aggregate strength comparison.
#[derive(Debug, Clone)]
pub struct StrengthAssessment {
    /// Total own stack count.
    pub own_stacks: u32,
    /// Total visible enemy stack count.
    pub enemy_stacks: u32,
    /// Own military entity count (soldiers).
    pub own_soldiers: u32,
    /// Visible enemy military entity count.
    pub enemy_soldiers: u32,
    /// Rough equipment quality ratio (0.0 = terrible, 1.0 = parity, 2.0 = dominant).
    pub equipment_quality_ratio: f32,
}

/// Snapshot of economic state.
#[derive(Debug, Clone)]
pub struct EconomySnapshot {
    /// Net food production minus consumption per tick.
    pub food_surplus: f32,
    /// Total material stockpile.
    pub material_stockpile: f32,
    /// Number of workshops currently producing.
    pub production_capacity: u32,
    /// Population growth trend (positive = growing).
    pub growth_trend: f32,
}

/// Observed enemy concentration.
#[derive(Debug, Clone)]
pub struct ThreatCluster {
    /// Estimated center of enemy force.
    pub position: Vec3,
    /// Direction of movement (None if stationary).
    pub direction: Option<Vec3>,
    /// Estimated entity count.
    pub entity_count: u32,
    /// Whether this group is advancing, static, or retreating.
    pub posture: ThreatPosture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatPosture {
    Advancing,
    Static,
    Retreating,
}

/// Aggregate readiness of a stack.
#[derive(Debug, Clone)]
pub struct StackHealth {
    pub stack_id: super::state::StackId,
    pub readiness: Readiness,
    pub member_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// All members healthy and equipped.
    Fresh,
    /// Some members wounded but still combat-effective.
    Wounded,
    /// Majority wounded or under-equipped.
    Depleted,
}

// ---------------------------------------------------------------------------
// Perception builder
// ---------------------------------------------------------------------------

/// Build a StrategicView for a given player from current game state.
///
/// This is a placeholder implementation for A1. The full perception layer
/// with fog of war, territory clustering, and threat analysis will be
/// refined as A2-A4 drive requirements.
pub fn build_strategic_view(state: &GameState, player: u8) -> StrategicView {
    let mut own_soldiers = 0u32;
    let mut enemy_soldiers = 0u32;

    for entity in state.entities.values() {
        let owner = match entity.owner {
            Some(o) => o,
            None => continue,
        };
        if entity.person.is_none() {
            continue;
        }
        if owner == player {
            own_soldiers += 1;
        } else {
            enemy_soldiers += 1;
        }
    }

    let own_stacks = state.stacks.iter().filter(|s| s.owner == player).count() as u32;
    let enemy_stacks = state.stacks.iter().filter(|s| s.owner != player).count() as u32;

    // Stack readiness.
    let stack_readiness: Vec<StackHealth> = state
        .stacks
        .iter()
        .filter(|s| s.owner == player)
        .map(|s| {
            let member_count = s.members.len() as u32;
            let wounded_count = s
                .members
                .iter()
                .filter(|&&m| {
                    state
                        .entities
                        .get(m)
                        .and_then(|e| e.wounds.as_ref())
                        .map(|w| !w.is_empty())
                        .unwrap_or(false)
                })
                .count() as u32;

            let readiness = if wounded_count == 0 {
                Readiness::Fresh
            } else if wounded_count * 2 < member_count {
                Readiness::Wounded
            } else {
                Readiness::Depleted
            };

            StackHealth {
                stack_id: s.id,
                readiness,
                member_count,
            }
        })
        .collect();

    StrategicView {
        territory: Vec::new(), // populated in A2+
        relative_strength: StrengthAssessment {
            own_stacks,
            enemy_stacks,
            own_soldiers,
            enemy_soldiers,
            equipment_quality_ratio: 1.0, // placeholder
        },
        economy: EconomySnapshot {
            food_surplus: 0.0,
            material_stockpile: 0.0,
            production_capacity: 0,
            growth_trend: 0.0,
        },
        threats: detect_threats(state, player),
        stack_readiness,
    }
}

// ---------------------------------------------------------------------------
// Threat detection
// ---------------------------------------------------------------------------

/// Basic threat detection: group enemy entities by hex proximity into clusters.
/// Each enemy entity with a position becomes a single-entity cluster for now.
/// Future: merge nearby entities into larger clusters.
fn detect_threats(state: &GameState, player: u8) -> Vec<ThreatCluster> {
    let mut threats = Vec::new();

    for entity in state.entities.values() {
        let owner = match entity.owner {
            Some(o) => o,
            None => continue,
        };
        if owner == player { continue; }
        if entity.person.is_none() { continue; }
        let pos = match entity.pos {
            Some(p) => p,
            None => continue,
        };

        threats.push(ThreatCluster {
            position: pos,
            direction: entity.mobile.as_ref().map(|m| {
                if m.vel.length_squared() > 0.01 { m.vel } else { Vec3::ZERO }
            }),
            entity_count: 1,
            posture: ThreatPosture::Static,
        });
    }

    threats
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::lifecycle::spawn_entity;
    use super::super::movement::Mobile;
    use super::super::spatial::{GeoMaterial, Heightfield};
    use super::super::state::{Combatant, EntityBuilder, Person, Role, Stack, StackId};
    use super::super::formation::FormationType;
    use smallvec::SmallVec;

    fn test_state() -> GameState {
        let hf = Heightfield::new(10, 10, 0.0, GeoMaterial::Soil);
        GameState::new(10, 10, 2, hf)
    }

    fn spawn_soldier(state: &mut GameState, pos: Vec3, owner: u8) -> crate::v2::state::EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .person(Person { role: Role::Soldier, combat_skill: 0.5 })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals(),
        )
    }

    #[test]
    fn view_counts_soldiers() {
        let mut state = test_state();
        spawn_soldier(&mut state, Vec3::new(0.0, 0.0, 0.0), 0);
        spawn_soldier(&mut state, Vec3::new(10.0, 0.0, 0.0), 0);
        spawn_soldier(&mut state, Vec3::new(100.0, 0.0, 0.0), 1);

        let view = build_strategic_view(&state, 0);
        assert_eq!(view.relative_strength.own_soldiers, 2);
        assert_eq!(view.relative_strength.enemy_soldiers, 1);
    }

    #[test]
    fn view_counts_stacks() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(0.0, 0.0, 0.0), 0);
        let s2 = spawn_soldier(&mut state, Vec3::new(100.0, 0.0, 0.0), 1);

        let id1 = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: id1,
            owner: 0,
            members: SmallVec::from_slice(&[s1]),
            formation: FormationType::Line,
            leader: s1,
        });
        let id2 = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: id2,
            owner: 1,
            members: SmallVec::from_slice(&[s2]),
            formation: FormationType::Line,
            leader: s2,
        });

        let view = build_strategic_view(&state, 0);
        assert_eq!(view.relative_strength.own_stacks, 1);
        assert_eq!(view.relative_strength.enemy_stacks, 1);
    }

    #[test]
    fn stack_readiness_fresh_when_no_wounds() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(0.0, 0.0, 0.0), 0);

        let id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id,
            owner: 0,
            members: SmallVec::from_slice(&[s1]),
            formation: FormationType::Line,
            leader: s1,
        });

        let view = build_strategic_view(&state, 0);
        assert_eq!(view.stack_readiness.len(), 1);
        assert_eq!(view.stack_readiness[0].readiness, Readiness::Fresh);
    }
}
