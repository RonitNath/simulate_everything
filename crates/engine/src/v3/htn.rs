use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use smallvec::smallvec;

use super::action_queue::Action;
use super::spatial::Vec3;
use super::state::{GameState, StructureType};
use super::utility::Goal;
use crate::v2::state::EntityKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainKind {
    Subsistence,
    MaterialWork,
    Construction,
    Transport,
    Combat,
    Social,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    Always,
    HasFriendlyStructure(StructureType),
    EnemyNearby,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HtnMethod {
    pub name: &'static str,
    pub domain: DomainKind,
    pub goal: Goal,
    pub preconditions: SmallVec<[Condition; 4]>,
    pub expected_duration: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DomainRegistry {
    pub defaults: Vec<HtnMethod>,
    pub faction_injections: Vec<Vec<HtnMethod>>,
}

impl DomainRegistry {
    pub fn for_players(num_players: u8) -> Self {
        Self {
            defaults: default_methods(),
            faction_injections: vec![Vec::new(); num_players as usize],
        }
    }

    pub fn methods_for_goal(&self, player: Option<u8>, goal: Goal) -> Vec<&HtnMethod> {
        let mut methods: Vec<&HtnMethod> = self.defaults.iter().filter(|method| method.goal == goal).collect();
        if let Some(owner) = player {
            if let Some(extra) = self.faction_injections.get(owner as usize) {
                methods.extend(extra.iter().filter(|method| method.goal == goal));
            }
        }
        methods
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MethodTraversalRecord {
    pub path: SmallVec<[String; 4]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanResult {
    pub actions: Vec<Action>,
    pub traversal: MethodTraversalRecord,
}

pub fn decompose_goal(state: &GameState, entity_key: EntityKey, goal: Goal) -> PlanResult {
    let entity = state.entities.get(entity_key);
    let owner = entity.and_then(|entity| entity.owner);
    let mut traversal = MethodTraversalRecord::default();
    let methods = state.domain_registry.methods_for_goal(owner, goal);
    let method = methods.first().copied();
    if let Some(method) = method {
        traversal.path.push(method.name.to_string());
    }

    let actions = match goal {
        Goal::Eat => {
            if let Some(target) = nearest_friendly_structure(state, entity_key, StructureType::Farm)
                && let Some(target_pos) = state.entities.get(target).and_then(|entity| entity.pos)
            {
                vec![
                    Action::MoveTo { target: target_pos },
                    Action::WorkAt {
                        target,
                        duration: 8.0,
                    },
                    Action::ConsumeStockpile,
                ]
            } else {
                vec![Action::ConsumeStockpile, Action::Wait { duration: 2.0 }]
            }
        }
        Goal::Work | Goal::Build => {
            if let Some(target) = nearest_work_site(state, entity_key)
                && let Some(target_pos) = state.entities.get(target).and_then(|entity| entity.pos)
            {
                vec![
                    Action::MoveTo { target: target_pos },
                    Action::WorkAt {
                        target,
                        duration: 12.0,
                    },
                ]
            } else {
                vec![Action::Wait { duration: 2.0 }]
            }
        }
        Goal::Fight => {
            if let Some(target) = nearest_enemy_person(state, entity_key) {
                vec![Action::AttackTarget { target }]
            } else {
                vec![Action::Wait { duration: 1.0 }]
            }
        }
        Goal::Flee => {
            if let Some(threat) = nearest_enemy_person(state, entity_key) {
                vec![Action::FleeFrom {
                    threat,
                    distance: 80.0,
                }]
            } else {
                vec![Action::Wait { duration: 1.0 }]
            }
        }
        Goal::Rest => vec![Action::Rest { duration: 10.0 }],
        Goal::Socialize => {
            let target = nearest_friendly_structure(state, entity_key, StructureType::Village)
                .or_else(|| nearest_friendly_structure(state, entity_key, StructureType::City));
            if let Some(target) = target
                && let Some(target_pos) = state.entities.get(target).and_then(|entity| entity.pos)
            {
                vec![
                    Action::MoveTo { target: target_pos },
                    Action::SocializeAt {
                        target,
                        duration: 6.0,
                    },
                ]
            } else {
                vec![Action::Wait { duration: 1.0 }]
            }
        }
        Goal::Explore => {
            let pos = entity.and_then(|entity| entity.pos).unwrap_or(Vec3::ZERO);
            vec![Action::MoveTo {
                target: Vec3::new(pos.x + 40.0, pos.y + 10.0, pos.z),
            }]
        }
        Goal::Shelter | Goal::Trade | Goal::Idle => vec![Action::Wait { duration: 2.0 }],
    };

    PlanResult { actions, traversal }
}

fn default_methods() -> Vec<HtnMethod> {
    vec![
        HtnMethod {
            name: "HarvestAndEat",
            domain: DomainKind::Subsistence,
            goal: Goal::Eat,
            preconditions: smallvec![Condition::HasFriendlyStructure(StructureType::Farm)],
            expected_duration: 20.0,
        },
        HtnMethod {
            name: "FarmOrWorkshopDuty",
            domain: DomainKind::MaterialWork,
            goal: Goal::Work,
            preconditions: smallvec![Condition::Always],
            expected_duration: 12.0,
        },
        HtnMethod {
            name: "BuildAtWorkshop",
            domain: DomainKind::Construction,
            goal: Goal::Build,
            preconditions: smallvec![Condition::HasFriendlyStructure(StructureType::Workshop)],
            expected_duration: 15.0,
        },
        HtnMethod {
            name: "EngageNearestEnemy",
            domain: DomainKind::Combat,
            goal: Goal::Fight,
            preconditions: smallvec![Condition::EnemyNearby],
            expected_duration: 4.0,
        },
        HtnMethod {
            name: "FallbackRetreat",
            domain: DomainKind::Combat,
            goal: Goal::Flee,
            preconditions: smallvec![Condition::EnemyNearby],
            expected_duration: 3.0,
        },
        HtnMethod {
            name: "GatherAtSettlement",
            domain: DomainKind::Social,
            goal: Goal::Socialize,
            preconditions: smallvec![Condition::HasFriendlyStructure(StructureType::Village)],
            expected_duration: 8.0,
        },
    ]
}

fn nearest_friendly_structure(
    state: &GameState,
    entity_key: EntityKey,
    structure_type: StructureType,
) -> Option<EntityKey> {
    let entity = state.entities.get(entity_key)?;
    let owner = entity.owner?;
    let pos = entity.pos?;
    state
        .entities
        .iter()
        .filter(|(_, other)| other.owner == Some(owner))
        .filter(|(_, other)| {
            other
                .structure
                .as_ref()
                .map(|structure| structure.structure_type == structure_type)
                .unwrap_or(false)
        })
        .filter_map(|(key, other)| {
            let other_pos = other.pos?;
            Some((key, (other_pos.x - pos.x).powi(2) + (other_pos.y - pos.y).powi(2)))
        })
        .min_by(|(_, left), (_, right)| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(key, _)| key)
}

fn nearest_work_site(state: &GameState, entity_key: EntityKey) -> Option<EntityKey> {
    nearest_friendly_structure(state, entity_key, StructureType::Farm)
        .or_else(|| nearest_friendly_structure(state, entity_key, StructureType::Workshop))
}

fn nearest_enemy_person(state: &GameState, entity_key: EntityKey) -> Option<EntityKey> {
    let entity = state.entities.get(entity_key)?;
    let owner = entity.owner?;
    let pos = entity.pos?;
    state
        .entities
        .iter()
        .filter(|(_, other)| other.owner != Some(owner))
        .filter(|(_, other)| other.person.is_some())
        .filter_map(|(key, other)| {
            let other_pos = other.pos?;
            Some((key, (other_pos.x - pos.x).powi(2) + (other_pos.y - pos.y).powi(2)))
        })
        .min_by(|(_, left), (_, right)| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(key, _)| key)
}
