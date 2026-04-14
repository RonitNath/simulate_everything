/// Perception layer: builds a StrategicView from game state with fog of war.
/// Strategy never reads raw game state — it reads this abstraction.
use serde::{Deserialize, Serialize};

use super::derived::{derive_hex_control, derive_player_stats, region_center};
use super::spatial::Vec3;
use super::state::GameState;
use super::terrain_ops::TerrainOp;
use crate::v2::hex::{Axial, neighbors};

// ---------------------------------------------------------------------------
// StrategicView
// ---------------------------------------------------------------------------

/// Abstracted game state as seen by the strategy layer. Fog of war applied.
/// All personalities read the same view; personality is the policy, not perception.
#[derive(Debug, Clone)]
pub struct StrategicView {
    /// Clusters of hexes: controlled, contested, unknown.
    pub territory: Vec<Region>,
    /// Aggregate own vs visible enemy strength.
    pub relative_strength: StrengthAssessment,
    /// Economic snapshot: food, materials, production capacity.
    pub economy: EconomySnapshot,
    /// Enemy concentrations visible to this player.
    pub threats: Vec<ThreatCluster>,
    /// Per-stack readiness assessment.
    pub stack_readiness: Vec<StackHealth>,
    /// Terrain infrastructure, opportunity, and damage state.
    pub terrain: TerrainAssessment,
}

