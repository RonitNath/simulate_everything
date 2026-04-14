use serde::{Deserialize, Serialize};

use super::state::GameState;
use super::utility::Goal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredGoal {
    pub goal: String,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialSummary {
    pub relationship_count: usize,
    pub memory_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorEntitySnapshot {
    pub id: u32,
    pub owner: Option<u8>,
    pub role: Option<String>,
    pub pos: [f32; 3],
    pub needs: Option<simulate_everything_protocol::EntityNeedsInfo>,
    pub current_goal: Option<String>,
    pub current_action: Option<String>,
    pub action_queue_preview: Vec<String>,
    pub decision_reason: Option<String>,
    pub decision_history: Vec<String>,
    pub traversal_path: Vec<String>,
    pub top_goals: Vec<ScoredGoal>,
    pub social: Option<SocialSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorSnapshot {
    pub tick: u64,
    pub dt: f32,
    pub map_width: usize,
    pub map_height: usize,
    pub entities: Vec<BehaviorEntitySnapshot>,
}

pub fn capture_behavior_snapshot(state: &GameState, dt: f32) -> BehaviorSnapshot {
    let entities = state
        .entities
        .values()
        .filter_map(|entity| {
            let pos = entity.pos?;
            let behavior = entity.behavior.as_ref();
            Some(BehaviorEntitySnapshot {
                id: entity.id,
                owner: entity.owner,
                role: entity
                    .person
                    .as_ref()
                    .map(|person| format!("{:?}", person.role)),
                pos: [pos.x, pos.y, pos.z],
                needs: behavior.as_ref().map(|behavior| {
                    simulate_everything_protocol::EntityNeedsInfo {
                        hunger: behavior.needs.hunger,
                        safety: behavior.needs.safety,
                        duty: behavior.needs.duty,
                        rest: behavior.needs.rest,
                        social: behavior.needs.social,
                        shelter: behavior.needs.shelter,
                    }
                }),
                current_goal: behavior
                    .as_ref()
                    .and_then(|behavior| behavior.current_goal)
                    .map(|goal| goal.label().to_string()),
                current_action: behavior
                    .as_ref()
                    .and_then(|behavior| behavior.action_queue.current.as_ref())
                    .map(|current| current.action.label()),
                action_queue_preview: behavior
                    .as_ref()
                    .map(|behavior| behavior.action_queue.preview(4))
                    .unwrap_or_default(),
                decision_reason: behavior
                    .as_ref()
                    .and_then(|behavior| behavior.decision_reason.clone()),
                decision_history: behavior
                    .as_ref()
                    .map(|behavior| {
                        behavior
                            .decision_history
                            .iter()
                            .map(|record| {
                                format!("{}:{}:{}", record.tick, record.goal.label(), record.reason)
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                traversal_path: behavior
                    .as_ref()
                    .map(|behavior| behavior.mtr.path.iter().cloned().collect())
                    .unwrap_or_default(),
                top_goals: behavior
                    .as_ref()
                    .map(|behavior| top_goal_scores(behavior.needs))
                    .unwrap_or_default(),
                social: behavior.as_ref().map(|behavior| SocialSummary {
                    relationship_count: behavior.social.relationship_cache.len(),
                    memory_count: behavior.social.memory.len(),
                }),
            })
        })
        .collect();

    BehaviorSnapshot {
        tick: state.tick,
        dt,
        map_width: state.map_width,
        map_height: state.map_height,
        entities,
    }
}

fn top_goal_scores(needs: super::needs::EntityNeeds) -> Vec<ScoredGoal> {
    let mut scores = vec![
        score_goal(Goal::Eat, needs.hunger),
        score_goal(Goal::Fight, needs.safety * 0.8 + needs.duty * 0.2),
        score_goal(Goal::Rest, needs.rest),
        score_goal(Goal::Socialize, needs.social),
        score_goal(Goal::Build, needs.duty * 0.6 + needs.shelter * 0.4),
        score_goal(Goal::Work, needs.duty),
    ];
    scores.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scores.truncate(3);
    scores
}

fn score_goal(goal: Goal, score: f32) -> ScoredGoal {
    ScoredGoal {
        goal: goal.label().to_string(),
        score: score.clamp(0.0, 1.0),
    }
}
