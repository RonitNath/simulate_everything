export type BiomeName =
  | "desert" | "steppe" | "grassland" | "forest"
  | "jungle" | "tundra" | "mountain";

export interface V2UnitSnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  strength: number;
  engagements?: Array<{ enemy_id: number; edge: number }>;
  move_cooldown?: number;
  destination?: { q: number; r: number } | null;
  engaged: boolean;
  ration_level?: number;
  _dead?: boolean;
  _deadTick?: number;
}

export interface V2PopSnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  count: number;
  role: "Idle" | "Farmer" | "Worker" | "Soldier";
  training: number;
}

export interface V2ConvoySnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  origin?: { q: number; r: number };
  destination?: { q: number; r: number };
  cargo_type: "Food" | "Material" | "Settlers";
  cargo_amount?: number;
  capacity?: number;
  speed?: number;
  move_cooldown?: number;
  returning?: boolean;
}

export interface SpectatorEntity {
  id: number;
  owner: number | null;
  q: number;
  r: number;
  health?: number;
  role?: string;
  combat_skill?: number;
  engaged: boolean;
  facing?: number;
  resource_type?: string;
  resource_amount?: number;
  structure_type?: string;
  build_progress?: number;
  contains_count: number;
}

export interface V2ScoreSnapshot {
  player_id: number;
  population: number;
  territory: number;
  military: number;
  stockpiles: number;
  total: number;
}

export interface V2HexDelta {
  index: number;
  owner: number | null;
  road_level: number;
  has_settlement: boolean;
  settlement_owner: number | null;
}

export interface V2Settlement {
  id?: number;
  q: number;
  r: number;
  owner: number;
  settlement_type?: "Farm" | "Village" | "City";
  population?: number;
}

export interface V2SpectatorPlayer {
  id: number;
  alive: boolean;
  population: number;
  territory: number;
  food_level: number;
  material_level: number;
}

export interface V2Frame {
  tick: number;
  entities: SpectatorEntity[];
  units: V2UnitSnapshot[];
  player_food?: number[];
  player_material?: number[];
  alive?: boolean[];
  territory?: Array<number | null>;
  roads?: number[];
  depots?: boolean[];
  population?: V2PopSnapshot[];
  convoys: V2ConvoySnapshot[];
  scores?: V2ScoreSnapshot[];
  engagements?: [number, number][];
  hex_ownership?: Array<number | null>;
  road_levels?: number[];
  settlements?: V2Settlement[];
  players?: V2SpectatorPlayer[];
}

export interface V2GameInfo {
  width: number;
  height: number;
  terrain: number[];
  material_map: number[];
  heights: number[];
  moistures: number[];
  biomes: BiomeName[];
  rivers: boolean[];
  height_map?: number[];
  region_ids?: number[];
  num_players: number;
  agent_names: string[];
  game_number: number;
}

export interface V2StaticCell {
  terrain_value: number;
  material_value: number;
  height: number;
  moisture: number;
  biome: BiomeName;
  is_river: boolean;
  water_access: number;
  region_id: number;
}

export interface V2CellSnapshot {
  food_stockpile: number;
  material_stockpile: number;
  stockpile_owner: number | null;
  road_level: number;
  has_depot: boolean;
}

export interface V2ReplayFrame {
  tick: number;
  units: V2UnitSnapshot[];
  player_food: number[];
  player_material: number[];
  alive: boolean[];
  cells: V2CellSnapshot[];
  population: V2PopSnapshot[];
  convoys: V2ConvoySnapshot[];
  scores: V2ScoreSnapshot[];
  settlements?: V2Settlement[];
}

export interface V2Replay {
  width: number;
  height: number;
  terrain: number[];
  material_map: number[];
  static_cells: V2StaticCell[];
  num_players: number;
  agent_names: string[];
  frames: V2ReplayFrame[];
  winner: number | null;
  timed_out: boolean;
}

export interface V2OverlapAnomaly {
  tick: number;
  q: number;
  r: number;
  owners: number[];
  unit_ids: number[];
}

export interface V2ReviewLogWindow {
  events: Array<Record<string, unknown>>;
  agent_polls: Array<Record<string, unknown>>;
  economy_samples: Array<Record<string, unknown>>;
  unit_positions: Array<Record<string, unknown>>;
}

