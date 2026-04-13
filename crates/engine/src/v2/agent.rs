use slotmap::Key;
use std::collections::HashMap;

use super::SETTLEMENT_THRESHOLD;
use super::directive::Directive;
use super::hex::{self, Axial};
use super::observation::{
    InitialObservation, NewScoutedHex, Observation, ObservationDelta, UnitInfo,
    apply_delta_to_observation, materialize_observation,
};
use super::state::UnitKey;

pub trait Agent: Send {
    fn name(&self) -> &str;
    fn init(&mut self, obs: &InitialObservation);
    fn act(&mut self, delta: &ObservationDelta) -> Vec<Directive>;
    fn reset(&mut self) {}
    fn mode(&self) -> Option<&str> {
        None
    }
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
    cached_observation: Option<Observation>,
}

impl SpreadAgent {
    pub fn new() -> Self {
        Self {
            cached_observation: None,
        }
    }

    fn decide(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();

        // Economy (population roles, infrastructure, production, settler dispatch)
        // is now managed by the city AI. Agents issue only military/movement directives.

        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();
        let friendly_near_enemy: HashMap<UnitKey, usize> = count_friendlies_near_enemies(obs);

        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let origin = settlement_hexes(obs).first().copied().unwrap_or(map_center);
        let enemy_target = enemy_direction(obs);

        for (idx, unit) in obs.own_units.iter().enumerate() {
            // Handle engaged units: disengage if losing.
            if !unit.engagements.is_empty() {
                if should_disengage(unit, &obs.visible_enemies) {
                    directives.push(Directive::DisengageAll { unit_id: unit.id });
                }
                continue;
            }
            if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos, &friendly_near_enemy) {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }

            let dest = if obs.own_units.len() <= 8 {
                pick_sector_destination(unit, idx, obs, origin)
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
        self.cached_observation = None;
    }
}

// ---------------------------------------------------------------------------
// StrikerAgent: military-focused, scout, rally, then decisive general kill
// ---------------------------------------------------------------------------

const STRIKER_RALLY_DISTANCE: i32 = 6;
const STRIKER_MIN_RALLY_SIZE: usize = 4;
const STRIKER_EXPAND_THRESHOLD: usize = 10;
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
    rally_point: Option<Axial>,
    cached_observation: Option<Observation>,
}

impl StrikerAgent {
    pub fn new() -> Self {
        Self {
            mode: StrikerMode::Expand,
            rally_point: None,
            cached_observation: None,
        }
    }

    fn strike_target(&self, obs: &Observation) -> Option<Axial> {
        // Target nearest visible enemy unit, or fall back to inferred enemy direction.
        if let Some(e) = obs.visible_enemies.first() {
            return Some(Axial::new(e.q, e.r));
        }
        enemy_direction(obs)
    }

    fn compute_rally_point(&self, obs: &Observation, target: Axial) -> Option<Axial> {
        // Rally toward target from centroid of own units.
        let own_units: Vec<_> = obs.own_units.iter().collect();
        if own_units.is_empty() {
            return None;
        }
        let sum_q: i32 = own_units.iter().map(|u| u.q).sum();
        let sum_r: i32 = own_units.iter().map(|u| u.r).sum();
        let n = own_units.len() as i32;
        let centroid = Axial::new(sum_q / n, sum_r / n);
        let (cr, cc) = hex::axial_to_offset(centroid);
        let (tr, tc) = hex::axial_to_offset(target);
        let dist = hex::distance(centroid, target) as f32;
        if dist < 1.0 {
            return Some(centroid);
        }
        let ratio = STRIKER_RALLY_DISTANCE as f32 / dist;
        let rr = tr + ((cr - tr) as f32 * ratio) as i32;
        let rc = tc + ((cc - tc) as f32 * ratio) as i32;
        Some(hex::offset_to_axial(
            rr.clamp(1, obs.height as i32 - 2),
            rc.clamp(1, obs.width as i32 - 2),
        ))
    }

    fn count_units_near(&self, obs: &Observation, point: Axial, radius: i32) -> usize {
        obs.own_units
            .iter()
            .filter(|u| u.engagements.is_empty())
            .filter(|u| hex::distance(Axial::new(u.q, u.r), point) <= radius)
            .count()
    }

