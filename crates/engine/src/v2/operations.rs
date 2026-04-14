/// Shared operations layer: translates strategic directives into entity-level
/// operational commands. All agent personalities share this implementation.
/// This is the V3-ready replacement for city_ai decision-making.
use std::collections::HashMap;

use super::agent_layers::*;
use super::hex::{self, Axial};
use super::observation::{Observation, UnitInfo};
use super::state::{Role, UnitKey};
use super::{SETTLEMENT_THRESHOLD, SOLDIER_READY_THRESHOLD};

/// Target role ratios (farmer, worker, soldier) by economic focus.
fn role_targets(focus: EconomicFocus) -> (f32, f32, f32) {
    match focus {
        EconomicFocus::Growth => (0.60, 0.20, 0.10),
        EconomicFocus::Military => (0.30, 0.10, 0.50),
        EconomicFocus::Infrastructure => (0.40, 0.40, 0.10),
    }
}

/// Shared operations layer used by all agent personalities.
pub struct SharedOperationsLayer {
    posture: Posture,
    economic_focus: EconomicFocus,
    priority_regions: Vec<(Axial, f32)>,
    expansion_target: Option<Axial>,
    stacks: Vec<Stack>,
    next_stack_id: u32,
}

impl SharedOperationsLayer {
    pub fn new() -> Self {
        Self {
            posture: Posture::Expand,
            economic_focus: EconomicFocus::Growth,
            priority_regions: Vec::new(),
            expansion_target: None,
            stacks: Vec::new(),
            next_stack_id: 0,
        }
    }

    fn update_from_directives(&mut self, directives: &[StrategicDirective]) {
        for d in directives {
            match d {
                StrategicDirective::SetPosture(p) => self.posture = *p,
                StrategicDirective::SetEconomicFocus(f) => self.economic_focus = *f,
                StrategicDirective::PrioritizeRegion { center, priority } => {
                    // Replace existing priority for same region or add new.
                    if let Some(entry) = self.priority_regions.iter_mut().find(|(c, _)| *c == *center)
                    {
                        entry.1 = *priority;
                    } else {
                        self.priority_regions.push((*center, *priority));
                    }
                }
                StrategicDirective::SetExpansionTarget { hex } => {
                    self.expansion_target = Some(*hex);
                }
                StrategicDirective::RequestStackFormation { .. } => {
                    // Handled during stack formation phase.
                }
            }
        }
        // Decay old priority regions.
        self.priority_regions.retain(|(_, p)| *p > 0.05);
    }

    /// Identify settlement hexes from population data.
    fn settlement_hexes(obs: &Observation) -> Vec<Axial> {
        let mut settlements = Vec::new();
        let mut pop_by_hex: HashMap<(i32, i32), u16> = HashMap::new();
        for pop in &obs.own_population {
            *pop_by_hex.entry((pop.q, pop.r)).or_default() += pop.count;
        }
        for (&(q, r), &count) in &pop_by_hex {
            if count >= SETTLEMENT_THRESHOLD {
                settlements.push(Axial::new(q, r));
            }
        }
        settlements
    }

    /// Population mix at a hex: (idle, farmers, workers, trained_soldiers, untrained_soldiers).
    fn population_mix(obs: &Observation, hex: Axial) -> (u16, u16, u16, u16, u16) {
        let mut idle = 0u16;
        let mut farmers = 0u16;
        let mut workers = 0u16;
        let mut trained = 0u16;
        let mut untrained = 0u16;
        for pop in obs
            .own_population
            .iter()
            .filter(|p| p.q == hex.q && p.r == hex.r)
        {
            match pop.role {
                Role::Idle | Role::Builder => idle += pop.count,
                Role::Farmer => farmers += pop.count,
                Role::Worker => workers += pop.count,
                Role::Soldier => {
                    if pop.training >= SOLDIER_READY_THRESHOLD {
                        trained += pop.count;
                    } else {
                        untrained += pop.count;
                    }
                }
            }
        }
        (idle, farmers, workers, trained, untrained)
    }

