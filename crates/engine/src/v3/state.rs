use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use smallvec::SmallVec;

use super::armor::{ArmorProperties, MaterialType};
use super::combat_log::CombatLog;
use super::equipment::Equipment;
use super::formation::FormationType;
use super::index::SpatialIndex;
use super::movement::Mobile;
use super::projectile::Projectile;
use super::spatial::{Heightfield, Vec3};
use super::vitals::Vitals;
use super::weapon::{AttackState, CooldownState, WeaponProperties};
use super::wound::WoundList;
use crate::v2::hex::Axial;
use crate::v2::state::EntityKey;

// ---------------------------------------------------------------------------
// Stacks
// ---------------------------------------------------------------------------

/// Stable identifier for a stack across ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StackId(pub u32);

/// A group of entities operating together under agent control.
/// Lives in GameState — movement reads stack membership for formation steering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stack {
    pub id: StackId,
    pub owner: u8,
    pub members: SmallVec<[EntityKey; 32]>,
    pub formation: FormationType,
    pub leader: EntityKey,
}

// ---------------------------------------------------------------------------
// Roles and structure types
// ---------------------------------------------------------------------------

/// Role of a Person entity. Determines behavior in the economy and agent layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Idle,
    Farmer,
    Worker,
    Soldier,
    Builder,
}

/// Type of structure entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StructureType {
    Farm,
    Village,
    City,
    Depot,
    Wall,
    Tower,
    Workshop,
}

/// Type of resource entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceType {
    Food,
    Material,
    Ore,
    Wood,
    Stone,
}

/// Persistent task assignment for per-tick economy and spectator state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskAssignment {
    Farm { site: EntityKey },
    Workshop { site: EntityKey },
    Patrol,
    Garrison,
    Train,
    Idle,
}

// ---------------------------------------------------------------------------
// Person component
// ---------------------------------------------------------------------------

/// A living being. Presence means the entity is a person (or animal).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    pub role: Role,
    /// Training level 0.0–1.0. Affects aim, block timing, target leading.
    pub combat_skill: f32,
    /// Current long-lived assignment used by the economy/runtime layers.
    pub task: Option<TaskAssignment>,
}

// ---------------------------------------------------------------------------
// Combatant component
// ---------------------------------------------------------------------------

/// Can engage in combat. Separated from Person because equipment entities
/// don't fight, and projectiles don't have facing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Combatant {
    /// Facing direction in radians. 0 = east, PI/2 = north.
    pub facing: f32,
    /// Current engagement target.
    pub target: Option<EntityKey>,
    /// Active attack in progress.
    pub attack: Option<AttackState>,
    /// Recovery after attack.
    pub cooldown: Option<CooldownState>,
}

impl Default for Combatant {
    fn default() -> Self {
        Self::new()
    }
}

impl Combatant {
    pub fn new() -> Self {
        Self {
            facing: 0.0,
            target: None,
            attack: None,
            cooldown: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Structure component
// ---------------------------------------------------------------------------

/// A building or fortification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Structure {
    pub structure_type: StructureType,
    /// 0.0 = foundation, 1.0 = complete.
    pub build_progress: f32,
    /// Structural health. Material-dependent.
    pub integrity: f32,
    /// Maximum number of contained entities.
    pub capacity: usize,
    /// What the structure is built from.
    pub material: MaterialType,
}

// ---------------------------------------------------------------------------
// Resource component
// ---------------------------------------------------------------------------

/// A material or food quantity. Can exist as a standalone entity (on ground,
/// in stockpile) or be contained within a structure/person.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub resource_type: ResourceType,
    pub amount: f32,
}

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

/// Universal entity. Components are optional — presence determines capability.
/// AoS layout for V3.0 (profile and switch to SoA if cache misses are measured).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Public monotonic ID, stable across the entity's lifetime.
    pub id: u32,
    /// World-space position. None if contained in another entity.
    pub pos: Option<Vec3>,
    /// Cached hex membership (derived from pos via hex projection + hysteresis).
    pub hex: Option<Axial>,
    /// Player owner. None for neutral entities (terrain features, wild animals).
    pub owner: Option<u8>,

