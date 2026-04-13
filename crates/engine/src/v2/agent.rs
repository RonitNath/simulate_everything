use slotmap::Key;
use std::collections::HashMap;

use super::directive::Directive;
use super::hex::{self, Axial};
use super::observation::{
    InitialObservation, NewScoutedHex, Observation, ObservationDelta, UnitInfo,
    apply_delta_to_observation, materialize_observation,
};
use super::state::{CargoType, Role, UnitKey};
use super::{
    SETTLEMENT_SUPPORT_RADIUS, SETTLEMENT_THRESHOLD, SETTLER_CONVOY_SIZE, SOLDIERS_PER_UNIT,
    UNIT_FOOD_COST, UNIT_MATERIAL_COST,
};

pub trait Agent: Send {
    fn name(&self) -> &str;
    fn init(&mut self, obs: &InitialObservation);
    fn act(&mut self, delta: &ObservationDelta) -> Vec<Directive>;
    fn reset(&mut self) {}
    fn mode(&self) -> Option<&str> { None }
}

fn seed_observation(obs: &InitialObservation) -> Observation {
    materialize_observation(
        obs,
        &ObservationDelta {
            tick: 0,
            player: obs.player,
            newly_scouted: obs
                .scouted
                .iter()
                .enumerate()
                .filter(|(_, s)| **s)
                .map(|(index, _)| NewScoutedHex {
                    index,
                    terrain: obs.terrain[index],
                    material: obs.material_map[index],
                    height: obs.height_map[index],
                })
                .collect(),
            hex_changes: Vec::new(),
            own_units: Vec::new(),
            visible_enemies: Vec::new(),
            own_population: Vec::new(),
            visible_enemy_population: Vec::new(),
            own_convoys: Vec::new(),
            visible_enemy_convoys: Vec::new(),
            visible: vec![false; obs.width * obs.height],
            total_food: 0.0,
            total_material: 0.0,
        },
    )
}

/// All known V2 agent names.
pub fn builtin_agent_names() -> &'static [&'static str] {
    &["spread", "striker", "turtle"]
}

/// Create a V2 agent by name. Returns None for unknown names.
pub fn agent_by_name(name: &str) -> Option<Box<dyn Agent>> {
    match name {
        "spread" => Some(Box::new(SpreadAgent::new())),
        "striker" => Some(Box::new(StrikerAgent::new())),
        "turtle" => Some(Box::new(TurtleAgent::new())),
        _ => None,
    }
}

pub struct SpreadAgent {
    pending_settlement: Option<Axial>,
    cached_observation: Option<Observation>,
}

impl SpreadAgent {
    pub fn new() -> Self {
        Self {
            pending_settlement: None,
            cached_observation: None,
        }
    }
    
    fn decide(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();
        let general = obs.own_units.iter().find(|u| u.is_general);
        let general_hex = general.map(|u| Axial::new(u.q, u.r));
        let settlements = settlement_hexes(obs);

        if let Some(target) = self.pending_settlement {
            if settlement_hexes(obs).contains(&target) {
                self.pending_settlement = None;
            } else if let Some(convoy) = obs
                .own_convoys
                .iter()
                .find(|c| c.cargo_type == CargoType::Settlers)
            {
                directives.push(Directive::SendConvoy {
                    convoy_id: convoy.id,
                    dest_q: target.q,
                    dest_r: target.r,
                });
            }
        }

        if let Some(hex) = general_hex {
            manage_settlement_infrastructure(obs, hex, &mut directives);
            manage_settlement_population(obs, hex, &mut directives);
            produce_units_at_settlement(obs, hex, &mut directives);
            try_send_settlers(obs, hex, &settlements, &mut self.pending_settlement, &mut directives);
            load_surplus_convoys(obs, general_hex, &mut directives);

            // Manage remote settlements
            for &settlement in &settlements {
                if Some(settlement) == general_hex {
                    continue;
                }
                manage_settlement_infrastructure(obs, settlement, &mut directives);
                manage_settlement_population(obs, settlement, &mut directives);
                produce_units_at_settlement(obs, settlement, &mut directives);
            }
        }

        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();
        let friendly_near_enemy: HashMap<UnitKey, usize> = count_friendlies_near_enemies(obs);

        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let enemy_target = enemy_direction(obs);

        for (idx, unit) in obs.own_units.iter().enumerate() {
            if !unit.engagements.is_empty() {
                continue;
            }
            if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos, &friendly_near_enemy) {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }
            if unit.is_general {
                if obs.own_units.len() > 10 {
                    let gen_pos = Axial::new(unit.q, unit.r);
                    if hex::distance(gen_pos, map_center) > 5 {
                        directives.push(Directive::Move {
                            unit_id: unit.id,
                            q: map_center.q,
                            r: map_center.r,
                        });
                    }
                }
                continue;
            }

            let dest = if obs.own_units.len() <= 8 {
                pick_sector_destination(unit, idx, obs, map_center)
            } else {
                pick_lane_destination(unit, obs, enemy_target)
            };
            if let Some(d) = dest {
                directives.push(Directive::Move {
                    unit_id: unit.id,
                    q: d.q,
                    r: d.r,
                });
                if let Some(idx) = cell_index(obs, Axial::new(unit.q, unit.r)) {
                    if obs.stockpile_owner[idx] == Some(obs.player) && obs.road_levels[idx] == 0 {
                        directives.push(Directive::BuildRoad {
                            hex_q: unit.q,
                            hex_r: unit.r,
                            level: 1,
                        });
                    }
                }
            }
        }

