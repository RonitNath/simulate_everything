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

/// Aggressive heuristic agent: expands fast, engages early, advances in lanes.
pub struct SpreadAgent;

impl Agent for SpreadAgent {
    fn name(&self) -> &str {
        "spread"
    }

    fn act(&mut self, obs: &Observation) -> Vec<Directive> {
        let mut directives = Vec::new();

        // Produce whenever affordable — resource cost is the natural throttle
        let mut remaining_food = obs.food;
        let mut remaining_material = obs.material;
        let mut produce_count = 0u32;
        while remaining_food >= UNIT_FOOD_COST && remaining_material >= UNIT_MATERIAL_COST {
            directives.push(Directive::Produce);
            remaining_food -= UNIT_FOOD_COST;
            remaining_material -= UNIT_MATERIAL_COST;
            produce_count += 1;
        }

        // Build position→enemy lookup for O(6) engagement checks
        let enemy_by_pos: HashMap<(i32, i32), &UnitInfo> = obs
            .visible_enemies
            .iter()
            .map(|e| ((e.q, e.r), e))
            .collect();

        // Count own units near each enemy for gang-up decisions
        let friendly_near_enemy: HashMap<u32, usize> = count_friendlies_near_enemies(obs);

        let map_center = hex::offset_to_axial(obs.height as i32 / 2, obs.width as i32 / 2);
        let enemy_target = enemy_direction(obs);

        for (idx, unit) in obs.own_units.iter().enumerate() {
            if !unit.engagements.is_empty() {
                continue;
            }

            // Try to engage adjacent enemies
            if let Some(target) = find_engageable_enemy(unit, &enemy_by_pos, &friendly_near_enemy) {
                directives.push(Directive::Engage {
                    unit_id: unit.id,
                    target_id: target,
                });
                continue;
            }

            // General: move toward center once we have escorts
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

            // Pick destination based on game phase
            let dest = if obs.own_units.len() <= 8 {
                // Early: fan out toward center in assigned sectors
                pick_sector_destination(unit, idx, obs, map_center)
            } else {
                // Late: advance in lanes toward the enemy
                pick_lane_destination(unit, idx, obs, enemy_target)
            };

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

        tracing::trace!(
            tick = obs.tick,
            player = obs.player,
            own_units = obs.own_units.len(),
            visible_enemies = obs.visible_enemies.len(),
            food = format_args!("{:.1}", obs.food),
            material = format_args!("{:.1}", obs.material),
            produced = produce_count,
            moves = move_count,
            engages = engage_count,
            already_engaged = engaged_count,
            "spread agent act"
        );

        directives
    }
}

/// Count how many own units are adjacent to each visible enemy.
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

/// Find an adjacent enemy to engage.
/// Engages aggressively: any adjacent enemy when we have numerical advantage,
/// or when we have at least 50% strength.
fn find_engageable_enemy(
    unit: &UnitInfo,
    enemy_by_pos: &HashMap<(i32, i32), &UnitInfo>,
    friendly_counts: &HashMap<u32, usize>,
) -> Option<u32> {
    let unit_pos = Axial::new(unit.q, unit.r);
    hex::neighbors(unit_pos)
        .iter()
        .filter_map(|nb| enemy_by_pos.get(&(nb.q, nb.r)).copied())
        .filter(|e| {
            let friends = friendly_counts.get(&e.id).copied().unwrap_or(0);
            // Engage if we outnumber them OR have decent strength
            friends >= 2 || unit.strength >= e.strength * 0.5
        })
        .min_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
        .map(|e| e.id)
}

/// Early game: fan out toward map center in angular sectors.
/// Each non-general unit gets a sector based on its index, creating a fan pattern.
fn pick_sector_destination(
    unit: &UnitInfo,
    idx: usize,
    obs: &Observation,
    map_center: Axial,
) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);

    // If there's a visible enemy nearby, chase it
    let nearby_enemy = obs
        .visible_enemies
        .iter()
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 8)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)));

    if let Some(enemy) = nearby_enemy {
        return Some(Axial::new(enemy.q, enemy.r));
    }

    // Fan out toward center with angular offset per unit
    let non_general_count = obs
        .own_units
        .iter()
        .filter(|u| !u.is_general)
        .count()
        .max(1);
    // Use index among non-generals for sector assignment
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