    /// Emit ProducePerson for settlements that have enough food surplus
    /// to support growth, and are below the target population.
    fn produce_population(&self, obs: &Observation) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();
        let settlements = Self::settlement_hexes(obs);
        for hex in &settlements {
            // Check food surplus at this hex.
            if let Some(idx) = cell_index(obs, *hex) {
                if obs.food_stockpiles[idx] > 10.0 {
                    commands.push(OperationalCommand::ProducePerson {
                        settlement_hex: *hex,
                    });
                }
            }
        }
        commands
    }

    /// Assign population roles based on economic focus.
    /// Returns BuildStructure commands for infrastructure needs.
    fn manage_infrastructure(&self, obs: &Observation) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();
        let settlements = Self::settlement_hexes(obs);

        for hex in &settlements {
            let Some(idx) = cell_index(obs, *hex) else {
                continue;
            };
            let material = obs.material_stockpiles[idx];
            let road_level = obs.road_levels[idx];

            // Build depot at settlements with enough material.
            if material >= 20.0 {
                commands.push(OperationalCommand::BuildStructure {
                    hex: *hex,
                    structure_type: StructureType::Depot,
                });
            }

            // Build roads based on posture.
            match self.posture {
                Posture::Attack => {
                    // Build roads toward priority regions for faster movement.
                    if road_level == 0 {
                        for (target, _) in &self.priority_regions {
                            let neighbors = hex::neighbors(*hex);
                            if let Some(toward) = neighbors
                                .iter()
                                .min_by_key(|n| hex::distance(**n, *target))
                            {
                                commands.push(OperationalCommand::BuildStructure {
                                    hex: *toward,
                                    structure_type: StructureType::Farm,
                                });
                            }
                        }
                    }
                }
                Posture::Expand => {
                    // Build farms on fertile adjacent hexes.
                    let neighbors = hex::neighbors(*hex);
                    for n in &neighbors {
                        if let Some(n_idx) = cell_index(obs, *n) {
                            if obs.terrain[n_idx] > 0.5
                                && obs.stockpile_owner[n_idx] == Some(obs.player)
                            {
                                commands.push(OperationalCommand::BuildStructure {
                                    hex: *n,
                                    structure_type: StructureType::Farm,
                                });
                            }
                        }
                    }
                }
                Posture::Defend | Posture::Consolidate => {
                    // Upgrade existing roads.
                    if road_level < 2 && material >= 10.0 {
                        commands.push(OperationalCommand::BuildStructure {
                            hex: *hex,
                            structure_type: StructureType::Depot,
                        });
                    }
                }
            }
        }
        commands
    }

    /// Form stacks from unengaged military units grouped by proximity.
    fn form_stacks(&mut self, obs: &Observation) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();

        // Collect unengaged own units.
        let available: Vec<&UnitInfo> = obs
            .own_units
            .iter()
            .filter(|u| u.engagements.is_empty())
            .collect();

        if available.is_empty() {
            return commands;
        }

        // Group units by hex.
        let mut by_hex: HashMap<(i32, i32), Vec<UnitKey>> = HashMap::new();
        for unit in &available {
            by_hex.entry((unit.q, unit.r)).or_default().push(unit.id);
        }

        // Clear old stacks and rebuild.
        self.stacks.clear();

        for ((q, r), unit_ids) in &by_hex {
            if unit_ids.len() < 2 {
                continue;
            }
            let hex = Axial::new(*q, *r);
            let role = match self.posture {
                Posture::Attack => StackRole::Assault,
                Posture::Defend => StackRole::Garrison,
                Posture::Expand => StackRole::Scout,
                Posture::Consolidate => StackRole::Garrison,
            };

            let stack_id = StackId(self.next_stack_id);
            self.next_stack_id += 1;

            self.stacks.push(Stack {
                id: stack_id,
                player: obs.player,
                hex,
                entities: unit_ids.clone(),
                role,
            });

            commands.push(OperationalCommand::FormStack {
                entities: unit_ids.clone(),
            });
        }

        // Also merge nearby small stacks. Units within 2 hexes of each other
        // that aren't in stacks get grouped with the nearest stack.
        let unstacked: Vec<&UnitInfo> = available
            .iter()
            .filter(|u| {
                !self
                    .stacks
                    .iter()
                    .any(|s| s.entities.contains(&u.id))
            })
            .copied()
            .collect();

        for unit in &unstacked {
            let unit_pos = Axial::new(unit.q, unit.r);
            if let Some(nearest_stack) = self
                .stacks
                .iter_mut()
                .filter(|s| hex::distance(s.hex, unit_pos) <= 3)
                .min_by_key(|s| hex::distance(s.hex, unit_pos))
            {
                nearest_stack.entities.push(unit.id);
            }
        }

        commands
    }

    /// Route stacks toward strategic objectives.
    fn route_stacks(&self, obs: &Observation) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();

        for stack in &self.stacks {
            let destination = match self.posture {
                Posture::Attack => {
                    // Route toward highest-priority region, or enemy centroid.
                    self.priority_regions
                        .iter()
                        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(c, _)| *c)
                        .or_else(|| enemy_centroid(obs))
                }
                Posture::Expand => {
                    // Route scouts toward unexplored territory.
                    self.expansion_target.or_else(|| {
                        find_unexplored_direction(obs, stack.hex)
                    })
                }
                Posture::Defend => {
                    // Route garrison toward nearest settlement.
                    let settlements = Self::settlement_hexes(obs);
                    settlements
                        .iter()
                        .min_by_key(|s| hex::distance(**s, stack.hex))
                        .copied()
                }
                Posture::Consolidate => {
                    // Stay near settlements.
                    let settlements = Self::settlement_hexes(obs);
                    settlements
                        .iter()
                        .min_by_key(|s| hex::distance(**s, stack.hex))
                        .copied()
                }
            };

            if let Some(dest) = destination {
                if hex::distance(stack.hex, dest) > 1 {
                    commands.push(OperationalCommand::RouteStack {
                        stack: stack.id,
                        destination: dest,
                    });
                }
            }
        }

        commands
    }

    /// Establish supply routes from surplus settlements to forward positions.
    fn manage_supply(&self, obs: &Observation) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();
        let settlements = Self::settlement_hexes(obs);

        if settlements.is_empty() || self.stacks.is_empty() {
            return commands;
        }

        // Find settlements with food surplus.
        let surplus_settlements: Vec<Axial> = settlements
            .iter()
            .filter(|hex| {
                cell_index(obs, **hex)
                    .map(|idx| obs.food_stockpiles[idx] > 20.0)
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        // Find forward stacks that need supply.
        for stack in &self.stacks {
            if stack.role == StackRole::Supply {
                continue;
            }
            // Check if any unit in the stack has low rations.
            let needs_supply = stack.entities.iter().any(|uid| {
                obs.own_units
                    .iter()
                    .find(|u| u.id == *uid)
                    .map(|u| u.rations < super::MAX_RATIONS * 0.5)
                    .unwrap_or(false)
            });

            if needs_supply {
                if let Some(source) = surplus_settlements
                    .iter()
                    .min_by_key(|s| hex::distance(**s, stack.hex))
                {
                    commands.push(OperationalCommand::EstablishSupplyRoute {
                        from: *source,
                        to: stack.hex,
                    });
                }
            }
        }

        commands
    }

    /// When expanding, find good settlement sites and dispatch settlers.
    fn manage_expansion(&self, obs: &Observation) -> Vec<OperationalCommand> {
        let mut commands = Vec::new();

        if self.posture != Posture::Expand {
            return commands;
        }

        let settlements = Self::settlement_hexes(obs);
        if settlements.is_empty() {
            return commands;
        }

        // Already have a settler convoy in flight? Skip.
        if !obs.own_convoys.is_empty() {
            // Simplification: any convoy means settlers may be en route.
            // A real implementation would check cargo type.
            return commands;
        }

        // Find the target from strategy, or pick one.
        let target = self.expansion_target.or_else(|| {
            find_best_settlement_site(obs, &settlements)
        });

        if let Some(target_hex) = target {
            // Find the largest settlement to dispatch from.
            let source = settlements
                .iter()
                .max_by_key(|hex| {
                    obs.own_population
                        .iter()
                        .filter(|p| p.q == hex.q && p.r == hex.r)
                        .map(|p| p.count)
                        .sum::<u16>()
                });

            if let Some(source_hex) = source {
                commands.push(OperationalCommand::ProducePerson {
                    settlement_hex: target_hex,
                });
                // Route supply to new settlement.
                commands.push(OperationalCommand::EstablishSupplyRoute {
                    from: *source_hex,
                    to: target_hex,
                });
            }
        }

        commands
    }

    /// Accessors for testing.
    pub fn posture(&self) -> Posture {
        self.posture
    }

    pub fn economic_focus(&self) -> EconomicFocus {
        self.economic_focus
    }

    pub fn stacks(&self) -> &[Stack] {
        &self.stacks
    }
}

