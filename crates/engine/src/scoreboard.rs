use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: String,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
}

impl AgentRecord {
    pub fn games(&self) -> u32 {
        self.wins + self.losses + self.draws
    }

    pub fn win_rate(&self) -> f64 {
        let g = self.games();
        if g == 0 { 0.0 } else { self.wins as f64 / g as f64 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scoreboard {
    pub records: HashMap<String, AgentRecord>,
    pub total_games: u32,
}

impl Scoreboard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the result of a game.
    /// `participants` is the list of agent IDs that played.
    /// `winner` is the index into participants, or None for a draw.
    pub fn record(&mut self, participants: &[String], winner: Option<usize>) {
        self.total_games += 1;
        for (i, id) in participants.iter().enumerate() {
            let record = self.records.entry(id.clone()).or_insert_with(|| AgentRecord {
                id: id.clone(),
                ..Default::default()
            });
            match winner {
                Some(w) if w == i => record.wins += 1,
                Some(_) => record.losses += 1,
                None => record.draws += 1,
            }
        }
    }

    /// Sorted by win rate descending, then by games played descending.
    pub fn ranked(&self) -> Vec<AgentRecord> {
        let mut records: Vec<AgentRecord> = self.records.values().cloned().collect();
        records.sort_by(|a, b| {
            b.win_rate()
                .partial_cmp(&a.win_rate())
                .unwrap()
                .then(b.games().cmp(&a.games()))
        });
        records
    }
}
