use super::hex::Axial;
use super::observation::Observation;
use super::state::{Role, UnitKey};
use serde::{Deserialize, Serialize};

/// Grand strategic posture: what the agent is trying to accomplish this phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Posture {
    Expand,
    Defend,
    Attack,
    Consolidate,
}

/// How the agent allocates surplus resources between growth and military.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EconomicFocus {
    Growth,
    Military,
    Infrastructure,
}

/// What a stack is assigned to do at the operational level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StackRole {
    Assault,
    Garrison,
    Scout,
    Supply,
}

/// Stable identifier for a stack across ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StackId(pub u32);

/// Bookkeeping for a group of entities operating together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stack {
    pub id: StackId,
    pub player: u8,
    pub hex: Axial,
    pub entities: Vec<UnitKey>,
    pub role: StackRole,
}

/// Physical structure that can be built on a hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StructureType {
    Farm,
    Village,
    City,
    Depot,
}

/// High-level intent from the strategy layer to guide operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StrategicDirective {
    SetPosture(Posture),
    PrioritizeRegion { center: Axial, priority: f32 },
    SetEconomicFocus(EconomicFocus),
    RequestStackFormation { size: usize, role: StackRole },
    SetExpansionTarget { hex: Axial },
}

/// Concrete entity-level orders from the operations layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationalCommand {
    AssignRole {
        entity: UnitKey,
        role: Role,
    },
    FormStack {
        entities: Vec<UnitKey>,
    },
    RouteStack {
        stack: StackId,
        destination: Axial,
    },
    DisbandStack {
        stack: StackId,
    },
    BuildStructure {
        hex: Axial,
        structure_type: StructureType,
    },
    EstablishSupplyRoute {
        from: Axial,
        to: Axial,
    },
    ProducePerson {
        settlement_hex: Axial,
    },
}

/// Per-tick orders for individual entities in contact with enemies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TacticalCommand {
    Engage { attacker: UnitKey, target: UnitKey },
    Disengage { entity: UnitKey },
    SetFacing { entity: UnitKey, angle: f32 },
    Retreat { entity: UnitKey, toward: Axial },
    Hold { entity: UnitKey },
}

/// Minimal observation of a single hex and its immediate occupants, used by the tactical layer.
#[derive(Debug, Clone)]
pub struct LocalObservation {
    pub hex: Axial,
    pub own_entities: Vec<UnitKey>,
    pub enemy_entities: Vec<UnitKey>,
}

/// Runs every ~50 ticks to set grand strategy for the session.
pub trait StrategyLayer: Send {
    fn plan(&mut self, obs: &Observation) -> Vec<StrategicDirective>;
}

/// Translates strategic directives into entity-level orders every ~5 ticks.
pub trait OperationsLayer: Send {
    fn execute(
        &mut self,
        obs: &Observation,
        directives: &[StrategicDirective],
    ) -> Vec<OperationalCommand>;
}

/// Issues per-tick combat decisions for stacks in contact with enemies.
pub trait TacticalLayer: Send {
    fn decide(&mut self, local_obs: &LocalObservation) -> Vec<TacticalCommand>;
}

/// An agent composed of three layers operating at different cadences.
/// Strategy runs every ~50 ticks, Operations every ~5 ticks,
/// Tactical every tick for stacks near enemies.
pub struct LayeredAgent {
    pub strategy: Box<dyn StrategyLayer>,
    pub operations: Box<dyn OperationsLayer>,
    pub tactical: Box<dyn TacticalLayer>,
    /// Cached strategic directives from last strategy run.
    pub active_directives: Vec<StrategicDirective>,
}