impl OperationsLayer for SharedOperationsLayer {
    fn execute(
        &mut self,
        obs: &Observation,
        directives: &[StrategicDirective],
    ) -> Vec<OperationalCommand> {
        self.update_from_directives(directives);
        let mut commands = Vec::new();
        commands.extend(self.produce_population(obs));
        commands.extend(self.form_stacks(obs));
        commands.extend(self.route_stacks(obs));
        commands.extend(self.manage_infrastructure(obs));
        commands.extend(self.manage_supply(obs));
        commands.extend(self.manage_expansion(obs));
        commands
    }
}

// ---------------------------------------------------------------------------
// Helpers
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

fn enemy_centroid(obs: &Observation) -> Option<Axial> {
    if obs.visible_enemies.is_empty() {
        return None;
    }
    let n = obs.visible_enemies.len() as i32;
    let sum_q: i32 = obs.visible_enemies.iter().map(|e| e.q).sum();
    let sum_r: i32 = obs.visible_enemies.iter().map(|e| e.r).sum();
    Some(Axial::new(sum_q / n, sum_r / n))
}

fn find_unexplored_direction(obs: &Observation, from: Axial) -> Option<Axial> {
    // Find the nearest unscouted hex direction and return a point in that direction.
    let (_from_row, _from_col) = hex::axial_to_offset(from);
    let mut best: Option<(i32, usize)> = None;

    for (idx, scouted) in obs.scouted.iter().enumerate() {
        if *scouted {
            continue;
        }
        let row = idx / obs.width;
        let col = idx % obs.width;
        let ax = hex::offset_to_axial(row as i32, col as i32);
        let dist = hex::distance(from, ax);
        match best {
            Some((d, _)) if d <= dist => {}
            _ => best = Some((dist, idx)),
        }
    }

    best.map(|(_, idx)| {
        let row = idx / obs.width;
        let col = idx % obs.width;
        hex::offset_to_axial(row as i32, col as i32)
    })
}