    // -- Containment --
    /// The entity this one is inside (e.g., sword inside a person's equipment).
    pub contained_in: Option<EntityKey>,
    /// Entities contained within this one (e.g., persons in a structure).
    pub contains: SmallVec<[EntityKey; 4]>,

    // -- Components (presence = capability) --
    pub person: Option<Person>,
    pub mobile: Option<Mobile>,
    pub combatant: Option<Combatant>,
    pub vitals: Option<Vitals>,
    pub wounds: Option<WoundList>,
    pub equipment: Option<Equipment>,
    pub weapon_props: Option<WeaponProperties>,
    pub armor_props: Option<ArmorProperties>,
    pub projectile: Option<Projectile>,
    pub structure: Option<Structure>,
    pub resource: Option<Resource>,
}

impl Entity {
    /// Minimal entity with only an ID. All components None.
    fn bare(id: u32) -> Self {
        Self {
            id,
            pos: None,
            hex: None,
            owner: None,
            contained_in: None,
            contains: SmallVec::new(),
            person: None,
            mobile: None,
            combatant: None,
            vitals: None,
            wounds: None,
            equipment: None,
            weapon_props: None,
            armor_props: None,
            projectile: None,
            structure: None,
            resource: None,
        }
    }
}

// ---------------------------------------------------------------------------
// EntityBuilder
// ---------------------------------------------------------------------------

/// Builder for composing entities from components before spawning.
pub struct EntityBuilder {
    pub(crate) pos: Option<Vec3>,
    pub(crate) owner: Option<u8>,
    pub(crate) person: Option<Person>,
    pub(crate) mobile: Option<Mobile>,
    pub(crate) combatant: Option<Combatant>,
    pub(crate) vitals: Option<Vitals>,
    pub(crate) equipment: Option<Equipment>,
    pub(crate) weapon_props: Option<WeaponProperties>,
    pub(crate) armor_props: Option<ArmorProperties>,
    pub(crate) projectile: Option<Projectile>,
    pub(crate) structure: Option<Structure>,
    pub(crate) resource: Option<Resource>,
}

impl Default for EntityBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EntityBuilder {
    pub fn new() -> Self {
        Self {
            pos: None,
            owner: None,
            person: None,
            mobile: None,
            combatant: None,
            vitals: None,
            equipment: None,
            weapon_props: None,
            armor_props: None,
            projectile: None,
            structure: None,
            resource: None,
        }
    }

    pub fn pos(mut self, pos: Vec3) -> Self {
        self.pos = Some(pos);
        self
    }

    pub fn owner(mut self, owner: u8) -> Self {
        self.owner = Some(owner);
        self
    }

    pub fn person(mut self, person: Person) -> Self {
        self.person = Some(person);
        self
    }

    pub fn mobile(mut self, mobile: Mobile) -> Self {
        self.mobile = Some(mobile);
        self
    }

    pub fn combatant(mut self, combatant: Combatant) -> Self {
        self.combatant = Some(combatant);
        self
    }

    pub fn vitals(mut self) -> Self {
        self.vitals = Some(Vitals::new());
        self
    }

    pub fn equipment(mut self, equipment: Equipment) -> Self {
        self.equipment = Some(equipment);
        self
    }

    pub fn weapon_props(mut self, props: WeaponProperties) -> Self {
        self.weapon_props = Some(props);
        self
    }

    pub fn armor_props(mut self, props: ArmorProperties) -> Self {
        self.armor_props = Some(props);
        self
    }

    pub fn projectile(mut self, proj: Projectile) -> Self {
        self.projectile = Some(proj);
        self
    }

    pub fn structure(mut self, structure: Structure) -> Self {
        self.structure = Some(structure);
        self
    }

    pub fn resource(mut self, resource: Resource) -> Self {
        self.resource = Some(resource);
        self
    }

    /// Build into an Entity with the given ID. Used by `spawn_entity`.
    pub(crate) fn build(self, id: u32) -> Entity {
        let mut e = Entity::bare(id);
        e.pos = self.pos;
        e.owner = self.owner;
        e.person = self.person;
        e.mobile = self.mobile;
        e.combatant = self.combatant;
        e.vitals = self.vitals;
        e.wounds = if e.vitals.is_some() {
            Some(WoundList::new())
        } else {
            None
        };
        e.equipment = self.equipment;
        e.weapon_props = self.weapon_props;
        e.armor_props = self.armor_props;
        e.projectile = self.projectile;
        e.structure = self.structure;
        e.resource = self.resource;
        e
    }
}