        directives
    }
}

impl Agent for SpreadAgent {
    fn name(&self) -> &str {
        "spread"
    }

    fn init(&mut self, obs: &InitialObservation) {
        self.cached_observation = Some(seed_observation(obs));
    }

    fn act(&mut self, delta: &ObservationDelta) -> Vec<Directive> {
        let Some(mut obs) = self.cached_observation.take() else {
            return Vec::new();
        };
        apply_delta_to_observation(&mut obs, delta);
        let directives = self.decide(&obs);
        self.cached_observation = Some(obs);
        directives
    }

    fn reset(&mut self) {
        self.pending_settlement = None;
        self.cached_observation = None;
    }
}

// ---------------------------------------------------------------------------
// StrikerAgent: economy-first, scout, rally, then decisive general kill
// ---------------------------------------------------------------------------

const STRIKER_RALLY_DISTANCE: i32 = 6;
const STRIKER_MIN_RALLY_SIZE: usize = 4;
const STRIKER_EXPAND_THRESHOLD: usize = 15;
const STRIKER_RETREAT_THRESHOLD: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
enum StrikerMode {
    Expand,
    Scout,
    Rally,
    Strike,
}

pub struct StrikerAgent {
    mode: StrikerMode,
    last_known_enemy_general: Option<Axial>,
    pending_settlement: Option<Axial>,
    rally_point: Option<Axial>,
    cached_observation: Option<Observation>,
}

impl StrikerAgent {
    pub fn new() -> Self {
        Self {
            mode: StrikerMode::Expand,
            last_known_enemy_general: None,
            pending_settlement: None,
            rally_point: None,
            cached_observation: None,
        }
    }

    fn strike_target(&self, obs: &Observation) -> Option<Axial> {
        if let Some(eg) = obs.visible_enemies.iter().find(|e| e.is_general) {
            return Some(Axial::new(eg.q, eg.r));
        }
        if let Some(lk) = self.last_known_enemy_general {
            return Some(lk);
        }
        enemy_direction(obs)
    }

    fn compute_rally_point(&self, obs: &Observation, target: Axial) -> Option<Axial> {
        let own_gen = obs.own_units.iter().find(|u| u.is_general)?;
        let gen_pos = Axial::new(own_gen.q, own_gen.r);
        let (gr, gc) = hex::axial_to_offset(gen_pos);
        let (tr, tc) = hex::axial_to_offset(target);
        // Rally point: RALLY_DISTANCE hexes from target, toward own general.
        let dist = hex::distance(gen_pos, target) as f32;
        if dist < 1.0 {
            return Some(gen_pos);
        }
        let ratio = STRIKER_RALLY_DISTANCE as f32 / dist;
        let rr = tr + ((gr - tr) as f32 * ratio) as i32;
        let rc = tc + ((gc - tc) as f32 * ratio) as i32;
        Some(hex::offset_to_axial(
            rr.clamp(1, obs.height as i32 - 2),
            rc.clamp(1, obs.width as i32 - 2),
        ))
    }

    fn count_units_near(&self, obs: &Observation, point: Axial, radius: i32) -> usize {
        obs.own_units
            .iter()
            .filter(|u| !u.is_general && u.engagements.is_empty())
            .filter(|u| hex::distance(Axial::new(u.q, u.r), point) <= radius)
            .count()
    }

