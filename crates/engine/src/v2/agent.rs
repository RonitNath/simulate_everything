use std::collections::HashMap;

use super::directive::Directive;
use super::hex::{self, Axial};
use super::observation::{Observation, UnitInfo};
use super::{UNIT_FOOD_COST, UNIT_MATERIAL_COST};

/// Observe → act interface for V2 game agents.
/// Send bound enables future use across threads.
pub trait Agent: Send {
    fn name(&self) -> &str;
    fn act(&mut self, obs: &Observation) -> Vec<Directive>;
    fn reset(&mut self) {}
}

// ---------------------------------------------------------------------------
// Policy and agent state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Policy {
    /// Push all fronts, engage aggressively
    Aggressive,
    /// Hold ground, build economy, only engage when advantageous
    Defensive,
    /// Concentrate force on weakest enemy axis
    Flanking,
}

#[derive(Debug, Clone)]
struct BackupRequest {
    pos: Axial,
    urgency: f32,
}

/// Three-layer agent: strategic (policy) → tactical (engagement) → operational (routing).
///
/// Steps toward the Centurion architecture (see docs/v2-agent-spec.md) while keeping
/// things simple enough to iterate on.
pub struct SpreadAgent {
    policy: Policy,
    last_strategic_tick: u64,
    backup_requests: Vec<BackupRequest>,
}

impl SpreadAgent {
    pub fn new() -> Self {
        Self {
            policy: Policy::Aggressive,
            last_strategic_tick: 0,
            backup_requests: Vec::new(),
        }
    }
}

impl Default for SpreadAgent {
    fn default() -> Self {
        Self::new()
    }
}

const STRATEGIC_INTERVAL: u64 = 25;

impl Agent for SpreadAgent {
    fn name(&self) -> &str {
        "spread"
    }

    fn reset(&mut self) {
        self.policy = Policy::Aggressive;
        self.last_strategic_tick = 0;
        self.backup_requests.clear();
    }

    fn act(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();
        self.backup_requests.clear();

        // --- Strategic layer (runs periodically) ---
        if obs.tick >= self.last_strategic_tick + STRATEGIC_INTERVAL || obs.tick == 0 {
            self.policy = evaluate_policy(obs);
            self.last_strategic_tick = obs.tick;
            tracing::debug!(
                tick = obs.tick,
                player = obs.player,
                policy = ?self.policy,
                "strategic layer: policy updated"
            );
        }

        // --- Production ---
        let mut remaining_food = obs.food;
        let mut remaining_material = obs.material;
        let mut produce_count = 0u32;
        while remaining_food >= UNIT_FOOD_COST && remaining_material >= UNIT_MATERIAL_COST {
            directives.push(Directive::Produce);
            remaining_food -= UNIT_FOOD_COST;
            remaining_material -= UNIT_MATERIAL_COST;
            produce_count += 1;
        }

        // --- Tactical layer: handle engaged units ---
        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();

        let friendly_near_enemy: HashMap<u32, usize> = count_friendlies_near_enemies(obs);

        for unit in &obs.own_units {
            if unit.engagements.is_empty() {
                continue;
            }
            if let Some(directive) =
                tactical_evaluate_engaged(unit, obs, self.policy, &mut self.backup_requests)
            {
                directives.push(directive);
            }
        }

        // --- Operational layer: route free units ---
        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let enemy_target = enemy_direction(obs);

        self.backup_requests
            .sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap());

        let mut claimed_backups: Vec<bool> = vec![false; self.backup_requests.len()];

        for (idx, unit) in obs.own_units.iter().enumerate() {
            if !unit.engagements.is_empty() {
                continue;
            }
            if unit.is_general {
                if let Some(d) = route_general(unit, obs, map_center) {
                    directives.push(d);
                }
                continue;
            }

            // Try to engage adjacent enemies (with smart filtering)
            if let Some(target) =
                find_engageable_enemy(unit, &enemy_by_pos, &friendly_near_enemy, self.policy)
            {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }

            // Try to respond to backup requests
            if let Some(d) =
                respond_to_backup(unit, &self.backup_requests, &mut claimed_backups)
            {
                directives.push(d);
                continue;
            }

            // Default movement
            let dest = pick_destination(unit, idx, obs, map_center, enemy_target, self.policy);
            if let Some(d) = dest {
                directives.push(Directive::Move {
                    unit_id: unit.id,
                    q: d.q,
                    r: d.r,
                });
            }
        }

        let engaged_count = obs
            .own_units
            .iter()
            .filter(|u| !u.engagements.is_empty())
            .count();
        let move_count = directives
            .iter()
            .filter(|d| matches!(d, Directive::Move { .. }))
            .count();
        let engage_count = directives
            .iter()
            .filter(|d| matches!(d, Directive::Engage { .. }))
            .count();
        let disengage_count = directives
            .iter()
            .filter(|d| matches!(d, Directive::DisengageAll { .. }))
            .count();

