use super::formation::FormationType;
use super::perception::StrategicView;
use super::spatial::Vec3;
use super::state::{GameState, StackId};
use crate::v2::hex::Axial;
use crate::v2::state::EntityKey;
/// V3 agent architecture: three-layer dispatch (Strategy, Operations, Tactical).
///
/// Strategy runs every ~50 game-seconds, Operations every ~5, Tactical every
/// tick for stacks near enemies. Personality differentiates only at the Strategy
/// layer. Operations and Tactical are shared implementations used by all agents.
use serde::{Deserialize, Serialize};

pub use super::commands::{
    CommandApplySummary, CommandStatus, apply_agent_output, apply_operational_command,
    apply_tactical_command, validate_operational_command as validate_operational,
    validate_tactical_command as validate_tactical,
};

// ---------------------------------------------------------------------------
// Enums shared across layers
// ---------------------------------------------------------------------------

/// Grand strategic posture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Posture {
    Expand,
    Consolidate,
    Attack,
    Defend,
}

/// How surplus resources are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EconomicFocus {
    Growth,
    Military,
    Infrastructure,
}

/// Abstract stack template requested by Strategy. Operations translates
/// these into concrete equipment loadouts using the damage estimate table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StackArchetype {
    HeavyInfantry,
    LightInfantry,
    Skirmisher,
    Cavalry,
    Garrison,
    Settler,
}

/// Equipment category for production commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EquipmentType {
    Sword,
    Spear,
    Axe,
    Mace,
    Bow,
    Shield,
    HelmetPlate,
    HelmetChain,
    CuirassPlate,
    CuirassChain,
    CuirassPadded,
    Greaves,
}

// ---------------------------------------------------------------------------
// Layer commands
// ---------------------------------------------------------------------------

/// High-level intent from the strategy layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StrategicDirective {
    SetPosture(Posture),
    SetEconomicFocus(EconomicFocus),
    PrioritizeRegion {
        center: Axial,
        priority: f32,
    },
    RequestStack {
        archetype: StackArchetype,
        region: Axial,
    },
    SetExpansionTarget {
        hex: Axial,
    },
}

/// Concrete entity-level orders from the operations layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationalCommand {
    FormStack {
        entities: Vec<EntityKey>,
        archetype: StackArchetype,
    },
    RouteStack {
        stack: StackId,
        waypoints: Vec<Vec3>,
    },
    DisbandStack {
        stack: StackId,
    },
    ProduceEquipment {
        workshop: EntityKey,
        item_type: EquipmentType,
    },
    EquipEntity {
        entity: EntityKey,
        equipment: EntityKey,
    },
    EstablishSupplyRoute {
        from: Axial,
        to: Axial,
    },
    FoundSettlement {
        entity: EntityKey,
        target: Axial,
    },
}

/// Per-tick combat orders for entities near enemies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TacticalCommand {
    Attack {
        attacker: EntityKey,
        target: EntityKey,
    },
    SetFacing {
        entity: EntityKey,
        angle: f32,
    },
    Block {
        entity: EntityKey,
    },
    Retreat {
        entity: EntityKey,
        toward: Vec3,
    },
    Hold {
        entity: EntityKey,
    },
    SetFormation {
        stack: StackId,
        formation: FormationType,
    },
}

// ---------------------------------------------------------------------------
// Agent traces — structured decision log for review bundles
// ---------------------------------------------------------------------------

/// Structured trace of an agent layer decision. Queryable by field for
/// pattern detection and debugging. All variants implement Debug/Display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentTrace {
    Policy {
        player: u8,
        summary: String,
    },
    MethodInjection {
        player: u8,
        summary: String,
    },
    Coordination {
        stack: u32,
        hotspot: String,
        summary: String,
    },
    Behavior {
        entity: Option<u32>,
        summary: String,
    },
}