    fn decide(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();
        let general = obs.own_units.iter().find(|u| u.is_general);
        let general_hex = general.map(|u| Axial::new(u.q, u.r));
        let settlements = settlement_hexes(obs);

        // Update intel.
        if let Some(eg) = obs.visible_enemies.iter().find(|e| e.is_general) {
            self.last_known_enemy_general = Some(Axial::new(eg.q, eg.r));
        }

        let non_general_count = obs.own_units.iter().filter(|u| !u.is_general).count();
        let enemy_general_known = self.last_known_enemy_general.is_some();
        let strike_target = self.strike_target(obs);

        // Mode transitions.
        match self.mode {
            StrikerMode::Expand => {
                if non_general_count >= STRIKER_EXPAND_THRESHOLD {
                    if enemy_general_known {
                        // We know where they are — rally then strike.
                        self.mode = StrikerMode::Rally;
                        self.rally_point = strike_target
                            .and_then(|t| self.compute_rally_point(obs, t));
                    } else {
                        // Need to find enemy general first.
                        self.mode = StrikerMode::Scout;
                    }
                }
            }
            StrikerMode::Scout => {
                if enemy_general_known {
                    self.mode = StrikerMode::Rally;
                    self.rally_point = strike_target
                        .and_then(|t| self.compute_rally_point(obs, t));
                }
                if non_general_count < STRIKER_RETREAT_THRESHOLD {
                    self.mode = StrikerMode::Expand;
                }
            }
            StrikerMode::Rally => {
                // Check if enough units gathered at rally point.
                if let Some(rp) = self.rally_point {
                    let gathered = self.count_units_near(obs, rp, 3);
                    if gathered >= STRIKER_MIN_RALLY_SIZE {
                        self.mode = StrikerMode::Strike;
                    }
                }
                // Lost too many units, retreat.
                if non_general_count < STRIKER_RETREAT_THRESHOLD {
                    self.mode = StrikerMode::Expand;
                    self.rally_point = None;
                }
            }
            StrikerMode::Strike => {
                // Adaptive: if strike force is getting destroyed, pull back.
                if non_general_count < STRIKER_RETREAT_THRESHOLD {
                    self.mode = StrikerMode::Expand;
                    self.rally_point = None;
                }
                // Update rally point to follow moving target.
                if let Some(t) = strike_target {
                    self.rally_point = self.compute_rally_point(obs, t);
                }
            }
        }

        // Pending settlement convoy dispatch.
        if let Some(target) = self.pending_settlement {
            if settlement_hexes(obs).contains(&target) {
                self.pending_settlement = None;
            } else if let Some(convoy) = obs
                .own_convoys
                .iter()
                .find(|c| c.cargo_type == CargoType::Settlers)
            {
                directives.push(Directive::SendConvoy {
                    convoy_id: convoy.id,
                    dest_q: target.q,
                    dest_r: target.r,
                });
            }
        }

        // Economy (shared with SpreadAgent).
        if let Some(hex) = general_hex {
            manage_settlement_infrastructure(obs, hex, &mut directives);
            manage_settlement_population(obs, hex, &mut directives);
            produce_units_at_settlement(obs, hex, &mut directives);
            try_send_settlers(obs, hex, &settlements, &mut self.pending_settlement, &mut directives);
            load_surplus_convoys(obs, general_hex, &mut directives);

            for &settlement in &settlements {
                if Some(settlement) == general_hex {
                    continue;
                }
                manage_settlement_infrastructure(obs, settlement, &mut directives);
                manage_settlement_population(obs, settlement, &mut directives);
                produce_units_at_settlement(obs, settlement, &mut directives);
            }
        }

        // Unit movement.
        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();
        let friendly_near_enemy: HashMap<UnitKey, usize> = count_friendlies_near_enemies(obs);
        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let enemy_target = enemy_direction(obs);

        let guard_ids = if self.mode != StrikerMode::Expand {
            if let Some(g) = general {
                assign_guards(obs, g, 2)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        for (idx, unit) in obs.own_units.iter().enumerate() {
            if !unit.engagements.is_empty() {
                continue;
            }

            // General: flee from threats, otherwise stay put in combat modes.
            if unit.is_general {
                let gen_pos = Axial::new(unit.q, unit.r);
                if self.mode != StrikerMode::Expand {
                    let threat = obs
                        .visible_enemies
                        .iter()
                        .filter(|e| hex::distance(gen_pos, Axial::new(e.q, e.r)) <= 3)
                        .min_by_key(|e| hex::distance(gen_pos, Axial::new(e.q, e.r)));
                    if let Some(enemy) = threat {
                        let ep = Axial::new(enemy.q, enemy.r);
                        let (gr, gc) = hex::axial_to_offset(gen_pos);
                        let (er, ec) = hex::axial_to_offset(ep);
                        let flee_r = (gr + (gr - er)).clamp(1, obs.height as i32 - 2);
                        let flee_c = (gc + (gc - ec)).clamp(1, obs.width as i32 - 2);
                        directives.push(Directive::Move {
                            unit_id: unit.id,
                            q: hex::offset_to_axial(flee_r, flee_c).q,
                            r: hex::offset_to_axial(flee_r, flee_c).r,
                        });
                    }
                } else if obs.own_units.len() > 10 && hex::distance(gen_pos, map_center) > 5 {
                    directives.push(Directive::Move {
                        unit_id: unit.id,
                        q: map_center.q,
                        r: map_center.r,
                    });
                }
                continue;
            }

            // Engage adjacent enemies.
            if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos, &friendly_near_enemy) {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }

            // Movement by mode and role.
            let dest = match self.mode {
                StrikerMode::Expand => {
                    if obs.own_units.len() <= 8 {
                        pick_sector_destination(unit, idx, obs, map_center)
                    } else {
                        pick_lane_destination(unit, obs, enemy_target)
                    }
                }
                StrikerMode::Scout => {
                    if guard_ids.contains(&unit.id) {
                        general_hex
                    } else {
                        // Send one scout toward inferred enemy direction,
                        // rest continue expanding.
                        let is_scout = unit.id.data().as_ffi() % 7 == 0;
                        if is_scout {
                            enemy_direction(obs)
                        } else {
                            pick_lane_destination(unit, obs, enemy_target)
                        }
                    }
                }
                StrikerMode::Rally => {
                    if guard_ids.contains(&unit.id) {
                        general_hex
                    } else {
                        // Everyone rallies to the rally point.
                        self.rally_point
                    }
                }
                StrikerMode::Strike => {
                    if guard_ids.contains(&unit.id) {
                        general_hex
                    } else {
                        // All non-guard units go for the kill.
                        strike_target
                    }
                }
            };

            if let Some(d) = dest {
                directives.push(Directive::Move {
                    unit_id: unit.id,
                    q: d.q,
                    r: d.r,
                });
                if let Some(idx) = cell_index(obs, Axial::new(unit.q, unit.r)) {
                    if obs.stockpile_owner[idx] == Some(obs.player) && obs.road_levels[idx] == 0 {
                        directives.push(Directive::BuildRoad {
                            hex_q: unit.q,
                            hex_r: unit.r,
                            level: 1,
                        });
                    }
                }
            }
        }

        directives
    }
}

impl Agent for StrikerAgent {
    fn name(&self) -> &str {
        "striker"
    }

