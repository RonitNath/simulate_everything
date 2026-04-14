use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocialMemory {
    pub tick: u64,
    pub counterpart_id: u32,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocialState {
    pub personality: [i8; 8],
    pub relationship_cache: SmallVec<[(u32, i16); 8]>,
    pub memory: SmallVec<[SocialMemory; 8]>,
}

impl Default for SocialState {
    fn default() -> Self {
        Self {
            personality: [0; 8],
            relationship_cache: SmallVec::new(),
            memory: SmallVec::new(),
        }
    }
}

impl SocialState {
    pub fn remember(&mut self, tick: u64, counterpart_id: u32, summary: impl Into<String>) {
        if self.memory.len() == 8 {
            self.memory.remove(0);
        }
        self.memory.push(SocialMemory {
            tick,
            counterpart_id,
            summary: summary.into(),
        });
        if let Some(existing) = self
            .relationship_cache
            .iter_mut()
            .find(|(id, _)| *id == counterpart_id)
        {
            existing.1 = existing.1.saturating_add(1);
            return;
        }
        if self.relationship_cache.len() == 8 {
            self.relationship_cache.remove(0);
        }
        self.relationship_cache.push((counterpart_id, 1));
    }
}