export interface V2ReviewBundleSummary {
  id: string;
  created_at: number;
  kind: "point" | "segment";
  game_number: number;
  seed: number;
  agent_names: string[];
  start_tick: number;
  stop_tick: number | null;
  flagged_ticks: number[];
  range_start: number;
  range_end: number;
  complete: boolean;
  saved: boolean;
  anomaly_count: number;
  event_count: number;
}

export interface V2ReviewBundle {
  id: string;
  created_at: number;
  kind: "point" | "segment";
  game_number: number;
  seed: number;
  agent_names: string[];
  start_tick: number;
  stop_tick: number | null;
  flagged_ticks: number[];
  range_start: number;
  range_end: number;
  complete: boolean;
  saved: boolean;
  anomaly_count: number;
  event_count: number;
  replay: V2Replay;
  anomalies: V2OverlapAnomaly[];
  log: V2ReviewLogWindow;
}

export interface V2ReviewListResponse {
  pending: V2ReviewBundleSummary[];
  saved: V2ReviewBundleSummary[];
}

export interface BoardStaticData {
  width: number;
  height: number;
  terrain: number[];
  materialMap: number[];
  heights: number[];
  moistures: number[];
  biomes: BiomeName[];
  rivers: boolean[];
}

export interface BoardFrameData {
  tick: number;
  entities: SpectatorEntity[];
  units: V2UnitSnapshot[];
  territory: Array<number | null>;
  stockpileOwners: Array<number | null>;
  roads: number[];
  depots: boolean[];
  population: V2PopSnapshot[];
  convoys: V2ConvoySnapshot[];
  settlements: V2Settlement[];
}

export interface BoardHexHover {
  index: number;
  q: number;
  r: number;
  row: number;
  col: number;
  centerX: number;
  centerY: number;
}

export interface V2ReviewStatus {
  paused: boolean;
  tick_ms: number;
  game_number: number | null;
  current_tick: number | null;
  capturable_start_tick: number | null;
  capturable_end_tick: number | null;
  pending_capture_count: number;
  active_capture: V2ReviewBundleSummary | null;
  review_dir: string;
}

export function normalizeGameInfoStatic(g: V2GameInfo): BoardStaticData {
  return {
    width: g.width,
    height: g.height,
    terrain: g.terrain,
    materialMap: g.material_map,
    heights: g.heights.length > 0 ? g.heights : (g.height_map ?? []),
    moistures: g.moistures,
    biomes: g.biomes,
    rivers: g.rivers,
  };
}

export function normalizeReplayStatic(r: V2Replay): BoardStaticData {
  return {
    width: r.width,
    height: r.height,
    terrain: r.terrain,
    materialMap: r.material_map,
    heights: r.static_cells.map((c) => c.height),
    moistures: r.static_cells.map((c) => c.moisture),
    biomes: r.static_cells.map((c) => c.biome.toLowerCase() as BiomeName),
    rivers: r.static_cells.map((c) => c.is_river),
  };
}

export function normalizeWsFrame(f: V2Frame): BoardFrameData {
  const territory = f.territory ?? f.hex_ownership ?? [];
  const roads = f.roads ?? f.road_levels ?? [];
  return {
    tick: f.tick,
    entities: f.entities ?? [],
    units: f.units,
    territory,
    stockpileOwners: territory,
    roads,
    depots: f.depots ?? Array.from({ length: territory.length }, () => false),
    population: f.population ?? [],
    convoys: f.convoys,
    settlements: f.settlements ?? [],
  };
}

export function normalizeReplayFrame(f: V2ReplayFrame): BoardFrameData {
  return {
    tick: f.tick,
    entities: [], // replay frames predate unified entities
    units: f.units,
    territory: f.cells.map((c) => c.stockpile_owner),
    stockpileOwners: f.cells.map((c) => c.stockpile_owner),
    roads: f.cells.map((c) => c.road_level),
    depots: f.cells.map((c) => c.has_depot),
    population: f.population,
    convoys: f.convoys,
    settlements: f.settlements ?? [],
  };
}
