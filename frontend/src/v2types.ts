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
  is_general: boolean;
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
  settlements: V2Settlement[];
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
    units: f.units,
    territory,
    roads,
    depots: f.depots ?? Array.from({ length: territory.length }, () => false),
    population: f.population ?? [],
    convoys: f.convoys,
    settlements: f.settlements ?? [],
  };
}

export function normalizeReplayFrame(f: V2ReplayFrame): BoardFrameData {
  return {
    units: f.units,
    territory: f.cells.map((c) => c.stockpile_owner),
    roads: f.cells.map((c) => c.road_level),
    depots: f.cells.map((c) => c.has_depot),
    population: f.population,
    convoys: f.convoys,
    settlements: f.settlements ?? [],
  };
}