    fn init(&mut self, obs: &InitialObservation) {
        self.cached_observation = Some(seed_observation(obs));
    }

    fn act(&mut self, delta: &ObservationDelta) -> Vec<Directive> {
        let Some(mut obs) = self.cached_observation.take() else {
            return Vec::new();
        };
        apply_delta_to_observation(&mut obs, delta);
        let directives = self.decide(&obs);
        self.cached_observation = Some(obs);
        directives
    }

    fn mode(&self) -> Option<&str> {
        Some(match self.mode {
            StrikerMode::Expand => "expand",
            StrikerMode::Scout => "scout",
            StrikerMode::Rally => "rally",
            StrikerMode::Strike => "strike",
        })
    }

    fn reset(&mut self) {
        self.mode = StrikerMode::Expand;
        self.last_known_enemy_general = None;
        self.pending_settlement = None;
        self.rally_point = None;
        self.cached_observation = None;
    }
}

// ---------------------------------------------------------------------------
// TurtleAgent: maximize settlements + population, overwhelm late game
// ---------------------------------------------------------------------------

pub struct TurtleAgent {
    pending_settlement: Option<Axial>,
    cached_observation: Option<Observation>,
}

impl TurtleAgent {
    pub fn new() -> Self {
        Self {
            pending_settlement: None,
            cached_observation: None,
        }
    }