        tracing::trace!(
            tick = obs.tick,
            player = obs.player,
            policy = ?self.policy,
            own_units = obs.own_units.len(),
            visible_enemies = obs.visible_enemies.len(),
            produced = produce_count,
            moves = move_count,
            engages = engage_count,
            disengages = disengage_count,
            already_engaged = engaged_count,
            backup_requests = self.backup_requests.len(),
            "spread agent act"
        );

        directives
    }
}

// ---------------------------------------------------------------------------
// Strategic layer
// ---------------------------------------------------------------------------

fn evaluate_policy(obs: &Observation) -> Policy {
    let own_count = obs.own_units.len();
    let enemy_count = obs.visible_enemies.len();

    if enemy_count > 0 && own_count as f32 > enemy_count as f32 * 1.5 {
        return Policy::Aggressive;
    }

    if own_count < 8 || (enemy_count > 0 && own_count <= enemy_count) {
        return Policy::Defensive;
    }

    if enemy_count >= 3 {
        if let Some(c) = enemy_centroid(obs) {
            let spread: f32 = obs
                .visible_enemies
                .iter()
                .map(|e| hex::distance(Axial::new(e.q, e.r), c) as f32)
                .sum::<f32>()
                / enemy_count as f32;
            if spread < 5.0 {
                return Policy::Flanking;
            }
        }
    }

    Policy::Aggressive
}

// ---------------------------------------------------------------------------
// Tactical layer
// ---------------------------------------------------------------------------