// ---------------------------------------------------------------------------
// GameState
// ---------------------------------------------------------------------------

/// Top-level game state. Owns all entities, spatial data, and tick counter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameState {
    pub entities: SlotMap<EntityKey, Entity>,
    pub spatial_index: SpatialIndex,
    pub heightfield: Heightfield,
    pub stacks: Vec<Stack>,
    pub map_width: usize,
    pub map_height: usize,
    pub num_players: u8,
    pub game_time: f64,
    pub tick: u64,
    /// Combat observation log. Drained by the protocol layer after each tick.
    #[serde(skip)]
    pub combat_log: CombatLog,
    next_id: u32,
    next_stack_id: u32,
}

impl GameState {
    pub fn new(
        map_width: usize,
        map_height: usize,
        num_players: u8,
        heightfield: Heightfield,
    ) -> Self {
        Self {
            entities: SlotMap::with_key(),
            spatial_index: SpatialIndex::new(map_width, map_height),
            heightfield,
            stacks: Vec::new(),
            map_width,
            map_height,
            num_players,
            game_time: 0.0,
            tick: 0,
            combat_log: CombatLog::new(),
            next_id: 1,
            next_stack_id: 1,
        }
    }

    /// Allocate a new monotonic entity ID.
    pub(crate) fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Allocate a new monotonic stack ID.
    pub fn alloc_stack_id(&mut self) -> StackId {
        let id = self.next_stack_id;
        self.next_stack_id += 1;
        StackId(id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::super::spatial::GeoMaterial;
    use super::*;

    fn test_state() -> GameState {
        let hf = Heightfield::new(10, 10, 0.0, GeoMaterial::Soil);
        GameState::new(10, 10, 2, hf)
    }

    #[test]
    fn game_state_initial() {
        let gs = test_state();
        assert_eq!(gs.entities.len(), 0);
        assert_eq!(gs.tick, 0);
        assert_eq!(gs.num_players, 2);
    }

    #[test]
    fn alloc_id_monotonic() {
        let mut gs = test_state();
        let a = gs.alloc_id();
        let b = gs.alloc_id();
        let c = gs.alloc_id();
        assert_eq!(a, 1);
        assert_eq!(b, 2);
        assert_eq!(c, 3);
    }

    #[test]
    fn entity_builder_minimal() {
        let e = EntityBuilder::new()
            .pos(Vec3::new(10.0, 20.0, 0.0))
            .owner(0)
            .build(1);
        assert_eq!(e.id, 1);
        assert!(e.pos.is_some());
        assert_eq!(e.owner, Some(0));
        assert!(e.person.is_none());
        assert!(e.wounds.is_none());
    }

    #[test]
    fn entity_builder_soldier() {
        let e = EntityBuilder::new()
            .pos(Vec3::ZERO)
            .owner(1)
            .person(Person {
                role: Role::Soldier,
                combat_skill: 0.6,
                task: None,
            })
            .mobile(Mobile::new(2.0, 10.0))
            .combatant(Combatant::new())
            .vitals()
            .equipment(Equipment::empty())
            .build(42);

        assert_eq!(e.id, 42);
        assert_eq!(e.person.as_ref().unwrap().role, Role::Soldier);
        assert!(e.mobile.is_some());
        assert!(e.combatant.is_some());
        assert!(e.vitals.is_some());
        assert!(e.wounds.is_some()); // auto-created when vitals present
        assert!(e.equipment.is_some());
    }

    #[test]
    fn entity_builder_weapon() {
        use super::super::weapon::iron_sword;
        let e = EntityBuilder::new()
            .pos(Vec3::ZERO)
            .weapon_props(iron_sword())
            .build(10);
        assert!(e.weapon_props.is_some());
        assert!(e.person.is_none());
    }

    #[test]
    fn entity_size_reasonable() {
        let size = std::mem::size_of::<Entity>();
        // AoS with all optional components. Plan E.1 says profile and switch
        // to SoA if cache misses measured. Alert at 1024 bytes.
        assert!(
            size <= 1024,
            "Entity struct is {size} bytes — profile cache misses and consider SoA"
        );
    }
}