    fn decide(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();

        let unit_count = obs.own_units.len();
        let enemy_visible = !obs.visible_enemies.is_empty();
        let strike_target = self.strike_target(obs);

        // Mode transitions.
        match self.mode {
            StrikerMode::Expand => {
                if unit_count >= STRIKER_EXPAND_THRESHOLD {
                    if enemy_visible {
                        // Enemy units visible — rally then strike.
                        self.mode = StrikerMode::Rally;
                        self.rally_point =
                            strike_target.and_then(|t| self.compute_rally_point(obs, t));
                    } else {
                        // Scout for enemy presence.
                        self.mode = StrikerMode::Scout;
                    }
                }
            }
            StrikerMode::Scout => {
                if enemy_visible {
                    self.mode = StrikerMode::Rally;
                    self.rally_point = strike_target.and_then(|t| self.compute_rally_point(obs, t));
                }
                if unit_count < STRIKER_RETREAT_THRESHOLD {
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
                if unit_count < STRIKER_RETREAT_THRESHOLD {
                    self.mode = StrikerMode::Expand;
                    self.rally_point = None;
                }
            }
            StrikerMode::Strike => {
                // Adaptive: if strike force is getting destroyed, pull back.
                if unit_count < STRIKER_RETREAT_THRESHOLD {
                    self.mode = StrikerMode::Expand;
                    self.rally_point = None;
                }
                // Update rally point to follow moving target.
                if let Some(t) = strike_target {
                    self.rally_point = self.compute_rally_point(obs, t);
                }
            }
        }

        // Economy is now handled by city AI. Agent only issues military/movement directives.

        // Unit movement.
        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();
        let friendly_near_enemy: HashMap<UnitKey, usize> = count_friendlies_near_enemies(obs);
        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let striker_origin = settlement_hexes(obs).first().copied().unwrap_or(map_center);
        let enemy_target = enemy_direction(obs);

        for (idx, unit) in obs.own_units.iter().enumerate() {
            // Handle engaged units: disengage if losing.
            if !unit.engagements.is_empty() {
                if should_disengage(unit, &obs.visible_enemies) {
                    directives.push(Directive::DisengageAll { unit_id: unit.id });
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

            // Movement by mode.
            let dest = match self.mode {
                StrikerMode::Expand => {
                    if obs.own_units.len() <= 8 {
                        pick_sector_destination(unit, idx, obs, striker_origin)
                    } else {
                        pick_lane_destination(unit, obs, enemy_target)
                    }
                }
                StrikerMode::Scout => {
                    // Send one scout toward inferred enemy direction,
                    // rest continue expanding.
                    let is_scout = unit.id.data().as_ffi() % 7 == 0;
                    if is_scout {
                        enemy_direction(obs)
                    } else {
                        pick_lane_destination(unit, obs, enemy_target)
                    }
                }
                StrikerMode::Rally => {
                    // Everyone rallies to the rally point.
                    self.rally_point
                }
                StrikerMode::Strike => {
                    // All units go for the target.
                    strike_target
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
        self.rally_point = None;
        self.cached_observation = None;
    }
}

// ---------------------------------------------------------------------------
// TurtleAgent: maximize settlements + population, overwhelm late game
// ---------------------------------------------------------------------------

pub struct TurtleAgent {
    cached_observation: Option<Observation>,
}

impl TurtleAgent {
    pub fn new() -> Self {
        Self {
            cached_observation: None,
        }
    }

    fn decide(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();
        let settlements = settlement_hexes(obs);

        // Economy handled by city AI. Turtle focuses on defensive positioning.

        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();
        let friendly_near_enemy: HashMap<UnitKey, usize> = count_friendlies_near_enemies(obs);
        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let turtle_origin = settlement_hexes(obs).first().copied().unwrap_or(map_center);
        let enemy_target = enemy_direction(obs);

        for (idx, unit) in obs.own_units.iter().enumerate() {
            // Handle engaged units: disengage if losing.
            if !unit.engagements.is_empty() {
                if should_disengage(unit, &obs.visible_enemies) {
                    directives.push(Directive::DisengageAll { unit_id: unit.id });
                }
                continue;
            }

            if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos, &friendly_near_enemy) {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }

            let dest = if obs.own_units.len() <= 8 {
                pick_sector_destination(unit, idx, obs, turtle_origin)
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
        self.cached_observation = None;
    }
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

// Economy helpers (manage_settlement_population, manage_settlement_infrastructure,
// produce_units_at_settlement, try_send_settlers, load_surplus_convoys, etc.)
// have been moved to city_ai.rs and run automatically.
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
        if total_population_on_hex(obs, hex) >= SETTLEMENT_THRESHOLD && !settlements.contains(&hex)
        {
            settlements.push(hex);
        }
    }
    settlements
}

fn count_friendlies_near_enemies(obs: &Observation) -> HashMap<UnitKey, usize> {
    let mut counts: HashMap<UnitKey, usize> = HashMap::new();
    for enemy in &obs.visible_enemies {
        let enemy_pos = Axial::new(enemy.q, enemy.r);
        let count = obs
            .own_units
            .iter()
            .filter(|u| u.engagements.is_empty())
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
            // Require 2+ friendlies nearby, or significant strength advantage solo.
            // Solo engagements against stronger/equal enemies are wasteful attrition.
            friends >= 2 || unit.strength >= e.strength * 0.8
        })
        .min_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
        .map(|e| e.id)
}

/// Check if an engaged unit should disengage. Returns true when the unit is
/// losing badly enough that staying engaged is worse than the disengage penalty.
fn should_disengage(unit: &UnitInfo, visible_enemies: &[UnitInfo]) -> bool {
    if unit.engagements.is_empty() {
        return false;
    }
    // Look up total enemy strength we're fighting.
    let total_enemy_strength: f32 = unit
        .engagements
        .iter()
        .filter_map(|eng| visible_enemies.iter().find(|e| e.id == eng.enemy_id))
        .map(|e| e.strength)
        .sum();
    // Disengage if enemy total strength is much higher than ours, or we're very weak.
    // The disengage penalty is 30% of current strength, so we should disengage before
    // combat damage exceeds that cost. At DAMAGE_RATE=0.05 per tick with 5-tick poll
    // interval, we'd take ~25% of enemy strength in damage over the next poll window.
    // Disengage if we'd lose more staying than the 30% penalty.
    let projected_damage = total_enemy_strength * 0.05 * 5.0; // 5 ticks until next decision
    let disengage_cost = unit.strength * 0.3;
    unit.strength < 30.0 || projected_damage > disengage_cost
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

    let unit_count = obs.own_units.len().max(1);
    let unit_idx = obs
        .own_units
        .iter()
        .position(|u| u.id == unit.id)
        .unwrap_or(idx);

    let angle = (unit_idx as f32 / unit_count as f32) * std::f32::consts::TAU;
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
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 6)
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
    // Return centroid of visible enemies if any, otherwise infer from own unit centroid.
    if !obs.visible_enemies.is_empty() {
        let sum_q: i32 = obs.visible_enemies.iter().map(|e| e.q).sum();
        let sum_r: i32 = obs.visible_enemies.iter().map(|e| e.r).sum();
        let n = obs.visible_enemies.len() as i32;
        return Some(Axial::new(sum_q / n, sum_r / n));
    }
    // No visible enemies — infer by reflecting own centroid across map center.
    if obs.own_units.is_empty() {
        return None;
    }
    let sum_q: i32 = obs.own_units.iter().map(|u| u.q).sum();
    let sum_r: i32 = obs.own_units.iter().map(|u| u.r).sum();
    let n = obs.own_units.len() as i32;
    let (own_row, own_col) = hex::axial_to_offset(Axial::new(sum_q / n, sum_r / n));
    Some(hex::offset_to_axial(
        ((obs.height as i32 - 1) - own_row).clamp(0, obs.height as i32 - 1),
        ((obs.width as i32 - 1) - own_col).clamp(0, obs.width as i32 - 1),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::mapgen::{MapConfig, generate};
    use crate::v2::observation::{ObservationSession, initial_observation, observe_delta};

    #[test]
    fn spread_agent_issues_military_directives() {
        // Economy is handled by city AI; SpreadAgent only issues Move/Engage/BuildRoad.
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
        // Should not issue economy directives; those are city AI's domain.
        assert!(
            !directives.iter().any(|d| matches!(
                d,
                Directive::AssignRole { .. }
                    | Directive::TrainSoldier { .. }
                    | Directive::LoadConvoy { .. }
            )),
            "SpreadAgent issued economy directives that should come from city AI"
        );
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