fn tactical_evaluate_engaged(
    unit: &UnitInfo,
    obs: &Observation,
    policy: Policy,
    backup_requests: &mut Vec<BackupRequest>,
) -> Option<Directive> {
    let num_engagements = unit.engagements.len();
    let unit_pos = Axial::new(unit.q, unit.r);

    // Can't disengage if surrounded (3+ edges)
    if num_engagements >= 3 {
        backup_requests.push(BackupRequest {
            pos: unit_pos,
            urgency: 1.0,
        });
        return None;
    }

    let total_enemy_strength: f32 = unit
        .engagements
        .iter()
        .filter_map(|eng| {
            obs.visible_enemies
                .iter()
                .find(|e| e.id == eng.enemy_id)
                .map(|e| e.strength)
        })
        .sum();

    let visible_opponents = unit
        .engagements
        .iter()
        .filter(|eng| obs.visible_enemies.iter().any(|e| e.id == eng.enemy_id))
        .count();
    if visible_opponents == 0 {
        return None;
    }

    let strength_ratio = unit.strength / total_enemy_strength.max(0.01);

    let disengage_threshold = match policy {
        Policy::Aggressive => 0.4,
        Policy::Defensive => 0.7,
        Policy::Flanking => 0.6,
    };

    if strength_ratio < 1.5 {
        let urgency = (1.0 - strength_ratio).clamp(0.0, 1.0);
        backup_requests.push(BackupRequest {
            pos: unit_pos,
            urgency,
        });
    }

    if strength_ratio < disengage_threshold {
        tracing::debug!(
            tick = obs.tick,
            player = obs.player,
            unit_id = unit.id,
            strength = unit.strength,
            enemy_strength = total_enemy_strength,
            ratio = strength_ratio,
            policy = ?policy,
            "tactical: disengaging — outmatched"
        );
        return Some(Directive::DisengageAll { unit_id: unit.id });
    }

    // Flanking policy: disengage from even 1v1 to set up 2v1
    if policy == Policy::Flanking && num_engagements == 1 && strength_ratio < 1.2 {
        let nearby_friendlies = obs
            .own_units
            .iter()
            .filter(|u| u.id != unit.id && u.engagements.is_empty() && !u.is_general)
            .filter(|u| hex::distance(Axial::new(u.q, u.r), unit_pos) <= 4)
            .count();
        if nearby_friendlies >= 1 {
            tracing::debug!(
                tick = obs.tick,
                player = obs.player,
                unit_id = unit.id,
                "tactical: disengaging 1v1 to set up flank"
            );
            return Some(Directive::DisengageAll { unit_id: unit.id });
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Operational layer
// ---------------------------------------------------------------------------

fn route_general(unit: &UnitInfo, obs: &Observation, map_center: Axial) -> Option<Directive> {
    if obs.own_units.len() <= 10 {
        return None;
    }

    let gen_pos = Axial::new(unit.q, unit.r);

    let nearest_enemy_dist = obs
        .visible_enemies
        .iter()
        .map(|e| hex::distance(gen_pos, Axial::new(e.q, e.r)))
        .min()
        .unwrap_or(999);

    if nearest_enemy_dist <= 5 {
        let enemy_centroid = enemy_centroid(obs)?;
        let away = Axial::new(
            gen_pos.q + (gen_pos.q - enemy_centroid.q).signum() * 3,
            gen_pos.r + (gen_pos.r - enemy_centroid.r).signum() * 3,
        );
        let (row, col) = hex::axial_to_offset(away);
        let row = row.clamp(2, obs.height as i32 - 3);
        let col = col.clamp(2, obs.width as i32 - 3);
        let safe = hex::offset_to_axial(row, col);
        return Some(Directive::Move {
            unit_id: unit.id,
            q: safe.q,
            r: safe.r,
        });
    }

    if hex::distance(gen_pos, map_center) > 5 {
        Some(Directive::Move {
            unit_id: unit.id,
            q: map_center.q,
            r: map_center.r,
        })
    } else {
        None
    }
}

fn respond_to_backup(
    unit: &UnitInfo,
    backup_requests: &[BackupRequest],
    claimed: &mut [bool],
) -> Option<Directive> {
    let unit_pos = Axial::new(unit.q, unit.r);

    let mut best: Option<(usize, f32)> = None;

    for (i, req) in backup_requests.iter().enumerate() {
        if claimed[i] {
            continue;
        }
        let dist = hex::distance(unit_pos, req.pos);
        if dist > 15 {
            continue;
        }
        let score = req.urgency * 10.0 - dist as f32;
        if best.map_or(true, |(_, s)| score > s) {
            best = Some((i, score));
        }
    }

    if let Some((idx, _)) = best {
        claimed[idx] = true;
        let target = backup_requests[idx].pos;
        Some(Directive::Move {
            unit_id: unit.id,
            q: target.q,
            r: target.r,
        })
    } else {
        None
    }
}

fn find_engageable_enemy(
    unit: &UnitInfo,
    enemy_by_pos: &HashMap<(i32, i32), &UnitInfo>,
    friendly_counts: &HashMap<u32, usize>,
    policy: Policy,
) -> Option<u32> {
    let unit_pos = Axial::new(unit.q, unit.r);
    let candidates: Vec<&&UnitInfo> = hex::neighbors(unit_pos)
        .iter()
        .filter_map(|nb| enemy_by_pos.get(&(nb.q, nb.r)))
        .collect();

    if candidates.is_empty() {
        return None;
    }

    candidates
        .into_iter()
        .filter(|e| {
            let friends = friendly_counts.get(&e.id).copied().unwrap_or(0);
            let already_engaged = e.engagements.len();

            match policy {
                Policy::Aggressive => {
                    friends >= 2
                        || already_engaged >= 1
                        || unit.strength > e.strength * 1.3
                }
                Policy::Defensive => {
                    friends >= 2 || already_engaged >= 1 || unit.strength > e.strength * 1.5
                }
                Policy::Flanking => friends >= 2 || already_engaged >= 1,
            }
        })
        .min_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
        .map(|e| e.id)
}

fn pick_destination(
    unit: &UnitInfo,
    idx: usize,
    obs: &Observation,
    map_center: Axial,
    enemy_target: Option<Axial>,
    policy: Policy,
) -> Option<Axial> {
    match policy {
        Policy::Defensive => pick_sector_destination(unit, idx, obs, map_center),
        Policy::Aggressive => {
            if obs.own_units.len() <= 8 {
                pick_sector_destination(unit, idx, obs, map_center)
            } else {
                pick_lane_destination(unit, obs, enemy_target)
            }
        }
        Policy::Flanking => pick_flank_destination(unit, idx, obs, enemy_target),
    }
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

    let target_row = target_row.clamp(1, obs.height as i32 - 2);
    let target_col = target_col.clamp(1, obs.width as i32 - 2);

    Some(hex::offset_to_axial(target_row, target_col))
}

fn pick_lane_destination(
    unit: &UnitInfo,
    obs: &Observation,
    enemy_target: Option<Axial>,
) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);

    let chase_enemy = obs
        .visible_enemies
        .iter()
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 10)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)));

    if let Some(enemy) = chase_enemy {
        return Some(Axial::new(enemy.q, enemy.r));
    }

    let target = enemy_target?;
    let (target_r, target_c) = hex::axial_to_offset(target);
    let (unit_r, unit_c) = hex::axial_to_offset(unit_pos);

    let dx = target_c - unit_c;
    let dy = target_r - unit_r;
    let lane_hash = ((unit.id as i32 * 7 + 13) % 5) - 2;
    let perp_offset = lane_hash * 3;

    let dest_r = target_r + if dx != 0 { perp_offset * dx.signum() } else { 0 };
    let dest_c = target_c + if dy != 0 { perp_offset * dy.signum() } else { 0 };

    let dest_r = dest_r.clamp(1, obs.height as i32 - 2);
    let dest_c = dest_c.clamp(1, obs.width as i32 - 2);

    Some(hex::offset_to_axial(dest_r, dest_c))
}

