use std::collections::HashMap;

use super::UNIT_COST;
use super::directive::Directive;
use super::hex::{self, Axial};
use super::observation::{Observation, UnitInfo};

/// Observe → act interface for V2 game agents.
/// Send bound enables future use across threads.
pub trait Agent: Send {
    fn name(&self) -> &str;
    fn act(&mut self, obs: &Observation) -> Vec<Directive>;
    fn reset(&mut self) {}
}

/// Stateless heuristic agent: produces units, spreads toward unexplored terrain,
/// engages adjacent enemies when odds are reasonable, and advances toward the
/// enemy half of the map so games reliably converge.
pub struct SpreadAgent;

impl Agent for SpreadAgent {
    fn name(&self) -> &str {
        "spread"
    }

    fn act(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();

        // Produce at most 3 units per poll to prevent unbounded army growth
        let mut remaining_resources = obs.resources;
        let mut produced = 0;
        while remaining_resources >= UNIT_COST && produced < 3 {
            directives.push(Directive::Produce);
            remaining_resources -= UNIT_COST;
            produced += 1;
        }

        // Build a position→enemy lookup to allow O(6) engagement checks per unit
        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();

        // Late game: march all non-general units toward enemy when army is large
        if obs.own_units.len() > 8 {
            let dest = enemy_direction(obs);
            for unit in &obs.own_units {
                if !unit.engagements.is_empty() || unit.is_general {
                    continue;
                }
                if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos) {
                    directives.push(Directive::Engage {
                        unit_id: unit.id,
                        target_id: target,
                    });
                    continue;
                }
                if let Some(d) = dest {
                    directives.push(Directive::Move {
                        unit_id: unit.id,
                        q: d.q,
                        r: d.r,
                    });
                }
            }
            return directives;
        }

        // Early game: spread to high-value terrain
        for unit in &obs.own_units {
            if !unit.engagements.is_empty() {
                continue;
            }
            if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos) {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }
            if unit.is_general {
                continue;
            }
            if let Some(d) = pick_destination_early(unit, obs) {
                directives.push(Directive::Move {
                    unit_id: unit.id,
                    q: d.q,
                    r: d.r,
                });
            }
        }

        directives
    }
}

/// Find an adjacent enemy this unit can engage using the neighbor lookup (O(6) per call).
/// Engages when the unit has at least 50% of the enemy's strength (aggressive).
/// Prefers the weakest adjacent enemy.
fn find_engageable_enemy(
    unit: &UnitInfo,
    enemy_by_pos: &HashMap<(i32, i32), &UnitInfo>,
) -> Option<u32> {
    let unit_pos = Axial::new(unit.q, unit.r);
    hex::neighbors(unit_pos)
        .iter()
        .filter_map(|nb| enemy_by_pos.get(&(nb.q, nb.r)).copied())
        .filter(|e| unit.strength >= e.strength * 0.5)
        .min_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
        .map(|e| e.id)
}

/// Estimate the enemy general's region and return an in-bounds target axial coord.
/// Shared across all units in late game (computed once per act call).
fn enemy_direction(obs: &Observation) -> Option<Axial> {
    // If we can see the enemy general, go straight for it
    if let Some(enemy_gen) = obs.visible_enemies.iter().find(|e| e.is_general) {
        return Some(Axial::new(enemy_gen.q, enemy_gen.r));
    }

    // If we can see any enemies, head toward the centroid of their positions
    if !obs.visible_enemies.is_empty() {
        let sum_q: i32 = obs.visible_enemies.iter().map(|e| e.q).sum();
        let sum_r: i32 = obs.visible_enemies.iter().map(|e| e.r).sum();
        let n = obs.visible_enemies.len() as i32;
        return Some(Axial::new(sum_q / n, sum_r / n));
    }

    // No visible enemies — use own general position to infer enemy side
    let own_gen = obs.own_units.iter().find(|u| u.is_general)?;
    let (gen_row, gen_col) = hex::axial_to_offset(Axial::new(own_gen.q, own_gen.r));

    let target_row = ((obs.height as i32 - 1) - gen_row).clamp(0, obs.height as i32 - 1);
    let target_col = ((obs.width as i32 - 1) - gen_col).clamp(0, obs.width as i32 - 1);
    Some(hex::offset_to_axial(target_row, target_col))
}

/// Early-game destination picker: spread to high-value terrain.
/// O(width/2 × height/2 × min(own_units, 8)) — bounded.
fn pick_destination_early(unit: &UnitInfo, obs: &Observation) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);

    // Priority 1: move toward the nearest visible enemy
    let target_enemy = obs
        .visible_enemies
        .iter()
        .filter(|e| e.strength <= unit.strength * 2.0)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)));

    if let Some(enemy) = target_enemy {
        return Some(Axial::new(enemy.q, enemy.r));
    }

    // Priority 2: scan terrain for the best spread destination (step-2 stride)
    let own_positions: Vec<Axial> = obs
        .own_units
        .iter()
        .filter(|u| u.id != unit.id)
        .take(8)
        .map(|u| Axial::new(u.q, u.r))
        .collect();

    let mut best_score = f32::NEG_INFINITY;
    let mut best_pos: Option<Axial> = None;

    let step = 2usize;
    let mut row = 0usize;
    while row < obs.height {
        let mut col = 0usize;
        while col < obs.width {
            let idx = row * obs.width + col;
            let ax = hex::offset_to_axial(row as i32, col as i32);
            let dist = hex::distance(unit_pos, ax);

            let min_friendly_dist = own_positions
                .iter()
                .map(|&p| hex::distance(p, ax))
                .min()
                .unwrap_or(999);

            let score = if !obs.visible[idx] {
                if dist < 3 || dist > 20 {
                    col += step;
                    continue;
                }
                min_friendly_dist as f32 * 2.0 - dist as f32
            } else {
                let terrain_value = obs.terrain[idx];
                if terrain_value < 1.5 || dist < 2 {
                    col += step;
                    continue;
                }
                terrain_value * 3.0 + min_friendly_dist as f32 * 1.5 - dist as f32 * 0.5
            };

            if score > best_score {
                best_score = score;
                best_pos = Some(ax);
            }

            col += step;
        }
        row += step;
    }

    // Fallback: advance toward enemy territory
    best_pos.or_else(|| enemy_direction(obs))
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
        state.players[0].resources = 25.0;
        let obs = observe(&state, 0);
        let mut agent = SpreadAgent;
        let directives = agent.act(&obs);
        let produce_count = directives
            .iter()
            .filter(|d| matches!(d, Directive::Produce))
            .count();
        // 25.0 resources → 2 produces (capped at 3 per poll, 25/10=2)
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
        let mut agent = SpreadAgent;
        let directives = agent.act(&obs);
        let move_count = directives
            .iter()
            .filter(|d| matches!(d, Directive::Move { .. }))
            .count();
        assert!(move_count > 0, "agent should move idle units");
    }

    #[test]
    fn spread_agent_does_not_move_general() {
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });
        let obs = observe(&state, 0);
        let general_id = obs.own_units.iter().find(|u| u.is_general).unwrap().id;
        let mut agent = SpreadAgent;
        let directives = agent.act(&obs);
        let general_moved = directives
            .iter()
            .any(|d| matches!(d, Directive::Move { unit_id, .. } if *unit_id == general_id));
        assert!(!general_moved, "agent should not move the general");
    }
}