    fn decide(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();
        let general = obs.own_units.iter().find(|u| u.is_general);
        let general_hex = general.map(|u| Axial::new(u.q, u.r));
        let settlements = settlement_hexes(obs);

        if let Some(target) = self.pending_settlement {
            if settlement_hexes(obs).contains(&target) {
                self.pending_settlement = None;
            } else if let Some(convoy) = obs
                .own_convoys
                .iter()
                .find(|c| c.cargo_type == CargoType::Settlers)
            {
                directives.push(Directive::SendConvoy {
                    convoy_id: convoy.id,
                    dest_q: target.q,
                    dest_r: target.r,
                });
            }
        }

        if let Some(hex) = general_hex {
            manage_settlement_infrastructure(obs, hex, &mut directives);
            manage_turtle_population(obs, hex, &mut directives);
            produce_units_at_settlement(obs, hex, &mut directives);

            let general_population = total_population_on_hex(obs, hex);
            if self.pending_settlement.is_none()
                && !obs
                    .own_convoys
                    .iter()
                    .any(|c| c.cargo_type == CargoType::Settlers)
                && general_population >= SETTLEMENT_THRESHOLD + SETTLER_CONVOY_SIZE
            {
                if let Some(target) = pick_settlement_target(obs, &settlements, hex) {
                    directives.push(Directive::LoadConvoy {
                        hex_q: hex.q,
                        hex_r: hex.r,
                        cargo_type: CargoType::Settlers,
                        amount: SETTLER_CONVOY_SIZE as f32,
                    });
                    self.pending_settlement = Some(target);
                }
            }

            load_surplus_convoys(obs, general_hex, &mut directives);

            for &settlement in &settlements {
                if Some(settlement) == general_hex {
                    continue;
                }
                manage_settlement_infrastructure(obs, settlement, &mut directives);
                manage_turtle_population(obs, settlement, &mut directives);
                produce_units_at_settlement(obs, settlement, &mut directives);
            }
        }

        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();
        let friendly_near_enemy: HashMap<UnitKey, usize> = count_friendlies_near_enemies(obs);
        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let enemy_target = enemy_direction(obs);

        for (idx, unit) in obs.own_units.iter().enumerate() {
            if !unit.engagements.is_empty() {
                continue;
            }

            if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos, &friendly_near_enemy) {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }

            if unit.is_general {
                let gen_pos = Axial::new(unit.q, unit.r);
                let threat = obs
                    .visible_enemies
                    .iter()
                    .filter(|e| hex::distance(gen_pos, Axial::new(e.q, e.r)) <= 4)
                    .min_by_key(|e| hex::distance(gen_pos, Axial::new(e.q, e.r)));
                if let Some(enemy) = threat {
                    let enemy_pos = Axial::new(enemy.q, enemy.r);
                    let (gr, gc) = hex::axial_to_offset(gen_pos);
                    let (er, ec) = hex::axial_to_offset(enemy_pos);
                    let flee_r = (gr + (gr - er)).clamp(1, obs.height as i32 - 2);
                    let flee_c = (gc + (gc - ec)).clamp(1, obs.width as i32 - 2);
                    directives.push(Directive::Move {
                        unit_id: unit.id,
                        q: hex::offset_to_axial(flee_r, flee_c).q,
                        r: hex::offset_to_axial(flee_r, flee_c).r,
                    });
                }
                continue;
            }

            let dest = if obs.own_units.len() <= 8 {
                pick_sector_destination(unit, idx, obs, map_center)
            } else if obs.own_units.len() >= 20 {
                pick_lane_destination(unit, obs, enemy_target)
            } else {
                let unit_pos = Axial::new(unit.q, unit.r);
                let nearby_threat = obs
                    .visible_enemies
                    .iter()
                    .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 6)
                    .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)));
                if let Some(enemy) = nearby_threat {
                    Some(Axial::new(enemy.q, enemy.r))
                } else {
                    let best_settlement = settlements
                        .iter()
                        .min_by_key(|s| hex::distance(unit_pos, **s));
                    best_settlement.map(|s| {
                        let angle = (unit.id.data().as_ffi() as f32 * 1.3) % std::f32::consts::TAU;
                        let (sr, sc) = hex::axial_to_offset(*s);
                        let pr = sr + (angle.sin() * 3.0) as i32;
                        let pc = sc + (angle.cos() * 3.0) as i32;
                        hex::offset_to_axial(
                            pr.clamp(1, obs.height as i32 - 2),
                            pc.clamp(1, obs.width as i32 - 2),
                        )
                    })
                }
            };

            if let Some(d) = dest {
                directives.push(Directive::Move {
                    unit_id: unit.id,
                    q: d.q,
                    r: d.r,
                });
                if let Some(idx) = cell_index(obs, Axial::new(unit.q, unit.r)) {
                    if obs.stockpile_owner[idx] == Some(obs.player) && obs.road_levels[idx] == 0 {
                        directives.push(Directive::BuildRoad {
                            hex_q: unit.q,
                            hex_r: unit.r,
                            level: 1,
                        });
                    }
                }
            }
        }

        directives
    }
}

impl Agent for TurtleAgent {
    fn name(&self) -> &str {
        "turtle"
    }

    fn init(&mut self, obs: &InitialObservation) {
        self.cached_observation = Some(seed_observation(obs));
    }

    fn act(&mut self, delta: &ObservationDelta) -> Vec<Directive> {
        let Some(mut obs) = self.cached_observation.take() else {
            return Vec::new();
        };
        apply_delta_to_observation(&mut obs, delta);
        let directives = self.decide(&obs);
        self.cached_observation = Some(obs);
        directives
    }

    fn reset(&mut self) {
        self.pending_settlement = None;
        self.cached_observation = None;
    }
}

/// Turtle population management: 60% farmers, 15% workers for maximum growth.
fn manage_turtle_population(obs: &Observation, hex: Axial, directives: &mut Vec<Directive>) {
    let (idle, farmers, workers, _trained, _untrained) = population_mix(obs, hex);
    let total = idle + farmers + workers + _trained + _untrained;
    let target_farmers = (total as f32 * 0.60).ceil() as u16;
    let target_workers = (total as f32 * 0.15).ceil() as u16;

    if farmers < target_farmers && idle > 0 {
        directives.push(Directive::AssignRole {
            hex_q: hex.q,
            hex_r: hex.r,
            role: Role::Farmer,
            count: (target_farmers - farmers).min(5),
        });
    } else if workers < target_workers && idle > 0 {
        directives.push(Directive::AssignRole {
            hex_q: hex.q,
            hex_r: hex.r,
            role: Role::Worker,
            count: (target_workers - workers).min(2),
        });
    } else if idle > 0 {
        directives.push(Directive::TrainSoldier {
            hex_q: hex.q,
            hex_r: hex.r,
        });
    }
}