fn pick_flank_destination(
    unit: &UnitInfo,
    idx: usize,
    obs: &Observation,
    enemy_target: Option<Axial>,
) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);

    let chase_enemy = obs
        .visible_enemies
        .iter()
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 6)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)));

    if let Some(enemy) = chase_enemy {
        return Some(Axial::new(enemy.q, enemy.r));
    }

    let target = enemy_target?;

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
    let approach_radius = 4.0;

    let (target_r, target_c) = hex::axial_to_offset(target);
    let dest_r = (target_r as f32 + angle.sin() * approach_radius) as i32;
    let dest_c = (target_c as f32 + angle.cos() * approach_radius) as i32;

    let dest_r = dest_r.clamp(1, obs.height as i32 - 2);
    let dest_c = dest_c.clamp(1, obs.width as i32 - 2);

    Some(hex::offset_to_axial(dest_r, dest_c))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn count_friendlies_near_enemies(obs: &Observation) -> HashMap<u32, usize> {
    let mut counts: HashMap<u32, usize> = HashMap::new();
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

fn enemy_direction(obs: &Observation) -> Option<Axial> {
    if let Some(enemy_gen) = obs.visible_enemies.iter().find(|e| e.is_general) {
        return Some(Axial::new(enemy_gen.q, enemy_gen.r));
    }

    if let Some(c) = enemy_centroid(obs) {
        return Some(c);
    }

    let own_gen = obs.own_units.iter().find(|u| u.is_general)?;
    let (gen_row, gen_col) = hex::axial_to_offset(Axial::new(own_gen.q, own_gen.r));

    let target_row = ((obs.height as i32 - 1) - gen_row).clamp(0, obs.height as i32 - 1);
    let target_col = ((obs.width as i32 - 1) - gen_col).clamp(0, obs.width as i32 - 1);
    Some(hex::offset_to_axial(target_row, target_col))
}

fn enemy_centroid(obs: &Observation) -> Option<Axial> {
    if obs.visible_enemies.is_empty() {
        return None;
    }
    let sum_q: i32 = obs.visible_enemies.iter().map(|e| e.q).sum();
    let sum_r: i32 = obs.visible_enemies.iter().map(|e| e.r).sum();
    let n = obs.visible_enemies.len() as i32;
    Some(Axial::new(sum_q / n, sum_r / n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::mapgen::{MapConfig, generate};
    use crate::v2::observation::observe;

    #[test]
    fn spread_agent_produces_when_affordable() {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        });
        state.players[0].food = 25.0;
        state.players[0].material = 14.0;
        let obs = observe(&state, 0);
        let mut agent = SpreadAgent::new();
        let directives = agent.act(&obs);
        let produce_count = directives
            .iter()
            .filter(|d| matches!(d, Directive::Produce))
            .count();
        // food: 25/8=3, material: 14/5=2 → min = 2 produces
        assert_eq!(produce_count, 2);
    }

    #[test]
    fn spread_agent_moves_idle_units() {
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });
        let obs = observe(&state, 0);
        let mut agent = SpreadAgent::new();
        let directives = agent.act(&obs);
        let move_count = directives
            .iter()
            .filter(|d| matches!(d, Directive::Move { .. }))
            .count();
        assert!(move_count > 0, "agent should move idle units");
    }

    #[test]
    fn spread_agent_fans_out_in_sectors() {
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });
        let obs = observe(&state, 0);
        let mut agent = SpreadAgent::new();
        let directives = agent.act(&obs);

        let dests: Vec<(i32, i32)> = directives
            .iter()
            .filter_map(|d| match d {
                Directive::Move { q, r, .. } => Some((*q, *r)),
                _ => None,
            })
            .collect();
        if dests.len() >= 2 {
            let unique: std::collections::HashSet<(i32, i32)> = dests.iter().copied().collect();
            assert!(
                unique.len() >= 2,
                "units should fan out to different destinations"
            );
        }
    }

    #[test]
    fn spread_agent_policy_starts_defensive_when_few_units() {
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });
        let obs = observe(&state, 0);
        let policy = evaluate_policy(&obs);
        assert_eq!(policy, Policy::Defensive);
    }

    #[test]
    fn spread_agent_reset_clears_state() {
        let mut agent = SpreadAgent::new();
        agent.policy = Policy::Flanking;
        agent.last_strategic_tick = 999;
        agent.backup_requests.push(BackupRequest {
            pos: Axial::new(0, 0),
            urgency: 1.0,
        });
        agent.reset();
        assert_eq!(agent.policy, Policy::Aggressive);
        assert_eq!(agent.last_strategic_tick, 0);
        assert!(agent.backup_requests.is_empty());
    }
}
