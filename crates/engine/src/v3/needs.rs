use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct EntityNeeds {
    pub hunger: f32,
    pub safety: f32,
    pub duty: f32,
    pub rest: f32,
    pub social: f32,
    pub shelter: f32,
}

impl Default for EntityNeeds {
    fn default() -> Self {
        Self {
            hunger: 0.15,
            safety: 0.05,
            duty: 0.2,
            rest: 0.1,
            social: 0.2,
            shelter: 0.1,
        }
    }
}

impl EntityNeeds {
    pub fn clamp_all(&mut self) {
        self.hunger = self.hunger.clamp(0.0, 1.0);
        self.safety = self.safety.clamp(0.0, 1.0);
        self.duty = self.duty.clamp(0.0, 1.0);
        self.rest = self.rest.clamp(0.0, 1.0);
        self.social = self.social.clamp(0.0, 1.0);
        self.shelter = self.shelter.clamp(0.0, 1.0);
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct NeedDecayRates {
    pub hunger: f32,
    pub safety: f32,
    pub duty: f32,
    pub rest: f32,
    pub social: f32,
    pub shelter: f32,
}

impl Default for NeedDecayRates {
    fn default() -> Self {
        Self {
            hunger: 0.0025,
            safety: -0.004,
            duty: 0.0018,
            rest: 0.0012,
            social: 0.0008,
            shelter: 0.0005,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct NeedWeights {
    pub combat_weight: f32,
    pub production_weight: f32,
    pub regional_weight: f32,
    pub recovery_weight: f32,
    pub cohesion_weight: f32,
}

impl Default for NeedWeights {
    fn default() -> Self {
        Self {
            combat_weight: 1.0,
            production_weight: 1.0,
            regional_weight: 1.0,
            recovery_weight: 1.0,
            cohesion_weight: 1.0,
        }
    }
}

pub fn apply_decay(needs: &mut EntityNeeds, rates: NeedDecayRates, ticks_elapsed: u64, dt: f32) {
    let scale = ticks_elapsed as f32 * dt.max(0.01);
    needs.hunger += rates.hunger * scale;
    needs.safety += rates.safety * scale;
    needs.duty += rates.duty * scale;
    needs.rest += rates.rest * scale;
    needs.social += rates.social * scale;
    needs.shelter += rates.shelter * scale;
    needs.clamp_all();
}