/// A cluster of hexes with a territorial status.
#[derive(Debug, Clone)]
pub struct Region {
    pub center: Axial,
    pub hex_count: u32,
    pub status: TerritoryStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerritoryStatus {
    Controlled,
    Contested,
    Unknown,
}

/// Aggregate strength comparison.
#[derive(Debug, Clone)]
pub struct StrengthAssessment {
    /// Total own stack count.
    pub own_stacks: u32,
    /// Total visible enemy stack count.
    pub enemy_stacks: u32,
    /// Own military entity count (soldiers).
    pub own_soldiers: u32,
    /// Visible enemy military entity count.
    pub enemy_soldiers: u32,
    /// Rough equipment quality ratio (0.0 = terrible, 1.0 = parity, 2.0 = dominant).
    pub equipment_quality_ratio: f32,
}

/// Snapshot of economic state.
#[derive(Debug, Clone)]
pub struct EconomySnapshot {
    /// Net food production minus consumption per tick.
    pub food_surplus: f32,
    /// Total material stockpile.
    pub material_stockpile: f32,
    /// Number of workshops currently producing.
    pub production_capacity: u32,
    /// Population growth trend (positive = growing).
    pub growth_trend: f32,
}

/// Observed enemy concentration.
#[derive(Debug, Clone)]
pub struct ThreatCluster {
    /// Estimated center of enemy force.
    pub position: Vec3,
    /// Direction of movement (None if stationary).
    pub direction: Option<Vec3>,
    /// Estimated entity count.
    pub entity_count: u32,
    /// Whether this group is advancing, static, or retreating.
    pub posture: ThreatPosture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreatPosture {
    Advancing,
    Static,
    Retreating,
}

/// Aggregate readiness of a stack.
#[derive(Debug, Clone)]
pub struct StackHealth {
    pub stack_id: super::state::StackId,
    pub readiness: Readiness,
    pub member_count: u32,
}

#[derive(Debug, Clone)]
pub struct TerrainAssessment {
    /// Road operations in controlled territory. Higher = better logistics.
    pub road_coverage: f32,
    /// Ditch + Wall operations on frontier hexes. Higher = better defense.
    pub fortification_density: f32,
    /// Furrow operations in controlled territory. Higher = better farming.
    pub farming_improvements: f32,
    /// Crater operations in controlled territory. Higher = more war damage.
    pub damage_density: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// All members healthy and equipped.
    Fresh,
    /// Some members wounded but still combat-effective.
    Wounded,
    /// Majority wounded or under-equipped.
    Depleted,
}

// ---------------------------------------------------------------------------
// Perception builder
// ---------------------------------------------------------------------------

pub fn build_strategic_view(state: &GameState, player: u8) -> StrategicView {
    let player_stats = derive_player_stats(state);
    let own_stats = &player_stats[player as usize];
    let enemy_stats = aggregate_enemy_stats(&player_stats, player);
    let territory = summarize_territory(state, player);
    let own_stacks = state.stacks.iter().filter(|s| s.owner == player).count() as u32;
    let enemy_stacks = state.stacks.iter().filter(|s| s.owner != player).count() as u32;

    // Stack readiness.
    let stack_readiness: Vec<StackHealth> = state
        .stacks
        .iter()
        .filter(|s| s.owner == player)
        .map(|s| {
            let member_count = s.members.len() as u32;
            let wounded_count = s
                .members
                .iter()
                .filter(|&&m| {
                    state
                        .entities
                        .get(m)
                        .and_then(|e| e.wounds.as_ref())
                        .map(|w| !w.is_empty())
                        .unwrap_or(false)
                })
                .count() as u32;

            let readiness = if wounded_count == 0 {
                Readiness::Fresh
            } else if wounded_count * 2 < member_count {
                Readiness::Wounded
            } else {
                Readiness::Depleted
            };

            StackHealth {
                stack_id: s.id,
                readiness,
                member_count,
            }
        })
        .collect();

    StrategicView {
        territory,
        relative_strength: StrengthAssessment {
            own_stacks,
            enemy_stacks,
            own_soldiers: own_stats.soldiers,
            enemy_soldiers: enemy_stats.soldiers,
            equipment_quality_ratio: equipment_quality_ratio(state, player),
        },
        economy: EconomySnapshot {
            food_surplus: own_stats.farmers as f32 - own_stats.soldiers as f32 * 0.4,
            material_stockpile: own_stats.material_stockpile,
            production_capacity: own_stats.workers + own_stats.workshops,
            growth_trend: own_stats.food_stockpile / (own_stats.population.max(1) as f32) - 5.0,
        },
        threats: detect_threats(state, player),
        stack_readiness,
        terrain: assess_terrain(state, player),
    }
}

fn assess_terrain(state: &GameState, player: u8) -> TerrainAssessment {
    let control = derive_hex_control(state);
    let mut roads = 0.0;
    let mut fortifications = 0.0;
    let mut farming = 0.0;
    let mut damage = 0.0;
    let mut controlled_hexes: f32 = 0.0;

    for row in 0..state.map_height as i32 {
        for col in 0..state.map_width as i32 {
            let hex = crate::v2::hex::offset_to_axial(row, col);
            let Some(idx) = hex_index(state, hex) else {
                continue;
            };
            let Some(hex_control) = control.get(idx) else {
                continue;
            };
            let is_controlled = hex_control.owner == Some(player) && !hex_control.contested;
            if !is_controlled {
                continue;
            }

            controlled_hexes += 1.0;
            let is_frontier = neighbors(hex).into_iter().any(|neighbor| {
                hex_index(state, neighbor)
                    .and_then(|neighbor_idx| control.get(neighbor_idx))
                    .map(|neighbor_control| {
                        neighbor_control.owner != Some(player) || neighbor_control.contested
                    })
                    .unwrap_or(true)
            });

            for op in state.terrain_ops.ops_for_hex(hex) {
                match op {
                    TerrainOp::Road { .. } => roads += 1.0,
                    TerrainOp::Ditch { .. } | TerrainOp::Wall { .. } if is_frontier => {
                        fortifications += 1.0
                    }
                    TerrainOp::Furrow { .. } => farming += 1.0,
                    TerrainOp::Crater { .. } => damage += 1.0,
                    _ => {}
                }
            }
        }
    }

    let denom = controlled_hexes.max(1.0);
    TerrainAssessment {
        road_coverage: roads / denom,
        fortification_density: fortifications / denom,
        farming_improvements: farming / denom,
        damage_density: damage / denom,
    }
}

// ---------------------------------------------------------------------------
// Threat detection
// ---------------------------------------------------------------------------

/// Basic threat detection: group enemy entities by hex proximity into clusters.
/// Each enemy entity with a position becomes a single-entity cluster for now.
/// Future: merge nearby entities into larger clusters.
fn detect_threats(state: &GameState, player: u8) -> Vec<ThreatCluster> {
    let mut threats = Vec::new();

    for entity in state.entities.values() {
        let owner = match entity.owner {
            Some(o) => o,
            None => continue,
        };
        if owner == player {
            continue;
        }
        if entity.person.is_none() {
            continue;
        }
        let pos = match entity.pos {
            Some(p) => p,
            None => continue,
        };
        let direction = entity
            .mobile
            .as_ref()
            .and_then(|m| (m.vel.length_squared() > 0.01).then_some(m.vel));

        threats.push(ThreatCluster {
            position: pos,
            direction,
            entity_count: 1,
            posture: ThreatPosture::Static,
        });
    }

    threats
}

fn summarize_territory(state: &GameState, player: u8) -> Vec<Region> {
    let control = derive_hex_control(state);
    let mut controlled = Vec::new();
    let mut contested = Vec::new();
    let mut frontier_unknown = Vec::new();

    for (idx, hex) in control.iter().enumerate() {
        if hex.owner == Some(player) && !hex.contested {
            controlled.push(idx);
            continue;
        }
        if hex.contested {
            contested.push(idx);
        }
    }

    for (idx, hex) in control.iter().enumerate() {
        if hex.owner.is_some() || hex.contested {
            continue;
        }
        let row = (idx / state.map_width) as i32;
        let col = (idx % state.map_width) as i32;
        let axial = Axial::new(col - (row - (row & 1)) / 2, row);
        if neighbors(axial).into_iter().any(|neighbor| {
            hex_index(state, neighbor)
                .and_then(|neighbor_idx| control.get(neighbor_idx))
                .map(|neighbor_control| neighbor_control.owner == Some(player))
                .unwrap_or(false)
        }) {
            frontier_unknown.push(idx);
        }
    }

    let mut regions = Vec::new();
    if !controlled.is_empty() {
        regions.push(Region {
            center: region_center(state, &controlled),
            hex_count: controlled.len() as u32,
            status: TerritoryStatus::Controlled,
        });
    }
    if !contested.is_empty() {
        regions.push(Region {
            center: region_center(state, &contested),
            hex_count: contested.len() as u32,
            status: TerritoryStatus::Contested,
        });
    }
    if !frontier_unknown.is_empty() {
        regions.push(Region {
            center: region_center(state, &frontier_unknown),
            hex_count: frontier_unknown.len() as u32,
            status: TerritoryStatus::Unknown,
        });
    }
    regions
}

fn aggregate_enemy_stats(
    player_stats: &[super::derived::PlayerDerivedStats],
    player: u8,
) -> super::derived::PlayerDerivedStats {
    let mut aggregate = super::derived::PlayerDerivedStats {
        id: player,
        population: 0,
        soldiers: 0,
        farmers: 0,
        workers: 0,
        idle: 0,
        workshops: 0,
        settlements: 0,
        territory: 0,
        food_stockpile: 0.0,
        material_stockpile: 0.0,
        alive: false,
    };

    for stats in player_stats.iter().filter(|stats| stats.id != player) {
        aggregate.population += stats.population;
        aggregate.soldiers += stats.soldiers;
        aggregate.farmers += stats.farmers;
        aggregate.workers += stats.workers;
        aggregate.idle += stats.idle;
        aggregate.workshops += stats.workshops;
        aggregate.settlements += stats.settlements;
        aggregate.territory += stats.territory;
        aggregate.food_stockpile += stats.food_stockpile;
        aggregate.material_stockpile += stats.material_stockpile;
        aggregate.alive |= stats.alive;
    }

    aggregate
}

fn equipment_quality_ratio(state: &GameState, player: u8) -> f32 {
    let own = player_equipment_score(state, player);
    let enemy = (0..state.num_players)
        .filter(|&other| other != player)
        .map(|other| player_equipment_score(state, other))
        .sum::<f32>();
    if enemy <= 0.01 {
        return if own <= 0.01 { 1.0 } else { 2.0 };
    }
    (own / enemy).clamp(0.25, 2.0)
}

fn player_equipment_score(state: &GameState, player: u8) -> f32 {
    state
        .entities
        .values()
        .filter(|entity| {
            entity.owner == Some(player)
                && entity
                    .person
                    .as_ref()
                    .map(|person| person.role == super::state::Role::Soldier)
                    .unwrap_or(false)
        })
        .map(|entity| {
            let mut score = 0.0;
            if entity
                .equipment
                .as_ref()
                .and_then(|equipment| equipment.weapon)
                .is_some()
            {
                score += 1.0;
            }
            if entity
                .equipment
                .as_ref()
                .map(|equipment| equipment.armor_slots.iter().any(|slot| slot.is_some()))
                .unwrap_or(false)
            {
                score += 1.0;
            }
            score
        })
        .sum()
}

fn hex_index(state: &GameState, hex: Axial) -> Option<usize> {
    let row = hex.r;
    let col = hex.q + (hex.r - (hex.r & 1)) / 2;
    if row < 0 || col < 0 {
        return None;
    }
    let row = row as usize;
    let col = col as usize;
    if row >= state.map_height || col >= state.map_width {
        return None;
    }
    Some(row * state.map_width + col)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::hex::hex_to_world;
    use super::super::formation::FormationType;
    use super::super::lifecycle::spawn_entity;
    use super::super::movement::Mobile;
    use super::super::physical::{MatterStack, SiteProperties};
    use super::super::spatial::{GeoMaterial, Heightfield, Vec2};
    use super::super::state::{Combatant, CommodityKind, EntityBuilder, Person, Role, Stack};
    use super::super::terrain_ops::{DitchProfile, TerrainOp, WallProfile};
    use super::*;
    use simulate_everything_protocol::PropertyTag;
    use smallvec::SmallVec;

    fn test_state() -> GameState {
        let hf = Heightfield::new(10, 10, 0.0, GeoMaterial::Soil);
        GameState::new(10, 10, 2, hf)
    }

    fn spawn_soldier(state: &mut GameState, pos: Vec3, owner: u8) -> crate::v2::state::EntityKey {
        spawn_entity(
            state,
            EntityBuilder::new()
                .pos(pos)
                .owner(owner)
                .person(Person {
                    role: Role::Soldier,
                    combat_skill: 0.5,
                })
                .mobile(Mobile::new(2.0, 10.0))
                .combatant(Combatant::new())
                .vitals(),
        )
    }

    fn control_hex(state: &mut GameState, hex: Axial, owner: u8) {
        spawn_soldier(state, hex_to_world(hex), owner);
    }

    fn push_terrain_op(state: &mut GameState, hex: Axial, op: TerrainOp) {
        let heightfield = state.heightfield.clone();
        state.terrain_ops.push_op(
            hex,
            op,
            &heightfield,
            state.map_width,
            state.map_height,
        );
    }

    #[test]
    fn view_counts_soldiers() {
        let mut state = test_state();
        spawn_soldier(&mut state, Vec3::new(0.0, 0.0, 0.0), 0);
        spawn_soldier(&mut state, Vec3::new(10.0, 0.0, 0.0), 0);
        spawn_soldier(&mut state, Vec3::new(100.0, 0.0, 0.0), 1);

        let view = build_strategic_view(&state, 0);
        assert_eq!(view.relative_strength.own_soldiers, 2);
        assert_eq!(view.relative_strength.enemy_soldiers, 1);
    }

    #[test]
    fn view_counts_stacks() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(0.0, 0.0, 0.0), 0);
        let s2 = spawn_soldier(&mut state, Vec3::new(100.0, 0.0, 0.0), 1);