fn assign_guards(obs: &Observation, general: &UnitInfo, count: usize) -> Vec<UnitKey> {
    let gen_pos = Axial::new(general.q, general.r);
    let mut candidates: Vec<&UnitInfo> = obs
        .own_units
        .iter()
        .filter(|u| !u.is_general && u.engagements.is_empty())
        .collect();
    candidates.sort_by_key(|u| hex::distance(Axial::new(u.q, u.r), gen_pos));
    candidates.iter().take(count).map(|u| u.id).collect()
}

// ---------------------------------------------------------------------------
// Shared utility functions
// ---------------------------------------------------------------------------

fn cell_index(obs: &Observation, ax: Axial) -> Option<usize> {
    let (row, col) = hex::axial_to_offset(ax);
    if row < 0 || col < 0 {
        return None;
    }
    let (row, col) = (row as usize, col as usize);
    if row < obs.height && col < obs.width {
        Some(row * obs.width + col)
    } else {
        None
    }
}

fn index_to_hex(obs: &Observation, idx: usize) -> Axial {
    let row = idx / obs.width;
    let col = idx % obs.width;
    hex::offset_to_axial(row as i32, col as i32)
}

// ---------------------------------------------------------------------------
// Shared economy helpers (used by SpreadAgent and StrikerAgent)
// ---------------------------------------------------------------------------

fn manage_settlement_population(obs: &Observation, hex: Axial, directives: &mut Vec<Directive>) {
    let (idle, farmers, workers, _trained, _untrained) = population_mix(obs, hex);
    let total = idle + farmers + workers + _trained + _untrained;
    let target_farmers = (total as f32 * 0.45).ceil() as u16;
    let target_workers = (total as f32 * 0.2).ceil() as u16;

    if farmers < target_farmers && idle > 0 {
        directives.push(Directive::AssignRole {
            hex_q: hex.q,
            hex_r: hex.r,
            role: Role::Farmer,
            count: (target_farmers - farmers).min(3),
        });
    } else if workers < target_workers && idle > 0 {
        directives.push(Directive::AssignRole {
            hex_q: hex.q,
            hex_r: hex.r,
            role: Role::Worker,
            count: (target_workers - workers).min(2),
        });
    } else if idle > 0 {
        directives.push(Directive::TrainSoldier {
            hex_q: hex.q,
            hex_r: hex.r,
        });
    }
}

fn manage_settlement_infrastructure(obs: &Observation, hex: Axial, directives: &mut Vec<Directive>) {
    if let Some(idx) = cell_index(obs, hex) {
        if obs.material_stockpiles[idx] >= 20.0 {
            directives.push(Directive::BuildDepot {
                hex_q: hex.q,
                hex_r: hex.r,
            });
        }
        if obs.road_levels[idx] == 0 {
            directives.push(Directive::BuildRoad {
                hex_q: hex.q,
                hex_r: hex.r,
                level: 1,
            });
        }
    }
}

fn produce_units_at_settlement(obs: &Observation, hex: Axial, directives: &mut Vec<Directive>) {
    let (_idle, _farmers, _workers, trained_soldiers, _untrained) = population_mix(obs, hex);
    if let Some(idx) = cell_index(obs, hex) {
        let mut remaining_food = obs.food_stockpiles[idx];
        let mut remaining_material = obs.material_stockpiles[idx];
        let mut ready = trained_soldiers;
        while remaining_food >= UNIT_FOOD_COST
            && remaining_material >= UNIT_MATERIAL_COST
            && ready >= SOLDIERS_PER_UNIT
        {
            directives.push(Directive::Produce { hex_q: hex.q, hex_r: hex.r });
            remaining_food -= UNIT_FOOD_COST;
            remaining_material -= UNIT_MATERIAL_COST;
            ready -= SOLDIERS_PER_UNIT;
        }
    }
}

fn try_send_settlers(
    obs: &Observation,
    general_hex: Axial,
    settlements: &[Axial],
    pending: &mut Option<Axial>,
    directives: &mut Vec<Directive>,
) {
    let general_population = total_population_on_hex(obs, general_hex);
    if pending.is_none()
        && !obs
            .own_convoys
            .iter()
            .any(|c| c.cargo_type == CargoType::Settlers)
        && general_population >= SETTLEMENT_THRESHOLD + SETTLER_CONVOY_SIZE + 5
    {
        if let Some(target) = pick_settlement_target(obs, settlements, general_hex) {
            directives.push(Directive::LoadConvoy {
                hex_q: general_hex.q,
                hex_r: general_hex.r,
                cargo_type: CargoType::Settlers,
                amount: SETTLER_CONVOY_SIZE as f32,
            });
            *pending = Some(target);
        }
    }
}

