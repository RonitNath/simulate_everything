// V3 wire protocol types — mirrors crates/web/src/v3_protocol.rs

export type EntityKind = "Person" | "Site" | "Object";
export type TimeMode = "Strategic" | "Tactical" | "Cinematic";
export type WoundSeverity = "Light" | "Serious" | "Critical";
export type DamageType = "Slash" | "Pierce" | "Crush";
export type FormationType = "Column" | "Line" | "Wedge" | "Square" | "Skirmish";
export type Role = "Idle" | "Farmer" | "Worker" | "Soldier" | "Builder";
export type BodyZone = "Head" | "Torso" | "LeftArm" | "RightArm" | "Legs";
export type MaterialKind =
  | "Iron" | "Steel" | "Bronze" | "Leather" | "Wood" | "Bone" | "Cloth" | "Stone"
  | "Soil" | "Sand" | "Clay" | "Flesh" | "Plant";
export type MatterState = "Solid" | "Liquid" | "Gas" | "Powder";
export type CommodityKind = "Food" | "Material" | "Ore" | "Wood" | "Stone";
export type PropertyTag =
  | "Harvestable" | "Edible" | "Fuel" | "HeatSource" | "Tool" | "Container"
  | "Shelter" | "Workable" | "Structural" | "Stockpile" | "Settlement"
  | "Farm" | "Workshop";

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

export interface PhysicalInfo {
  material: MaterialKind;
  matter_state: MatterState;
  temperature_k: number;
  mass_kg: number;
  hardness: number;
  tags: PropertyTag[];
}

export interface ToolInfo {
  force_mult: number;
  precision: number;
  cutting_edge: number;
  heat_output_k: number;
  capacity_l: number;
  durability: number;
}

export interface MatterInfo {
  commodity: CommodityKind;
  amount: number;
}

export interface SiteInfo {
  build_progress: number;
  integrity: number;
  occupancy_capacity: number;
}

export interface EntityNeedsInfo {
  hunger: number;
  safety: number;
  duty: number;
  rest: number;
  social: number;
  shelter: number;
}

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
  physical?: PhysicalInfo;
  tool?: ToolInfo;
  matter?: MatterInfo;
  site?: SiteInfo;
  contains_count: number;
  stack_id?: number;
  needs?: EntityNeedsInfo;
  current_goal?: string;
  current_action?: string;
  action_queue_preview?: string[];
  decision_reason?: string;
  // Swordplay visual state
  attack_phase?: string;   // "idle" | "windup" | "committed" | "recovery"
  attack_motion?: string;  // "overhead" | "forehand" | "backhand" | "thrust"
  weapon_angle?: number;   // radians, separate from body facing
  attack_progress?: number; // 0.0–1.0 windup progress
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
  physical?: PhysicalInfo | null;
  tool?: ToolInfo | null;
  matter?: MatterInfo | null;
  site?: SiteInfo | null;
  contains_count?: number;
  stack_id?: number | null;
  needs?: EntityNeedsInfo | null;
  current_goal?: string | null;
  current_action?: string | null;
  action_queue_preview?: string[];
  decision_reason?: string | null;
  attack_phase?: string | null;
  attack_motion?: string | null;
  weapon_angle?: number | null;
  attack_progress?: number | null;
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

// ---------------------------------------------------------------------------
// Delta application — apply V3SnapshotDelta to mutable state
// ---------------------------------------------------------------------------

/** Mutable game state maintained by the frontend from init + deltas. */
export interface V3GameState {
  entities: Map<number, SpectatorEntityInfo>;
  projectiles: Map<number, ProjectileInfo>;
  stacks: Map<number, StackInfo>;
  players: PlayerInfo[];
  tick: number;
  dt: number;
}

/** Initialize game state from a full snapshot. */
export function initGameState(snapshot: V3Snapshot): V3GameState {
  const entities = new Map<number, SpectatorEntityInfo>();
  for (const e of snapshot.entities) entities.set(e.id, e);
  const projectiles = new Map<number, ProjectileInfo>();
  for (const p of snapshot.projectiles) projectiles.set(p.id, p);
  const stacks = new Map<number, StackInfo>();
  for (const s of snapshot.stacks) stacks.set(s.id, s);
  return {
    entities,
    projectiles,
    stacks,
    players: snapshot.players,
    tick: snapshot.tick,
    dt: snapshot.dt,
  };
}

/** Apply a delta to mutable game state. */
export function applyDelta(state: V3GameState, delta: V3SnapshotDelta): void {
  state.tick = delta.tick;
  state.dt = delta.dt;
  state.players = delta.players;

  // Entities
  for (const e of delta.entities_appeared) state.entities.set(e.id, e);
  for (const id of delta.entities_removed) state.entities.delete(id);
  for (const u of delta.entities_updated) {
    const e = state.entities.get(u.id);
    if (!e) continue;
    if (u.x !== undefined) e.x = u.x;
    if (u.y !== undefined) e.y = u.y;
    if (u.z !== undefined) e.z = u.z;
    if (u.hex_q !== undefined) e.hex_q = u.hex_q;
    if (u.hex_r !== undefined) e.hex_r = u.hex_r;
    if (u.facing !== undefined) e.facing = u.facing;
    if (u.blood !== undefined) e.blood = u.blood;
    if (u.stamina !== undefined) e.stamina = u.stamina;
    if (u.wounds !== undefined) e.wounds = u.wounds;
    if (u.weapon_type !== undefined) e.weapon_type = u.weapon_type ?? undefined;
    if (u.armor_type !== undefined) e.armor_type = u.armor_type ?? undefined;
    if (u.contains_count !== undefined) e.contains_count = u.contains_count;
    if (u.stack_id !== undefined) e.stack_id = u.stack_id ?? undefined;
    if (u.needs !== undefined) e.needs = u.needs ?? undefined;
    if (u.current_goal !== undefined) e.current_goal = u.current_goal ?? undefined;
    if (u.current_action !== undefined) e.current_action = u.current_action ?? undefined;
    if (u.action_queue_preview !== undefined) e.action_queue_preview = u.action_queue_preview;
    if (u.decision_reason !== undefined) e.decision_reason = u.decision_reason ?? undefined;
    if (u.attack_phase !== undefined) e.attack_phase = u.attack_phase ?? undefined;
    if (u.attack_motion !== undefined) e.attack_motion = u.attack_motion ?? undefined;
    if (u.weapon_angle !== undefined) e.weapon_angle = u.weapon_angle ?? undefined;
    if (u.attack_progress !== undefined) e.attack_progress = u.attack_progress ?? undefined;
  }

  // Projectiles
  for (const p of delta.projectiles_spawned) state.projectiles.set(p.id, p);
  for (const id of delta.projectiles_removed) state.projectiles.delete(id);

  // Stacks
  for (const s of delta.stacks_created) state.stacks.set(s.id, s);
  for (const id of delta.stacks_dissolved) state.stacks.delete(id);
  for (const u of delta.stacks_updated) {
    const s = state.stacks.get(u.id);
    if (!s) continue;
    if (u.members !== undefined) s.members = u.members;
    if (u.formation !== undefined) s.formation = u.formation;
    if (u.center_x !== undefined) s.center_x = u.center_x;
    if (u.center_y !== undefined) s.center_y = u.center_y;
    if (u.facing !== undefined) s.facing = u.facing;
  }
}