        let id1 = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: id1,
            owner: 0,
            members: SmallVec::from_slice(&[s1]),
            formation: FormationType::Line,
            leader: s1,
        });
        let id2 = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: id2,
            owner: 1,
            members: SmallVec::from_slice(&[s2]),
            formation: FormationType::Line,
            leader: s2,
        });

        let view = build_strategic_view(&state, 0);
        assert_eq!(view.relative_strength.own_stacks, 1);
        assert_eq!(view.relative_strength.enemy_stacks, 1);
    }

    #[test]
    fn stack_readiness_fresh_when_no_wounds() {
        let mut state = test_state();
        let s1 = spawn_soldier(&mut state, Vec3::new(0.0, 0.0, 0.0), 0);

        let id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id,
            owner: 0,
            members: SmallVec::from_slice(&[s1]),
            formation: FormationType::Line,
            leader: s1,
        });

        let view = build_strategic_view(&state, 0);
        assert_eq!(view.stack_readiness.len(), 1);
        assert_eq!(view.stack_readiness[0].readiness, Readiness::Fresh);
    }

    #[test]
    fn view_builds_territory_and_economy_from_state() {
        let mut state = test_state();
        spawn_soldier(&mut state, Vec3::new(0.0, 0.0, 0.0), 0);
        spawn_entity(
            &mut state,
            EntityBuilder::new()
                .owner(0)
                .physical(super::super::economy::stockpile_physical(
                    CommodityKind::Material,
                ))
                .matter(MatterStack {
                    commodity: CommodityKind::Material,
                    amount: 80.0,
                }),
        );
        spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(20.0, 0.0, 0.0))
                .owner(0)
                .physical(super::super::economy::site_physical(PropertyTag::Workshop))
                .site(SiteProperties {
                    build_progress: 1.0,
                    integrity: 100.0,
                    occupancy_capacity: 10,
                }),
        );
        spawn_entity(
            &mut state,
            EntityBuilder::new()
                .pos(Vec3::new(5.0, 0.0, 0.0))
                .owner(0)
                .person(Person {
                    role: Role::Farmer,
                    combat_skill: 0.1,
                })
                .mobile(Mobile::new(2.0, 10.0)),
        );

        let view = build_strategic_view(&state, 0);
        assert!(
            !view.territory.is_empty(),
            "territory should no longer be empty"
        );
        assert!(
            view.economy.material_stockpile >= 80.0,
            "economy should include owned material stockpiles"
        );
        assert!(
            view.economy.production_capacity >= 1,
            "workshops and workers should contribute to production capacity"
        );
    }

    #[test]
    fn test_terrain_empty() {
        let mut state = test_state();
        control_hex(&mut state, Axial::new(0, 0), 0);

        let view = build_strategic_view(&state, 0);
        assert_eq!(view.terrain.road_coverage, 0.0);
        assert_eq!(view.terrain.fortification_density, 0.0);
        assert_eq!(view.terrain.farming_improvements, 0.0);
        assert_eq!(view.terrain.damage_density, 0.0);
    }

    #[test]
    fn test_terrain_road_coverage() {
        let mut state = test_state();
        let hex = Axial::new(0, 0);
        control_hex(&mut state, hex, 0);
        push_terrain_op(
            &mut state,
            hex,
            TerrainOp::Road {
                points: SmallVec::from_slice(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)]),
                width: 2.0,
                grade: 0.0,
                material: GeoMaterial::Rock,
            },
        );

        let view = build_strategic_view(&state, 0);
        assert!(view.terrain.road_coverage > 0.0);
        assert_eq!(view.terrain.fortification_density, 0.0);
        assert_eq!(view.terrain.farming_improvements, 0.0);
        assert_eq!(view.terrain.damage_density, 0.0);
    }

    #[test]
    fn test_terrain_fortification() {
        let mut state = test_state();
        let center = Axial::new(4, 4);
        control_hex(&mut state, center, 0);
        for neighbor in neighbors(center) {
            control_hex(&mut state, neighbor, 0);
        }

        let frontier_hex = neighbors(center)[0];
        push_terrain_op(
            &mut state,
            frontier_hex,
            TerrainOp::Ditch {
                start: Vec2::new(0.0, 0.0),
                end: Vec2::new(10.0, 0.0),
                width: 3.0,
                depth: 1.0,
                profile: DitchProfile::Trapezoidal,
            },
        );
        push_terrain_op(
            &mut state,
            center,
            TerrainOp::Ditch {
                start: Vec2::new(0.0, 0.0),
                end: Vec2::new(10.0, 0.0),
                width: 3.0,
                depth: 1.0,
                profile: DitchProfile::Trapezoidal,
            },
        );
        push_terrain_op(
            &mut state,
            frontier_hex,
            TerrainOp::Wall {
                start: Vec2::new(0.0, 0.0),
                end: Vec2::new(10.0, 0.0),
                width: 2.0,
                height: 1.0,
                profile: WallProfile::Rounded,
            },
        );

        let view = build_strategic_view(&state, 0);
        assert!(view.terrain.fortification_density > 0.0);
        assert!((view.terrain.fortification_density - (2.0 / 7.0)).abs() < 1e-6);
    }

    #[test]
    fn test_terrain_farming() {
        let mut state = test_state();
        let hex = Axial::new(1, 1);
        control_hex(&mut state, hex, 0);
        push_terrain_op(
            &mut state,
            hex,
            TerrainOp::Furrow {
                center: Vec2::new(0.0, 0.0),
                half_extents: Vec2::new(5.0, 2.0),
                rotation: 0.0,
                spacing: 1.0,
                depth: 0.5,
            },
        );

        let view = build_strategic_view(&state, 0);
        assert!(view.terrain.farming_improvements > 0.0);
        assert_eq!(view.terrain.road_coverage, 0.0);
        assert_eq!(view.terrain.fortification_density, 0.0);
        assert_eq!(view.terrain.damage_density, 0.0);
    }

    #[test]
    fn test_terrain_damage() {
        let mut state = test_state();
        let hex = Axial::new(2, 2);
        control_hex(&mut state, hex, 0);
        push_terrain_op(
            &mut state,
            hex,
            TerrainOp::Crater {
                center: Vec2::new(0.0, 0.0),
                radius: 5.0,
                depth: 1.5,
                rim_height: 0.3,
            },
        );

        let view = build_strategic_view(&state, 0);
        assert!(view.terrain.damage_density > 0.0);
        assert_eq!(view.terrain.road_coverage, 0.0);
        assert_eq!(view.terrain.fortification_density, 0.0);
        assert_eq!(view.terrain.farming_improvements, 0.0);
    }

    #[test]
    fn test_terrain_enemy_ops_not_counted() {
        let mut state = test_state();
        control_hex(&mut state, Axial::new(0, 0), 0);
        let enemy_hex = Axial::new(5, 5);
        control_hex(&mut state, enemy_hex, 1);
        push_terrain_op(
            &mut state,
            enemy_hex,
            TerrainOp::Road {
                points: SmallVec::from_slice(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)]),
                width: 2.0,
                grade: 0.0,
                material: GeoMaterial::Rock,
            },
        );
        push_terrain_op(
            &mut state,
            enemy_hex,
            TerrainOp::Furrow {
                center: Vec2::new(0.0, 0.0),
                half_extents: Vec2::new(5.0, 2.0),
                rotation: 0.0,
                spacing: 1.0,
                depth: 0.5,
            },
        );
        push_terrain_op(
            &mut state,
            enemy_hex,
            TerrainOp::Crater {
                center: Vec2::new(0.0, 0.0),
                radius: 5.0,
                depth: 1.5,
                rim_height: 0.3,
            },
        );

        let view = build_strategic_view(&state, 0);
        assert_eq!(view.terrain.road_coverage, 0.0);
        assert_eq!(view.terrain.fortification_density, 0.0);
        assert_eq!(view.terrain.farming_improvements, 0.0);
        assert_eq!(view.terrain.damage_density, 0.0);
    }
}