fn load_surplus_convoys(obs: &Observation, general_hex: Option<Axial>, directives: &mut Vec<Directive>) {
    for (idx, owner) in obs.stockpile_owner.iter().enumerate() {
        if *owner != Some(obs.player) {
            continue;
        }
        let hex = index_to_hex(obs, idx);
        if Some(hex) == general_hex {
            continue;
        }
        if obs.food_stockpiles[idx] > 15.0 {
            directives.push(Directive::LoadConvoy {
                hex_q: hex.q,
                hex_r: hex.r,
                cargo_type: CargoType::Food,
                amount: 10.0,
            });
        } else if obs.material_stockpiles[idx] > 10.0 {
            directives.push(Directive::LoadConvoy {
                hex_q: hex.q,
                hex_r: hex.r,
                cargo_type: CargoType::Material,
                amount: 10.0,
            });
        }
    }
}

fn population_mix(obs: &Observation, hex: Axial) -> (u16, u16, u16, u16, u16) {
    let mut idle = 0;
    let mut farmers = 0;
    let mut workers = 0;
    let mut trained_soldiers = 0;
    let mut untrained_soldiers = 0;
    for pop in obs
        .own_population
        .iter()
        .filter(|p| p.q == hex.q && p.r == hex.r)
    {
        match pop.role {
            Role::Idle => idle += pop.count,
            Role::Farmer => farmers += pop.count,
            Role::Worker => workers += pop.count,
            Role::Soldier => {
                if pop.training >= 1.0 {
                    trained_soldiers += pop.count;
                } else {
                    untrained_soldiers += pop.count;
                }
            }
        }
    }
    (idle, farmers, workers, trained_soldiers, untrained_soldiers)
}

fn total_population_on_hex(obs: &Observation, hex: Axial) -> u16 {
    obs.own_population
        .iter()
        .filter(|p| p.q == hex.q && p.r == hex.r)
        .map(|p| p.count)
        .sum()
}

fn settlement_hexes(obs: &Observation) -> Vec<Axial> {
    let mut settlements = Vec::new();
    for pop in &obs.own_population {
        let hex = Axial::new(pop.q, pop.r);
        if total_population_on_hex(obs, hex) >= SETTLEMENT_THRESHOLD && !settlements.contains(&hex) {
            settlements.push(hex);
        }
    }
    settlements
}

fn pick_settlement_target(obs: &Observation, settlements: &[Axial], origin: Axial) -> Option<Axial> {
    let mut best: Option<(Axial, f32)> = None;
    for (idx, owner) in obs.stockpile_owner.iter().enumerate() {
        if *owner != Some(obs.player) {
            continue;
        }
        let hex = index_to_hex(obs, idx);
        if settlements.contains(&hex) {
            continue;
        }
        if total_population_on_hex(obs, hex) > 0 {
            continue;
        }
        let support_distance = settlements
            .iter()
            .map(|s| hex::distance(*s, hex))
            .min()
            .unwrap_or(i32::MAX);
        if support_distance <= SETTLEMENT_SUPPORT_RADIUS {
            continue;
        }
        let distance_from_origin = hex::distance(origin, hex);
        if distance_from_origin < 2 || distance_from_origin > 8 {
            continue;
        }
        let fertility = obs.terrain[idx];
        if fertility <= 0.0 {
            continue;
        }
        let score = fertility * 2.0 + obs.material_map[idx] - distance_from_origin as f32 * 0.25;
        match best {
            Some((_, best_score)) if best_score >= score => {}
            _ => best = Some((hex, score)),
        }
    }
    best.map(|(hex, _)| hex)
}

fn count_friendlies_near_enemies(obs: &Observation) -> HashMap<UnitKey, usize> {
    let mut counts: HashMap<UnitKey, usize> = HashMap::new();
    for enemy in &obs.visible_enemies {
        let enemy_pos = Axial::new(enemy.q, enemy.r);
        let count = obs
            .own_units
            .iter()
            .filter(|u| !u.is_general && u.engagements.is_empty())
            .filter(|u| hex::distance(Axial::new(u.q, u.r), enemy_pos) <= 1)
            .count();
        counts.insert(enemy.id, count);
    }
    counts
}

fn find_engageable_enemy(
    unit: &UnitInfo,
    enemy_by_pos: &HashMap<(i32, i32), &UnitInfo>,
    friendly_counts: &HashMap<UnitKey, usize>,
) -> Option<UnitKey> {
    let unit_pos = Axial::new(unit.q, unit.r);
    hex::neighbors(unit_pos)
        .iter()
        .filter_map(|nb| enemy_by_pos.get(&(nb.q, nb.r)).copied())
        .filter(|e| {
            let friends = friendly_counts.get(&e.id).copied().unwrap_or(0);
            friends >= 2 || unit.strength >= e.strength * 0.5
        })
        .min_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
        .map(|e| e.id)
}

