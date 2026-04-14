// V3 wire protocol types — mirrors crates/web/src/v3_protocol.rs

export type EntityKind = "Person" | "Structure";
export type TimeMode = "Strategic" | "Tactical" | "Cinematic";
export type WoundSeverity = "Light" | "Serious" | "Critical";
export type DamageType = "Slash" | "Pierce" | "Crush";
export type FormationType = "Column" | "Line" | "Wedge" | "Square" | "Skirmish";
export type Role = "Idle" | "Farmer" | "Worker" | "Soldier" | "Builder";
export type ResourceType = "Food" | "Material" | "Ore" | "Wood" | "Stone";
export type StructureType =
  | "Farm" | "Village" | "City" | "Depot" | "Wall" | "Tower" | "Workshop";
export type BodyZone = "Head" | "Torso" | "LeftArm" | "RightArm" | "Legs";

// ---------------------------------------------------------------------------
// Init — sent once on spectator connect
// ---------------------------------------------------------------------------

export interface V3Init {
  width: number;
  height: number;
  terrain: number[];
  height_map: number[];
  material_map: number[];
  region_ids: number[];
  player_count: number;
  agent_names: string[];
  agent_versions: string[];
  game_number: number;
}

// ---------------------------------------------------------------------------
// Entity info
// ---------------------------------------------------------------------------

export interface SpectatorEntityInfo {
  id: number;
  owner?: number | null;
  x: number;
  y: number;
  z: number;
  hex_q: number;
  hex_r: number;
  facing?: number;
  entity_kind: EntityKind;
  role?: Role;
  blood?: number;
  stamina?: number;
  wounds?: [BodyZone, WoundSeverity][];
  weapon_type?: string;
  armor_type?: string;
  resource_type?: ResourceType;
  resource_amount?: number;
  structure_type?: StructureType;
  build_progress?: number;
  contains_count: number;
  stack_id?: number;
  current_task?: string;
}

// ---------------------------------------------------------------------------
// Entity update — changed fields only
// ---------------------------------------------------------------------------

export interface EntityUpdate {
  id: number;
  x?: number;
  y?: number;
  z?: number;
  hex_q?: number;
  hex_r?: number;
  facing?: number;
  blood?: number;
  stamina?: number;
  wounds?: [BodyZone, WoundSeverity][];
  weapon_type?: string;
  armor_type?: string;
  contains_count?: number;
  stack_id?: number | null;
  current_task?: string | null;
}

// ---------------------------------------------------------------------------
// Projectile info
// ---------------------------------------------------------------------------

export interface ProjectileInfo {
  id: number;
  x: number;
  y: number;
  z: number;
  vx: number;
  vy: number;
  vz: number;
  damage_type: DamageType;
}

// ---------------------------------------------------------------------------
// Stack info
// ---------------------------------------------------------------------------

export interface StackInfo {
  id: number;
  owner: number;
  members: number[];
  formation: FormationType;
  center_x: number;
  center_y: number;
  facing: number;
}

export interface StackUpdate {
  id: number;
  members?: number[];
  formation?: FormationType;
  center_x?: number;
  center_y?: number;
  facing?: number;
}

// ---------------------------------------------------------------------------
// Player info
// ---------------------------------------------------------------------------

export interface PlayerInfo {
  id: number;
  population: number;
  territory: number;
  food_level: number;
  material_level: number;
  alive: boolean;
  score: number;
}

// ---------------------------------------------------------------------------
// Hex delta
// ---------------------------------------------------------------------------

export interface HexDelta {
  index: number;
  owner?: number | null;
  road_level?: number;
  structure_id?: number | null;
}

// ---------------------------------------------------------------------------
// Snapshot — full state
// ---------------------------------------------------------------------------

export interface V3Snapshot {
  tick: number;
  dt: number;
  full_state: boolean;
  entities: SpectatorEntityInfo[];
  projectiles: ProjectileInfo[];
  stacks: StackInfo[];
  hex_ownership: (number | null)[];
  hex_roads: number[];
  hex_structures: (number | null)[];
  players: PlayerInfo[];
}

// ---------------------------------------------------------------------------
// Snapshot delta — per-tick update
// ---------------------------------------------------------------------------

export interface V3SnapshotDelta {
  tick: number;
  dt: number;
  full_state: boolean;
  entities_appeared: SpectatorEntityInfo[];
  entities_updated: EntityUpdate[];
  entities_removed: number[];
  projectiles_spawned: ProjectileInfo[];
  projectiles_removed: number[];
  stacks_created: StackInfo[];
  stacks_updated: StackUpdate[];
  stacks_dissolved: number[];
  hex_changes: HexDelta[];
  players: PlayerInfo[];
}

// ---------------------------------------------------------------------------
// RR status
// ---------------------------------------------------------------------------

export interface V3RrStatus {
  game_number: number;
  current_tick: number;
  dt: number;
  mode: TimeMode;
  paused: boolean;
  tick_ms: number;
  autoplay: boolean;
  capturable_start_tick?: number;
  capturable_end_tick?: number;
  active_capture?: string;
}

// ---------------------------------------------------------------------------
// Server-to-spectator message union
// ---------------------------------------------------------------------------

export type V3ServerToSpectator =
  | { type: "v3_init"; game_number: number } & V3Init
  | { type: "v3_snapshot" } & V3Snapshot
  | { type: "v3_snapshot_delta" } & V3SnapshotDelta
  | { type: "v3_game_end"; winner: number | null; tick: number; timed_out: boolean; scores: number[] }
  | { type: "v3_config"; tick_ms?: number; mode?: TimeMode; autoplay?: boolean }
  | ({ type: "v3_rr_status" } & V3RrStatus);

// ---------------------------------------------------------------------------
// Replay types
// ---------------------------------------------------------------------------

export interface V3ChunkEntry {
  tick_start: number;
  tick_end: number;
  byte_offset: number;
  byte_length: number;
}

export interface V3ReplayHeader {
  magic: string;
  version: number;
  game_seed: number;
  agent_names: string[];
  agent_versions: string[];
  init: V3Init;
  chunk_count: number;
  chunk_index: V3ChunkEntry[];
}

export interface V3ReplayChunk {
  keyframe: V3Snapshot;
  deltas: V3SnapshotDelta[];
  dt_per_tick: number[];
}

// ---------------------------------------------------------------------------
// Review types
// ---------------------------------------------------------------------------

export interface V3ReviewBundleSummary {
  id: string;
  game_number: number;
  tick: number;
  annotation?: string;
  agent_names: string[];
  agent_versions: string[];
  seed: number;
}