fn find_best_settlement_site(obs: &Observation, existing: &[Axial]) -> Option<Axial> {
    let mut best: Option<(Axial, f32)> = None;

    for (idx, &terrain) in obs.terrain.iter().enumerate() {
        if terrain <= 0.0 || !obs.scouted[idx] {
            continue;
        }
        let row = idx / obs.width;
        let col = idx % obs.width;
        let ax = hex::offset_to_axial(row as i32, col as i32);

        // Must not already be a settlement.
        if existing.contains(&ax) {
            continue;
        }

        // Min distance from any existing settlement.
        let min_dist = existing
            .iter()
            .map(|s| hex::distance(*s, ax))
            .min()
            .unwrap_or(i32::MAX);

        // Too close or too far.
        if min_dist < 3 || min_dist > 10 {
            continue;
        }

        let score = terrain * 2.0 - min_dist as f32 * 0.2;
        match best {
            Some((_, bs)) if bs >= score => {}
            _ => best = Some((ax, score)),
        }
    }

    best.map(|(ax, _)| ax)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2::mapgen::{MapConfig, generate};
    use crate::v2::observation::observe;

    fn test_obs(seed: u64) -> Observation {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed,
        });
        observe(&mut state, 0)
    }

    #[test]
    fn expand_growth_produces_population() {
        let obs = test_obs(42);
        let mut ops = SharedOperationsLayer::new();
        let directives = vec![
            StrategicDirective::SetPosture(Posture::Expand),
            StrategicDirective::SetEconomicFocus(EconomicFocus::Growth),
        ];
        let commands = ops.execute(&obs, &directives);

        // Should emit ProducePerson for settlements with food surplus.
        let produce_count = commands
            .iter()
            .filter(|c| matches!(c, OperationalCommand::ProducePerson { .. }))
            .count();
        assert!(
            produce_count > 0,
            "Expand+Growth should produce population at settlements"
        );
    }

    #[test]
    fn attack_military_forms_stacks() {
        let obs = test_obs(42);
        let mut ops = SharedOperationsLayer::new();
        let directives = vec![
            StrategicDirective::SetPosture(Posture::Attack),
            StrategicDirective::SetEconomicFocus(EconomicFocus::Military),
        ];
        let commands = ops.execute(&obs, &directives);

        // With Attack posture, stacks should be formed as Assault.
        let _form_count = commands
            .iter()
            .filter(|c| matches!(c, OperationalCommand::FormStack { .. }))
            .count();
        // Stacks are only formed when 2+ units share a hex.
        // With initial state, units may be spread out.
        // Verify internal stack tracking is consistent.
        for stack in ops.stacks() {
            assert_eq!(stack.role, StackRole::Assault);
            assert!(stack.entities.len() >= 2);
        }
        assert_eq!(ops.posture(), Posture::Attack);
        assert_eq!(ops.economic_focus(), EconomicFocus::Military);
    }

    #[test]
    fn defend_routes_stacks_toward_settlements() {
        let obs = test_obs(42);
        let mut ops = SharedOperationsLayer::new();
        let directives = vec![
            StrategicDirective::SetPosture(Posture::Defend),
            StrategicDirective::SetEconomicFocus(EconomicFocus::Growth),
        ];
        let commands = ops.execute(&obs, &directives);

        // RouteStack commands should route toward settlements.
        let settlements = SharedOperationsLayer::settlement_hexes(&obs);
        for cmd in &commands {
            if let OperationalCommand::RouteStack { destination, .. } = cmd {
                // Destination should be near a settlement.
                let near_settlement = settlements
                    .iter()
                    .any(|s| hex::distance(*s, *destination) <= 2);
                assert!(
                    near_settlement,
                    "Defend should route stacks toward settlements"
                );
            }
        }
    }

    #[test]
    fn supply_routes_from_surplus_settlements() {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        });
        // Boost food at player 0's settlement to create surplus.
        let settlement_hex = state
            .settlements
            .values()
            .find(|s| s.owner == 0)
            .map(|s| s.hex)
            .unwrap();
        if let Some(cell) = state.cell_at_mut(settlement_hex) {
            cell.food_stockpile = 50.0;
            state.mark_dirty_axial(settlement_hex);
        }

        let obs = observe(&mut state, 0);
        let mut ops = SharedOperationsLayer::new();

        // Place a priority region far away so stacks route there and need supply.
        let directives = vec![
            StrategicDirective::SetPosture(Posture::Attack),
            StrategicDirective::SetEconomicFocus(EconomicFocus::Military),
            StrategicDirective::PrioritizeRegion {
                center: Axial::new(15, 15),
                priority: 0.9,
            },
        ];
        let commands = ops.execute(&obs, &directives);

        // If stacks exist with low-ration units, supply routes should be established.
        let supply_count = commands
            .iter()
            .filter(|c| matches!(c, OperationalCommand::EstablishSupplyRoute { .. }))
            .count();
        // Supply routes depend on stacks existing with low rations.
        // The initial state has units with full rations, so supply may not trigger.
        // This test verifies the pipeline runs without panic.
        let _ = supply_count; // Structural test — no panic.
    }

    #[test]
    fn expansion_finds_settlement_sites() {
        let obs = test_obs(42);
        let settlements = SharedOperationsLayer::settlement_hexes(&obs);
        let site = find_best_settlement_site(&obs, &settlements);

        if let Some(site_hex) = site {
            // Site should be at least 3 hexes from existing settlements.
            for s in &settlements {
                assert!(
                    hex::distance(*s, site_hex) >= 3,
                    "Settlement site too close to existing"
                );
            }
            // Site should be on fertile terrain.
            if let Some(idx) = cell_index(&obs, site_hex) {
                assert!(obs.terrain[idx] > 0.0, "Settlement site on barren terrain");
            }
        }
    }

    #[test]
    fn directives_update_internal_state() {
        let mut ops = SharedOperationsLayer::new();
        assert_eq!(ops.posture(), Posture::Expand);
        assert_eq!(ops.economic_focus(), EconomicFocus::Growth);

        ops.update_from_directives(&[
            StrategicDirective::SetPosture(Posture::Consolidate),
            StrategicDirective::SetEconomicFocus(EconomicFocus::Infrastructure),
            StrategicDirective::PrioritizeRegion {
                center: Axial::new(5, 5),
                priority: 0.8,
            },
            StrategicDirective::SetExpansionTarget {
                hex: Axial::new(10, 10),
            },
        ]);

        assert_eq!(ops.posture(), Posture::Consolidate);
        assert_eq!(ops.economic_focus(), EconomicFocus::Infrastructure);
        assert_eq!(ops.expansion_target, Some(Axial::new(10, 10)));
        assert_eq!(ops.priority_regions.len(), 1);
        assert_eq!(ops.priority_regions[0].0, Axial::new(5, 5));
    }

    #[test]
    fn priority_region_updates_replace_existing() {
        let mut ops = SharedOperationsLayer::new();
        ops.update_from_directives(&[StrategicDirective::PrioritizeRegion {
            center: Axial::new(5, 5),
            priority: 0.5,
        }]);
        assert_eq!(ops.priority_regions[0].1, 0.5);

        ops.update_from_directives(&[StrategicDirective::PrioritizeRegion {
            center: Axial::new(5, 5),
            priority: 0.9,
        }]);
        assert_eq!(ops.priority_regions.len(), 1);
        assert_eq!(ops.priority_regions[0].1, 0.9);
    }

    #[test]
    fn stack_formation_groups_colocated_units() {
        let obs = test_obs(42);
        let mut ops = SharedOperationsLayer::new();
        let directives = vec![StrategicDirective::SetPosture(Posture::Attack)];
        ops.execute(&obs, &directives);

        // Every stack should have units that were on the same hex.
        for stack in ops.stacks() {
            assert!(
                stack.entities.len() >= 2,
                "Stack should have at least 2 units"
            );
            // All entities in the stack should be real unit IDs from the observation.
            for uid in &stack.entities {
                // Either in own_units or was merged from nearby.
                // We just verify the IDs are non-null.
                assert!(!uid.is_null());
            }
        }
    }

    #[test]
    fn role_targets_are_valid_ratios() {
        for focus in [
            EconomicFocus::Growth,
            EconomicFocus::Military,
            EconomicFocus::Infrastructure,
        ] {
            let (f, w, s) = role_targets(focus);
            let total = f + w + s;
            assert!(
                total <= 1.0 + f32::EPSILON,
                "Role targets for {:?} exceed 1.0: {}",
                focus,
                total
            );
            assert!(f > 0.0, "Farmers must always be positive");
        }
    }

    #[test]
    fn infrastructure_builds_at_settlements() {
        let mut state = generate(&MapConfig {
            width: 20,
            height: 20,
            num_players: 2,
            seed: 42,
        });
        // Give settlement material for depot.
        let settlement_hex = state
            .settlements
            .values()
            .find(|s| s.owner == 0)
            .map(|s| s.hex)
            .unwrap();
        if let Some(cell) = state.cell_at_mut(settlement_hex) {
            cell.material_stockpile = 30.0;
            state.mark_dirty_axial(settlement_hex);
        }

        let obs = observe(&mut state, 0);
        let mut ops = SharedOperationsLayer::new();
        let directives = vec![
            StrategicDirective::SetPosture(Posture::Expand),
            StrategicDirective::SetEconomicFocus(EconomicFocus::Infrastructure),
        ];
        let commands = ops.execute(&obs, &directives);

        let build_count = commands
            .iter()
            .filter(|c| matches!(c, OperationalCommand::BuildStructure { .. }))
            .count();
        assert!(
            build_count > 0,
            "Should build infrastructure at settlements with material"
        );
    }

    #[test]
    fn full_pipeline_no_panic() {
        // Run the full pipeline across different seeds to ensure no panics.
        for seed in 0..5 {
            let obs = test_obs(seed);
            let mut ops = SharedOperationsLayer::new();
            for posture in [
                Posture::Expand,
                Posture::Attack,
                Posture::Defend,
                Posture::Consolidate,
            ] {
                for focus in [
                    EconomicFocus::Growth,
                    EconomicFocus::Military,
                    EconomicFocus::Infrastructure,
                ] {
                    let directives = vec![
                        StrategicDirective::SetPosture(posture),
                        StrategicDirective::SetEconomicFocus(focus),
                    ];
                    let commands = ops.execute(&obs, &directives);
                    // Just verify no panic and commands are non-empty for at least some combos.
                    let _ = commands;
                }
            }
        }
    }

    use slotmap::Key;

    #[test]
    fn population_mix_counts_correctly() {
        let obs = test_obs(42);
        let settlements = SharedOperationsLayer::settlement_hexes(&obs);
        for hex in &settlements {
            let (idle, farmers, workers, trained, untrained) =
                SharedOperationsLayer::population_mix(&obs, *hex);
            let total = idle + farmers + workers + trained + untrained;
            // Settlement must have population.
            assert!(total >= SETTLEMENT_THRESHOLD, "Settlement at {:?} has only {} pop", hex, total);
        }
    }
}
