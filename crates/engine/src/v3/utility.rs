use serde::{Deserialize, Serialize};

use super::needs::{EntityNeeds, NeedWeights};
use super::state::{GameState, Role};
use crate::v2::state::EntityKey;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Goal {
    Eat,
    Work,
    Fight,
    Flee,
    Rest,
    Socialize,
    Shelter,
    Build,
    Trade,
    Explore,
    Idle,
}

impl Goal {
    pub fn label(self) -> &'static str {
        match self {
            Goal::Eat => "Eat",
            Goal::Work => "Work",
            Goal::Fight => "Fight",
            Goal::Flee => "Flee",
            Goal::Rest => "Rest",
            Goal::Socialize => "Socialize",
            Goal::Shelter => "Shelter",
            Goal::Build => "Build",
            Goal::Trade => "Trade",
            Goal::Explore => "Explore",
            Goal::Idle => "Idle",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoalChoice {
    pub goal: Goal,
    pub reason: String,
}

pub struct UtilityScorer;

impl UtilityScorer {
    pub fn choose_goal(
        state: &GameState,
        entity_key: EntityKey,
        needs: EntityNeeds,
        weights: NeedWeights,
        resolution_demand: f32,
        enemy_nearby: bool,
    ) -> GoalChoice {
        let Some(entity) = state.entities.get(entity_key) else {
            return GoalChoice {
                goal: Goal::Idle,
                reason: "entity missing".to_string(),
            };
        };
        let role = entity
            .person
            .as_ref()
            .map(|person| person.role)
            .unwrap_or(Role::Idle);

        if needs.hunger > 0.72 {
            return GoalChoice {
                goal: Goal::Eat,
                reason: format!("survival/hunger {:.2}", needs.hunger),
            };
        }
        if enemy_nearby || resolution_demand > 0.45 {
            if role == Role::Soldier || entity.combatant.is_some() {
                return GoalChoice {
                    goal: Goal::Fight,
                    reason: format!(
                        "resolution {:.2} with combat bias {:.2}",
                        resolution_demand, weights.combat_weight
                    ),
                };
            }
            return GoalChoice {
                goal: Goal::Flee,
                reason: format!("safety spike {:.2}", needs.safety.max(resolution_demand)),
            };
        }
        if needs.rest * weights.recovery_weight > 0.58 {
            return GoalChoice {
                goal: Goal::Rest,
                reason: format!("rest {:.2}", needs.rest),
            };
        }
        if needs.social * weights.cohesion_weight > 0.6 {
            return GoalChoice {
                goal: Goal::Socialize,
                reason: format!("social {:.2}", needs.social),
            };
        }
        if needs.duty * weights.production_weight > 0.35 {
            let goal = match role {
                Role::Builder => Goal::Build,
                Role::Soldier => Goal::Explore,
                _ => Goal::Work,
            };
            return GoalChoice {
                goal,
                reason: format!("duty {:.2}", needs.duty),
            };
        }
        GoalChoice {
            goal: Goal::Idle,
            reason: "no urgent bucket exceeded threshold".to_string(),
        }
    }
}
