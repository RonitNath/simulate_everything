use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use super::spatial::Vec3;
use crate::v2::state::EntityKey;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Action {
    MoveTo { target: Vec3 },
    WorkAt { target: EntityKey, duration: f32 },
    ConsumeStockpile,
    AttackTarget { target: EntityKey },
    FleeFrom { threat: EntityKey, distance: f32 },
    Rest { duration: f32 },
    SocializeAt { target: EntityKey, duration: f32 },
    Wait { duration: f32 },
}

impl Action {
    pub fn label(&self) -> String {
        match self {
            Action::MoveTo { .. } => "MoveTo".to_string(),
            Action::WorkAt { .. } => "WorkAt".to_string(),
            Action::ConsumeStockpile => "Consume".to_string(),
            Action::AttackTarget { .. } => "Attack".to_string(),
            Action::FleeFrom { .. } => "Flee".to_string(),
            Action::Rest { .. } => "Rest".to_string(),
            Action::SocializeAt { .. } => "Socialize".to_string(),
            Action::Wait { .. } => "Wait".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CurrentAction {
    pub action: Action,
    pub progress: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ActionQueue {
    pub queued: VecDeque<Action>,
    pub current: Option<CurrentAction>,
}

impl ActionQueue {
    pub fn clear(&mut self) {
        self.queued.clear();
        self.current = None;
    }

    pub fn is_empty(&self) -> bool {
        self.current.is_none() && self.queued.is_empty()
    }

    pub fn preview(&self, max_items: usize) -> Vec<String> {
        let mut labels = Vec::new();
        if let Some(current) = self.current.as_ref() {
            labels.push(current.action.label());
        }
        labels.extend(self.queued.iter().take(max_items.saturating_sub(labels.len())).map(Action::label));
        labels
    }
}
