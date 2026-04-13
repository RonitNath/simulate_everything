export interface Cell {
  tile: "Empty" | "Mountain" | "City" | "General";
  owner: number | null;
  armies: number;
}

export interface PlayerStats {
  player: number;
  land: number;
  armies: number;
  alive: boolean;
}

export interface Frame {
  turn: number;
  grid: Cell[];
  stats: PlayerStats[];
  compute_us: number[];
}

export interface Replay {
  width: number;
  height: number;
  num_players: number;
  agent_names: string[];
  frames: Frame[];
  winner: number | null;
}

