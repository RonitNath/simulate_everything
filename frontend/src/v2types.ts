// ---- Biome type ----
export type BiomeName =
  | "desert" | "steppe" | "grassland" | "forest"
  | "jungle" | "tundra" | "mountain";

// ---- Unit snapshot (expanded) ----
export interface V2UnitSnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  strength: number;
  engagements: Array<{ enemy_id: number; edge: number }>;
  move_cooldown: number;
  destination: { q: number; r: number } | null;
  engaged: boolean;
  is_general: boolean;
}

// ---- Population snapshot ----
export interface V2PopSnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  count: number;
  role: "Idle" | "Farmer" | "Worker" | "Soldier";
  training: number;
}

// ---- Convoy snapshot ----
export interface V2ConvoySnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  origin: { q: number; r: number };
  destination: { q: number; r: number };
  cargo_type: "Food" | "Material" | "Settlers";
  cargo_amount: number;
  capacity: number;
  speed: number;
  move_cooldown: number;
  returning: boolean;
}

// ---- Score snapshot ----
export interface V2ScoreSnapshot {
  player_id: number;
  population: number;
  territory: number;
  military: number;
  stockpiles: number;
  total: number;
}

// ---- WS frame (expanded) ----
export interface V2Frame {
  tick: number;
  units: V2UnitSnapshot[];
  player_food: number[];
  player_material: number[];
  alive: boolean[];
  territory: Array<number | null>;
  roads: number[];
  depots: boolean[];
  population: V2PopSnapshot[];
  convoys: V2ConvoySnapshot[];
  scores: V2ScoreSnapshot[];
}

// ---- WS GameInfo (expanded) ----
export interface V2GameInfo {
  width: number;
  height: number;
  terrain: number[];
  material_map: number[];
  heights: number[];
  moistures: number[];
  biomes: BiomeName[];
  rivers: boolean[];
  num_players: number;
  agent_names: string[];
  game_number: number;
}

// ---- Replay types ----
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

// ---- Board interfaces (consumed by HexBoard) ----
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
  units: V2UnitSnapshot[];
  territory: Array<number | null>;
  roads: number[];
  depots: boolean[];
  population: V2PopSnapshot[];
  convoys: V2ConvoySnapshot[];
}

// ---- Normalization helpers ----
export function normalizeGameInfoStatic(g: V2GameInfo): BoardStaticData {
  return {
    width: g.width, height: g.height,
    terrain: g.terrain, materialMap: g.material_map,
    heights: g.heights, moistures: g.moistures,
    biomes: g.biomes, rivers: g.rivers,
  };
}

export function normalizeReplayStatic(r: V2Replay): BoardStaticData {
  return {
    width: r.width, height: r.height,
    terrain: r.terrain,
    materialMap: r.material_map,
    heights: r.static_cells.map(c => c.height),
    moistures: r.static_cells.map(c => c.moisture),
    // Replay biomes come from Rust's derive(Serialize) — may be PascalCase
    biomes: r.static_cells.map(c => c.biome.toLowerCase() as BiomeName),
    rivers: r.static_cells.map(c => c.is_river),
  };
}

export function normalizeWsFrame(f: V2Frame): BoardFrameData {
  return {
    units: f.units,
    territory: f.territory,
    roads: f.roads,
    depots: f.depots,
    population: f.population,
    convoys: f.convoys,
  };
}

export function normalizeReplayFrame(f: V2ReplayFrame): BoardFrameData {
  return {
    units: f.units,
    territory: f.cells.map(c => c.stockpile_owner),
    roads: f.cells.map(c => c.road_level),
    depots: f.cells.map(c => c.has_depot),
    population: f.population,
    convoys: f.convoys,
  };
}
