export interface V2UnitSnapshot {
  id: number;
  owner: number;
  q: number;
  r: number;
  strength: number;
  engaged: boolean;
  is_general: boolean;
}

export interface V2Frame {
  tick: number;
  units: V2UnitSnapshot[];
  player_resources: number[];
  alive: boolean[];
}

export interface V2GameInfo {
  width: number;
  height: number;
  terrain: number[];
  num_players: number;
  agent_names: string[];
}
