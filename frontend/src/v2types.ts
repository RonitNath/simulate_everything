export interface V2UnitSnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  strength: number;
  engaged: boolean;
  is_general: boolean;
}

export interface V2ConvoySnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  cargo_type: "Food" | "Material" | "Settlers";
}

export interface V2HexDelta {
  index: number;
  owner: number | null;
  road_level: number;
  has_settlement: boolean;
  settlement_owner: number | null;
}

export interface V2Settlement {
  q: number;
  r: number;
  owner: number;
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
  engagements: [number, number][];
  convoys: V2ConvoySnapshot[];
  hex_ownership: (number | null)[];
  road_levels: number[];
  settlements: V2Settlement[];
  players: V2SpectatorPlayer[];
}

export interface V2GameInfo {
  width: number;
  height: number;
  terrain: number[];
  material_map: number[];
  height_map: number[];
  region_ids: number[];
  num_players: number;
  agent_names: string[];
}
