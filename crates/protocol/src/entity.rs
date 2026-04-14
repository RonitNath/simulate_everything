use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Shared enums — canonical definitions, re-exported by engine
// ---------------------------------------------------------------------------

/// Body zones for hit location and armor coverage.
pub const ZONE_COUNT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BodyZone {
    Head,
    Torso,
    LeftArm,
    RightArm,
    Legs,
}

impl BodyZone {
    pub const ALL: [BodyZone; ZONE_COUNT] = [
        BodyZone::Head,
        BodyZone::Torso,
        BodyZone::LeftArm,
        BodyZone::RightArm,
        BodyZone::Legs,
    ];
}

/// Damage delivery mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DamageType {
    Slash,
    Pierce,
    Crush,
}

/// Role of a Person entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    Idle,
    Farmer,
    Worker,
    Soldier,
    Builder,
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

/// Formation type for a group of entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormationType {
    Column,
    Line,
    Wedge,
    Square,
    Skirmish,
}

// ---------------------------------------------------------------------------
// Protocol-only enums
// ---------------------------------------------------------------------------

/// Entity kind discriminator for the wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    Person,
    Structure,
}

/// 2-bit wound severity for spectator wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WoundSeverity {
    Light,
    Serious,
    Critical,
}

/// Simulation time resolution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeMode {
    Strategic,
    Tactical,
    Cinematic,
}

impl TimeMode {
    pub fn dt(&self) -> f32 {
        match self {
            TimeMode::Strategic => 3600.0,
            TimeMode::Tactical => 1.0,
            TimeMode::Cinematic => 0.01,
        }
    }
}

// ---------------------------------------------------------------------------
// Wire types — entity info
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectatorEntityInfo {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<u8>,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub hex_q: i32,
    pub hex_r: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facing: Option<f32>,
    pub entity_kind: EntityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blood: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamina: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wounds: Vec<(BodyZone, WoundSeverity)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<ResourceType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_amount: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_type: Option<StructureType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_progress: Option<f32>,
    pub contains_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_motion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_angle: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_progress: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BodyPointWire {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CapsuleWire {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub radius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DiscWire {
    pub center: [f32; 3],
    pub normal: [f32; 3],
    pub radius: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BodyRenderInfo {
    pub entity_id: u32,
    pub points: [BodyPointWire; 16],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon: Option<CapsuleWire>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shield: Option<DiscWire>,
}

/// Changed fields only (for delta snapshots).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityUpdate {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex_q: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hex_r: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facing: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blood: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamina: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wounds: Option<Vec<(BodyZone, WoundSeverity)>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub armor_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contains_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_id: Option<Option<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_task: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_phase: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_motion: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_angle: Option<Option<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_progress: Option<Option<f32>>,
}

// ---------------------------------------------------------------------------
// Projectile, stack, player, hex
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectileInfo {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    pub damage_type: DamageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackInfo {
    pub id: u32,
    pub owner: u8,
    pub members: Vec<u32>,
    pub formation: FormationType,
    pub center_x: f32,
    pub center_y: f32,
    pub facing: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StackUpdate {
    pub id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub members: Option<Vec<u32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formation: Option<FormationType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_x: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_y: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facing: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInfo {
    pub id: u8,
    pub population: u32,
    pub territory: u32,
    pub food_level: u8,
    pub material_level: u8,
    pub alive: bool,
    pub score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexDelta {
    pub index: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub road_level: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure_id: Option<Option<u32>>,
}