/// Late game: advance in 2-3 lanes toward the enemy.
/// Units are assigned to lanes based on their position relative to the attack direction.
fn pick_lane_destination(
    unit: &UnitInfo,
    _idx: usize,
    obs: &Observation,
    enemy_target: Option<Axial>,
) -> Option<Axial> {
    let unit_pos = Axial::new(unit.q, unit.r);

    // If there's a visible enemy within striking range, go straight for it
    let chase_enemy = obs
        .visible_enemies
        .iter()
        .filter(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)) <= 10)
        .min_by_key(|e| hex::distance(unit_pos, Axial::new(e.q, e.r)));

    if let Some(enemy) = chase_enemy {
        return Some(Axial::new(enemy.q, enemy.r));
    }

    // No nearby enemy — advance toward the target with lane spread
    let target = enemy_target?;
    let (target_r, target_c) = hex::axial_to_offset(target);
    let (unit_r, unit_c) = hex::axial_to_offset(unit_pos);

    // Offset perpendicular to the attack direction for lane spacing
    let dx = target_c - unit_c;
    let dy = target_r - unit_r;
    // Perpendicular offset based on unit position hash for consistent lane assignment
    let lane_hash = ((unit.id as i32 * 7 + 13) % 5) - 2; // -2, -1, 0, 1, 2
    let perp_offset = lane_hash * 3; // 3-hex lane spacing

    // Apply perpendicular offset (rotate 90 degrees: (dx,dy) -> (-dy,dx))
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

    let dest_r = dest_r.clamp(1, obs.height as i32 - 2);
    let dest_c = dest_c.clamp(1, obs.width as i32 - 2);

    Some(hex::offset_to_axial(dest_r, dest_c))
}

/// Estimate the enemy general's region.
fn enemy_direction(obs: &Observation) -> Option<Axial> {
    // If we can see the enemy general, go straight for it
    if let Some(enemy_gen) = obs.visible_enemies.iter().find(|e| e.is_general) {
        return Some(Axial::new(enemy_gen.q, enemy_gen.r));
    }

    // Head toward centroid of visible enemies
    if !obs.visible_enemies.is_empty() {
        let sum_q: i32 = obs.visible_enemies.iter().map(|e| e.q).sum();
        let sum_r: i32 = obs.visible_enemies.iter().map(|e| e.r).sum();
        let n = obs.visible_enemies.len() as i32;
        return Some(Axial::new(sum_q / n, sum_r / n));
    }

    // No visible enemies — mirror own general position across the map center
    let own_gen = obs.own_units.iter().find(|u| u.is_general)?;
    let (gen_row, gen_col) = hex::axial_to_offset(Axial::new(own_gen.q, own_gen.r));

    let target_row = ((obs.height as i32 - 1) - gen_row).clamp(0, obs.height as i32 - 1);
    let target_col = ((obs.width as i32 - 1) - gen_col).clamp(0, obs.width as i32 - 1);
    Some(hex::offset_to_axial(target_row, target_col))
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
        let mut agent = SpreadAgent;
        let directives = agent.act(&obs);
        let produce_count = directives
            .iter()
            .filter(|d| matches!(d, Directive::Produce))
            .count();
        // Enough for two produces on both resource axes.
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
    fn spread_agent_fans_out_in_sectors() {
        let state = generate(&MapConfig {
            width: 30,
            height: 30,
            num_players: 2,
            seed: 42,
        });
        let obs = observe(&state, 0);
        let mut agent = SpreadAgent;
        let directives = agent.act(&obs);

        // Collect move destinations — they should not all be the same
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
}