fn pick_sector_destination(
    unit: &UnitInfo,
    idx: usize,
    obs: &Observation,
    map_center: Axial,
) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);
    let nearby_enemy = obs
        .visible_enemies
        .iter()
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 8)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)));

    if let Some(enemy) = nearby_enemy {
        return Some(Axial::new(enemy.q, enemy.r));
    }

    let non_general_count = obs
        .own_units
        .iter()
        .filter(|u| !u.is_general)
        .count()
        .max(1);
    let non_gen_idx = obs
        .own_units
        .iter()
        .filter(|u| !u.is_general)
        .position(|u| u.id == unit.id)
        .unwrap_or(idx);

    let angle = (non_gen_idx as f32 / non_general_count as f32) * std::f32::consts::TAU;
    let spread_radius = (obs.width.min(obs.height) / 4) as f32;
    let target_row = (map_center.r as f32 + angle.sin() * spread_radius) as i32;
    let target_col = {
        let (_cr, cc) = hex::axial_to_offset(map_center);
        (cc as f32 + angle.cos() * spread_radius) as i32
    };
    Some(hex::offset_to_axial(
        target_row.clamp(1, obs.height as i32 - 2),
        target_col.clamp(1, obs.width as i32 - 2),
    ))
}

fn pick_lane_destination(
    unit: &UnitInfo,
    obs: &Observation,
    enemy_target: Option<Axial>,
) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);
    if let Some(enemy) = obs
        .visible_enemies
        .iter()
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 10)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)))
    {
        return Some(Axial::new(enemy.q, enemy.r));
    }
    let target = enemy_target?;
    let (target_r, target_c) = hex::axial_to_offset(target);
    let (unit_r, unit_c) = hex::axial_to_offset(unit_pos);
    let dx = target_c - unit_c;
    let dy = target_r - unit_r;
    let lane_hash = (((unit.id.data().as_ffi() as i32) * 7 + 13) % 5) - 2;
    let perp_offset = lane_hash * 3;
    let dest_r = target_r
        + if dx != 0 {
            perp_offset * dx.signum()
        } else {
            0
        };
    let dest_c = target_c
        + if dy != 0 {
            perp_offset * dy.signum()
        } else {
            0
        };
    Some(hex::offset_to_axial(
        dest_r.clamp(1, obs.height as i32 - 2),
        dest_c.clamp(1, obs.width as i32 - 2),
    ))
}

fn enemy_direction(obs: &Observation) -> Option<Axial> {
    if let Some(enemy_gen) = obs.visible_enemies.iter().find(|e| e.is_general) {
        return Some(Axial::new(enemy_gen.q, enemy_gen.r));
    }
    if !obs.visible_enemies.is_empty() {
        let sum_q: i32 = obs.visible_enemies.iter().map(|e| e.q).sum();
        let sum_r: i32 = obs.visible_enemies.iter().map(|e| e.r).sum();
        let n = obs.visible_enemies.len() as i32;
        return Some(Axial::new(sum_q / n, sum_r / n));
    }
    let own_gen = obs.own_units.iter().find(|u| u.is_general)?;
    let (gen_row, gen_col) = hex::axial_to_offset(Axial::new(own_gen.q, own_gen.r));
    Some(hex::offset_to_axial(
        ((obs.height as i32 - 1) - gen_row).clamp(0, obs.height as i32 - 1),
        ((obs.width as i32 - 1) - gen_col).clamp(0, obs.width as i32 - 1),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::mapgen::{generate, MapConfig};
    use crate::v2::observation::{initial_observation, observe_delta, ObservationSession};

    #[test]
    fn spread_agent_manages_population() {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        });
        let mut agent = SpreadAgent::new();
        let init = initial_observation(&state, 0);
        agent.init(&init);
        let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
        let delta = observe_delta(&mut state, 0, &mut session);
        let directives = agent.act(&delta);
        assert!(directives.iter().any(|d| {
            matches!(
                d,
                Directive::AssignRole { .. }
                    | Directive::TrainSoldier { .. }
                    | Directive::BuildDepot { .. }
            )
        }));
    }

    #[test]
    fn spread_agent_moves_idle_units() {
        let mut state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });
        let mut agent = SpreadAgent::new();
        let init = initial_observation(&state, 0);
        agent.init(&init);
        let mut session = ObservationSession::new(state.players.len(), state.width * state.height);
        let delta = observe_delta(&mut state, 0, &mut session);
        let directives = agent.act(&delta);
        assert!(
            directives
                .iter()
                .any(|d| matches!(d, Directive::Move { .. }))
        );
    }
}