impl std::fmt::Display for AgentTrace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentTrace::Policy { player, summary } => {
                write!(f, "Policy P{}: {}", player, summary)
            }
            AgentTrace::MethodInjection { player, summary } => {
                write!(f, "Operations P{}: {}", player, summary)
            }
            AgentTrace::Coordination {
                stack,
                hotspot,
                summary,
            } => {
                write!(f, "Coordination stack {} @ {}: {}", stack, hotspot, summary)
            }
            AgentTrace::Behavior { entity, summary } => {
                write!(f, "Behavior {:?}: {}", entity, summary)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Layer traits
// ---------------------------------------------------------------------------

/// Runs every ~50 game-seconds. Reads a StrategicView (fog-of-war abstraction),
/// emits directives. Personality-specific implementations.
pub trait StrategyLayer: Send {
    fn plan(&mut self, view: &StrategicView) -> Vec<StrategicDirective>;
}

/// Runs every ~5 game-seconds. Translates strategic directives into stack,
/// equipment, and behavior-availability changes. Shared implementation used by
/// all agents.
pub trait OperationsLayer: Send {
    fn execute(
        &mut self,
        state: &GameState,
        directives: &[StrategicDirective],
        player: u8,
    ) -> Vec<OperationalCommand>;
}

/// Runs every tick for stacks within engagement range of enemies.
/// Shared implementation used by all agents.
pub trait TacticalLayer: Send {
    fn decide(
        &mut self,
        state: &GameState,
        stack: &super::state::Stack,
        player: u8,
    ) -> Vec<TacticalCommand>;
}

// ---------------------------------------------------------------------------
// LayeredAgent
// ---------------------------------------------------------------------------

/// Hotspot threshold above which tactical coordination should run.
const RESOLUTION_DEMAND_THRESHOLD: f32 = 0.24;

/// An agent composed of three layers operating at different cadences.
pub struct LayeredAgent {
    pub strategy: Box<dyn StrategyLayer>,
    pub operations: Box<dyn OperationsLayer>,
    pub tactical: Box<dyn TacticalLayer>,
    pub active_directives: Vec<StrategicDirective>,
    pub player: u8,
    /// Ticks between strategy invocations.
    pub strategy_cadence: u64,
    /// Ticks between operations invocations.
    pub operations_cadence: u64,
}

impl LayeredAgent {
    pub fn new(
        strategy: Box<dyn StrategyLayer>,
        operations: Box<dyn OperationsLayer>,
        tactical: Box<dyn TacticalLayer>,
        player: u8,
        strategy_cadence: u64,
        operations_cadence: u64,
    ) -> Self {
        Self {
            strategy,
            operations,
            tactical,
            active_directives: Vec::new(),
            player,
            strategy_cadence,
            operations_cadence,
        }
    }

    /// Run the agent for one tick. Returns all commands to be validated and executed.
    pub fn tick(&mut self, state: &GameState) -> AgentOutput {
        let tick = state.tick;
        let mut output = AgentOutput {
            player: self.player,
            ..AgentOutput::default()
        };

        // Strategy layer — runs at strategy cadence.
        if tick.is_multiple_of(self.strategy_cadence) {
            let view = super::perception::build_strategic_view(state, self.player);
            self.active_directives = self.strategy.plan(&view);
            output.directives = self.active_directives.clone();
            output.strategy_ran = true;

            // Emit strategy trace.
            output.traces.push(AgentTrace::Policy {
                player: self.player,
                summary: format!(
                    "tick {} policy refresh: {} directives",
                    tick,
                    self.active_directives.len()
                ),
            });
        }

        // Operations layer — runs at operations cadence.
        if tick.is_multiple_of(self.operations_cadence) {
            let commands = self
                .operations
                .execute(state, &self.active_directives, self.player);

            // Emit operations traces.
            for cmd in &commands {
                output.traces.push(AgentTrace::MethodInjection {
                    player: self.player,
                    summary: match cmd {
                        OperationalCommand::FormStack {
                            entities,
                            archetype,
                        } => format!("form stack {:?} with {} members", archetype, entities.len()),
                        OperationalCommand::RouteStack { stack, waypoints } => {
                            format!(
                                "route stack {:?} across {} waypoints",
                                stack,
                                waypoints.len()
                            )
                        }
                        OperationalCommand::DisbandStack { stack } => {
                            format!("disband stack {:?}", stack)
                        }
                        OperationalCommand::ProduceEquipment {
                            workshop,
                            item_type,
                        } => format!("queue {:?} production at {:?}", item_type, workshop),
                        OperationalCommand::EquipEntity { entity, equipment } => {
                            format!("equip {:?} with {:?}", entity, equipment)
                        }
                        OperationalCommand::EstablishSupplyRoute { from, to } => {
                            format!("inject supply route {:?}->{:?}", from, to)
                        }
                        OperationalCommand::FoundSettlement { entity, target } => {
                            format!("enable settlement founding by {:?} at {:?}", entity, target)
                        }
                    },
                });
            }

            output.operational_commands = commands;
            output.operations_ran = true;
        }

        // Tactical layer — runs for each stack near enemies.
        for stack in &state.stacks {
            if stack.owner != self.player {
                continue;
            }
            let hotspot = stack_hotspot(state, stack);
            if hotspot >= RESOLUTION_DEMAND_THRESHOLD {
                let commands = self.tactical.decide(state, stack, self.player);

                // Emit tactical traces.
                for cmd in &commands {
                    let summary = match cmd {
                        TacticalCommand::Attack { .. } => "engage target".to_string(),
                        TacticalCommand::Retreat { .. } => "retreat from hotspot".to_string(),
                        TacticalCommand::Hold { .. } => "hold line".to_string(),
                        TacticalCommand::Block { .. } => "block threat".to_string(),
                        TacticalCommand::SetFacing { .. } => "adjust facing".to_string(),
                        TacticalCommand::SetFormation { .. } => "set formation".to_string(),
                    };
                    output.traces.push(AgentTrace::Coordination {
                        stack: stack.id.0,
                        hotspot: format!("{:.2}", hotspot),
                        summary,
                    });
                }

                output.tactical_commands.extend(commands);
                output.tactical_stacks += 1;
            }
        }

        output
    }
}

/// Output of a single agent tick.
#[derive(Debug, Default)]
pub struct AgentOutput {
    pub player: u8,
    pub strategy_ran: bool,
    pub operations_ran: bool,
    pub tactical_stacks: usize,
    pub directives: Vec<StrategicDirective>,
    pub operational_commands: Vec<OperationalCommand>,
    pub tactical_commands: Vec<TacticalCommand>,
    pub traces: Vec<AgentTrace>,
}

// ---------------------------------------------------------------------------
// Engagement detection
// ---------------------------------------------------------------------------

fn stack_hotspot(state: &GameState, stack: &super::state::Stack) -> f32 {
    stack
        .members
        .iter()
        .filter_map(|member_key| {
            state
                .entities
                .get(*member_key)
                .and_then(|entity| entity.hex)
        })
        .map(|hex| super::resolution::resolution_demand_at(state, hex))
        .fold(0.0, f32::max)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::movement::Mobile;
    use super::super::spatial::GeoMaterial;
    use super::super::spatial::{Heightfield, Vec3};
    use super::super::state::{Combatant, EntityBuilder, GameState, Person, Role, Stack};
    use super::*;
    use smallvec::SmallVec;

    fn test_state() -> GameState {
        let hf = Heightfield::new(10, 10, 0.0, GeoMaterial::Soil);
        GameState::new(10, 10, 2, hf)
    }

    /// Stub strategy that always returns a fixed set of directives.
    struct StubStrategy {
        directives: Vec<StrategicDirective>,
    }
    impl StrategyLayer for StubStrategy {
        fn plan(&mut self, _view: &StrategicView) -> Vec<StrategicDirective> {
            self.directives.clone()
        }
    }

    /// Stub operations that returns no commands.
    struct StubOperations;
    impl OperationsLayer for StubOperations {
        fn execute(
            &mut self,
            _state: &GameState,
            _directives: &[StrategicDirective],
            _player: u8,
        ) -> Vec<OperationalCommand> {
            Vec::new()
        }
    }

    /// Stub tactical that returns one Hold per member.
    struct StubTactical;
    impl TacticalLayer for StubTactical {
        fn decide(
            &mut self,
            _state: &GameState,
            stack: &Stack,
            _player: u8,
        ) -> Vec<TacticalCommand> {
            stack
                .members
                .iter()
                .map(|&e| TacticalCommand::Hold { entity: e })
                .collect()
        }
    }

    fn make_agent(player: u8) -> LayeredAgent {
        LayeredAgent::new(
            Box::new(StubStrategy {
                directives: vec![StrategicDirective::SetPosture(Posture::Attack)],
            }),
            Box::new(StubOperations),
            Box::new(StubTactical),
            player,
            50, // strategy every 50 ticks
            5,  // operations every 5 ticks
        )
    }

    fn spawn_person(state: &mut GameState, pos: Vec3, owner: u8) -> EntityKey {
        use super::super::lifecycle::spawn_entity;
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
                .combatant(Combatant::new()),
        )
    }

    #[test]
    fn cadence_strategy_runs_at_tick_zero() {
        let state = test_state();
        let mut agent = make_agent(0);
        let output = agent.tick(&state);
        assert!(output.strategy_ran, "strategy should run at tick 0");
        assert!(output.operations_ran, "operations should run at tick 0");
    }

    #[test]
    fn cadence_strategy_skips_intermediate_ticks() {
        let mut state = test_state();
        let mut agent = make_agent(0);

        // Tick 0 — both run.
        let _ = agent.tick(&state);

        // Tick 1 — neither should run.
        state.tick = 1;
        let output = agent.tick(&state);
        assert!(!output.strategy_ran);
        assert!(!output.operations_ran);
    }

    #[test]
    fn cadence_operations_runs_every_5() {
        let mut state = test_state();
        let mut agent = make_agent(0);

        for t in 0..=10 {
            state.tick = t;
            let output = agent.tick(&state);
            if t % 5 == 0 {
                assert!(output.operations_ran, "operations should run at tick {t}");
            } else {
                assert!(
                    !output.operations_ran,
                    "operations should NOT run at tick {t}"
                );
            }
        }
    }

    #[test]
    fn cadence_strategy_runs_every_50() {
        let mut state = test_state();
        let mut agent = make_agent(0);

        for t in [0, 25, 49, 50, 51, 100] {
            state.tick = t;
            let output = agent.tick(&state);
            if t % 50 == 0 {
                assert!(output.strategy_ran, "strategy should run at tick {t}");
            } else {
                assert!(!output.strategy_ran, "strategy should NOT run at tick {t}");
            }
        }
    }

    #[test]
    fn tactical_runs_for_stacks_near_enemies() {
        // Use a larger map so positions are within hex bounds.
        let hf = Heightfield::new(30, 30, 0.0, GeoMaterial::Soil);
        let mut state = GameState::new(30, 30, 2, hf);
        state.tick = 1; // not a strategy/ops tick

        // Spawn friendly and enemy entities close together (<300m).
        let friendly = spawn_person(&mut state, Vec3::new(100.0, 100.0, 0.0), 0);
        let _enemy = spawn_person(&mut state, Vec3::new(130.0, 100.0, 0.0), 1);
        super::super::sim::tick(&mut state, 0.0);
        state.tick = 1;

        // Create a stack for the friendly entity.
        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members: SmallVec::from_slice(&[friendly]),
            formation: FormationType::Line,
            leader: friendly,
        });

        let mut agent = make_agent(0);
        let output = agent.tick(&state);
        assert_eq!(output.tactical_stacks, 1, "should run tactical for 1 stack");
        assert!(
            !output.tactical_commands.is_empty(),
            "should emit tactical commands"
        );
    }

    #[test]
    fn tactical_skips_distant_stacks() {
        let hf = Heightfield::new(30, 30, 0.0, GeoMaterial::Soil);
        let mut state = GameState::new(30, 30, 2, hf);
        state.tick = 1;

        // Spawn entities far apart (>300m).
        let friendly = spawn_person(&mut state, Vec3::new(100.0, 100.0, 0.0), 0);
        let _enemy = spawn_person(&mut state, Vec3::new(600.0, 600.0, 0.0), 1);
        super::super::sim::tick(&mut state, 0.0);
        state.tick = 1;

        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 0,
            members: SmallVec::from_slice(&[friendly]),
            formation: FormationType::Line,
            leader: friendly,
        });

        let mut agent = make_agent(0);
        let output = agent.tick(&state);
        assert_eq!(
            output.tactical_stacks, 0,
            "should not run tactical for distant stack"
        );
    }

    #[test]
    fn tactical_ignores_other_players_stacks() {
        let hf = Heightfield::new(30, 30, 0.0, GeoMaterial::Soil);
        let mut state = GameState::new(30, 30, 2, hf);
        state.tick = 1;

        let _p0 = spawn_person(&mut state, Vec3::new(100.0, 100.0, 0.0), 0);
        let p1 = spawn_person(&mut state, Vec3::new(200.0, 100.0, 0.0), 1);

        // Stack belongs to player 1.
        let stack_id = state.alloc_stack_id();
        state.stacks.push(Stack {
            id: stack_id,
            owner: 1,
            members: SmallVec::from_slice(&[p1]),
            formation: FormationType::Line,
            leader: p1,
        });

        let mut agent = make_agent(0); // agent for player 0
        let output = agent.tick(&state);
        assert_eq!(output.tactical_stacks, 0);
    }

    #[test]
    fn strategy_updates_directives() {
        let state = test_state();
        let mut agent = make_agent(0);

        // Tick 0: strategy runs, sets directives.
        let _ = agent.tick(&state);
        assert!(!agent.active_directives.is_empty());
        assert!(matches!(
            agent.active_directives[0],
            StrategicDirective::SetPosture(Posture::Attack)
        ));
    }
}
