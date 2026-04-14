const PLAYER_COLORS = [
  "#4b76ff",
  "#ff5c63",
  "#37c871",
  "#f2c94c",
  "#bb6bd9",
  "#3bc9db",
  "#ff9f43",
  "#c7c7c7",
];

export function playerColorHex(playerId: number): string {
  return PLAYER_COLORS[playerId % PLAYER_COLORS.length] ?? "#c7c7c7";
}
