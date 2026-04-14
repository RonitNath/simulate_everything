// Pure rendering functions for hex grid, territory overlay, roads, and settlements.
// No component state — takes PixiJS Graphics objects + data, draws, returns.

import { Graphics } from "pixi.js";
import type { BiomeName } from "../../v2types";

export const SQRT3 = Math.sqrt(3);
export const HEX_SIZE = 20;
export const ENGINE_HEX_RADIUS = 86.60254;
export const WORLD_TO_CANVAS_SCALE = HEX_SIZE / ENGINE_HEX_RADIUS;

export interface HexRegion {
  minRow: number;
  maxRow: number;
  minCol: number;
  maxCol: number;
}

const BIOME_BASE: Record<BiomeName, [number, number, number]> = {
  desert: [180, 160, 90],
  steppe: [140, 150, 80],
  grassland: [80, 140, 60],
  forest: [40, 100, 45],
  jungle: [20, 80, 35],
  tundra: [130, 155, 170],
  mountain: [100, 95, 95],
};

const PLAYER_COLORS_HEX = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

// --- Color utilities ---

function parseHexColor(hex: string): [number, number, number] {
  return [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ];
}

function rgbToNum(r: number, g: number, b: number): number {
  // >>> 0 ensures unsigned 32-bit integer (PixiJS rejects negative color values)
  return ((r << 16) | (g << 8) | b) >>> 0;
}

function biomeColorNum(biome: BiomeName, height: number, isRiver: boolean): number {
  const [br, bg, bb] = BIOME_BASE[biome] ?? BIOME_BASE.grassland;
  // Height shading: darker = lower, lighter = higher. Clamp height to [0,1].
  const h = Math.max(0, Math.min(1, isNaN(height) ? 0.5 : height));
  const t = 0.6 + 0.4 * h;
  let r = Math.round(br * t);
  let g = Math.round(bg * t);
  let b = Math.round(bb * t);
  if (isRiver) {
    r = Math.round(r * 0.8 + 60 * 0.2);
    g = Math.round(g * 0.8 + 120 * 0.2);
    b = Math.round(b * 0.8 + 200 * 0.2);
  }
  return rgbToNum(r, g, b);
}

export function playerColorNum(owner: number): number {
  const [r, g, b] = parseHexColor(PLAYER_COLORS_HEX[owner % PLAYER_COLORS_HEX.length]);
  return rgbToNum(r, g, b);
}

export function playerColorHex(owner: number): string {
  return PLAYER_COLORS_HEX[owner % PLAYER_COLORS_HEX.length];
}

// --- Hex geometry ---

export function hexCenter(row: number, col: number, size: number): [number, number] {
  const x = SQRT3 * size * (col + 0.5 * (row & 1));
  const y = 1.5 * size * row;
  return [x, y];
}

export function worldToCanvas(x: number, y: number): [number, number] {
  return [x * WORLD_TO_CANVAS_SCALE, y * WORLD_TO_CANVAS_SCALE];
}

export function drawHexPath(g: Graphics, cx: number, cy: number, radius: number): void {
  for (let i = 0; i < 6; i++) {
    const angle = (Math.PI / 180) * (60 * i - 30);
    const px = cx + radius * Math.cos(angle);
    const py = cy + radius * Math.sin(angle);
    if (i === 0) g.moveTo(px, py);
    else g.lineTo(px, py);
  }
  g.closePath();
}

const EVEN_NEIGHBORS: [number, number][] = [[-1, -1], [-1, 0], [0, 1], [1, 0], [1, -1], [0, -1]];
const ODD_NEIGHBORS: [number, number][] = [[-1, 0], [-1, 1], [0, 1], [1, 1], [1, 0], [0, -1]];

export function pixelToHex(wx: number, wy: number, size: number): [number, number] {
  const rowApprox = wy / (1.5 * size);
  const row = Math.round(rowApprox);
  const col = Math.round(wx / (SQRT3 * size) - 0.5 * (row & 1));

  let bestRow = row;
  let bestCol = col;
  let bestDist = Infinity;
  for (let dr = -1; dr <= 1; dr++) {
    for (let dc = -1; dc <= 1; dc++) {
      const cr = row + dr;
      const cc = col + dc;
      const [cx, cy] = hexCenter(cr, cc, size);
      const dist = (wx - cx) ** 2 + (wy - cy) ** 2;
      if (dist < bestDist) {
        bestDist = dist;
        bestRow = cr;
        bestCol = cc;
      }
    }
  }
  return [bestRow, bestCol];
}

// --- Board dimensions ---

export function boardPixelSize(width: number, height: number, size: number): [number, number] {
  return [
    SQRT3 * size * (width + 0.5),
    1.5 * size * height,
  ];
}

// --- Terrain rendering ---

export interface TerrainData {
  width: number;
  height: number;
  biomes: BiomeName[];
  heights: number[];
  rivers: boolean[];
  region?: HexRegion | null;
}

export function drawTerrain(g: Graphics, data: TerrainData): void {
  g.clear();
  const { width, height, biomes, heights, rivers, region } = data;
  const size = HEX_SIZE;
  const minRow = Math.max(0, region?.minRow ?? 0);
  const maxRow = Math.min(height - 1, region?.maxRow ?? (height - 1));
  const minCol = Math.max(0, region?.minCol ?? 0);
  const maxCol = Math.min(width - 1, region?.maxCol ?? (width - 1));

  for (let row = minRow; row <= maxRow; row++) {
    for (let col = minCol; col <= maxCol; col++) {
      const idx = row * width + col;
      const biome = (biomes[idx] ?? "grassland") as BiomeName;
      const h = heights[idx] ?? 0.5;
      const isRiver = rivers[idx] ?? false;
      const color = biomeColorNum(biome, h, isRiver);
      const [cx, cy] = hexCenter(row, col, size);
      drawHexPath(g, cx, cy, size * 0.96);
      g.fill({ color });
      g.stroke({ color: 0x1a1a2e, width: Math.max(0.5, size * 0.04) });
    }
  }
}

// --- Territory overlay ---

export function drawTerritory(
  g: Graphics,
  width: number,
  height: number,
  ownership: (number | null)[],
  region?: HexRegion | null,
): void {
  g.clear();
  const size = HEX_SIZE;
  const minRow = Math.max(0, region?.minRow ?? 0);
  const maxRow = Math.min(height - 1, region?.maxRow ?? (height - 1));
  const minCol = Math.max(0, region?.minCol ?? 0);
  const maxCol = Math.min(width - 1, region?.maxCol ?? (width - 1));

  for (let row = minRow; row <= maxRow; row++) {
    for (let col = minCol; col <= maxCol; col++) {
      const idx = row * width + col;
      const owner = ownership[idx];
      if (owner === null || owner === undefined) continue;
      const [cx, cy] = hexCenter(row, col, size);
      drawHexPath(g, cx, cy, size * 0.88);
      g.fill({ color: playerColorNum(owner), alpha: 0.35 });
    }
  }
}

// --- Road rendering ---

export function drawRoads(
  g: Graphics,
  width: number,
  height: number,
  roads: number[],
  region?: HexRegion | null,
): void {
  g.clear();
  const size = HEX_SIZE;
  const minRow = Math.max(0, region?.minRow ?? 0);
  const maxRow = Math.min(height - 1, region?.maxRow ?? (height - 1));
  const minCol = Math.max(0, region?.minCol ?? 0);
  const maxCol = Math.min(width - 1, region?.maxCol ?? (width - 1));

  for (let row = minRow; row <= maxRow; row++) {
    for (let col = minCol; col <= maxCol; col++) {
      const idx = row * width + col;
      const myRoad = roads[idx] ?? 0;
      if (myRoad <= 0) continue;
      const [cx, cy] = hexCenter(row, col, size);
      const neighbors = (row & 1) ? ODD_NEIGHBORS : EVEN_NEIGHBORS;
      for (const [dr, dc] of neighbors) {
        const nr = row + dr;
        const nc = col + dc;
        if (nr < 0 || nr >= height || nc < 0 || nc >= width) continue;
        const nIdx = nr * width + nc;
        const nRoad = roads[nIdx] ?? 0;
        if (nRoad <= 0) continue;
        const [nx, ny] = hexCenter(nr, nc, size);
        const level = Math.min(myRoad, nRoad);
        const roadAlpha = level >= 3 ? 0.8 : level >= 2 ? 0.7 : 0.6;
        const roadColor = level >= 3 ? 0xf0dca0 : level >= 2 ? 0xdcc88c : 0xc8c8b4;
        g.moveTo(cx, cy);
        g.lineTo((cx + nx) / 2, (cy + ny) / 2);
        g.stroke({
          color: roadColor,
          alpha: roadAlpha,
          width: Math.max(1.5, level * 0.8 + size * 0.04),
          cap: "round",
        });
      }
    }
  }
}

// --- Settlement rendering ---

export interface SettlementEntry {
  row: number;
  col: number;
  owner: number;
  structureType: string;
  containsCount: number;
}

export function drawSettlements(g: Graphics, settlements: SettlementEntry[]): void {
  g.clear();
  const size = HEX_SIZE;

  for (const s of settlements) {
    const [cx, cy] = hexCenter(s.row, s.col, size);
    const color = playerColorNum(s.owner);
    const stype = s.structureType;

    if (stype === "Farm") {
      g.circle(cx, cy, size * 0.3);
      g.fill({ color, alpha: 0.3 });
      g.circle(cx, cy, size * 0.25);
      g.fill({ color, alpha: 0.8 });
      g.stroke({ color: 0xffffff, width: 0.5 });
    } else if (stype === "Village") {
      const hs = size * 0.45;
      g.circle(cx, cy, size * 0.45);
      g.fill({ color, alpha: 0.3 });
      const bx = cx - hs;
      const by = cy - hs * 0.1;
      const bw = hs * 2;
      const bh = hs * 1.2;
      const peakY = cy - hs * 1.1;
      g.moveTo(bx, by);
      g.lineTo(bx, by + bh);
      g.lineTo(bx + bw, by + bh);
      g.lineTo(bx + bw, by);
      g.lineTo(cx, peakY);
      g.closePath();
      g.fill({ color, alpha: 0.9 });
      g.stroke({ color: 0xffffff, width: 0.5 });
    } else if (stype === "City") {
      const w = size * 0.4;
      const h = size * 0.45;
      g.circle(cx, cy, size * 0.55);
      g.fill({ color, alpha: 0.35 });
      g.moveTo(cx - w, cy + h);
      g.lineTo(cx - w, cy - h);
      g.lineTo(cx - w * 0.6, cy - h);
      g.lineTo(cx - w * 0.6, cy - h * 1.3);
      g.lineTo(cx - w * 0.2, cy - h * 1.3);
      g.lineTo(cx - w * 0.2, cy - h);
      g.lineTo(cx + w * 0.2, cy - h);
      g.lineTo(cx + w * 0.2, cy - h * 1.3);
      g.lineTo(cx + w * 0.6, cy - h * 1.3);
      g.lineTo(cx + w * 0.6, cy - h);
      g.lineTo(cx + w, cy - h);
      g.lineTo(cx + w, cy + h);
      g.closePath();
      g.fill({ color, alpha: 0.95 });
      g.stroke({ color: 0xffffff, width: 1 });
    } else if (stype === "Depot") {
      const side = Math.max(3, size * 0.22);
      g.rect(cx - side / 2, cy - side / 2, side, side);
      g.fill({ color: 0xc0a000 });
      g.stroke({ color: 0x8a7200, width: 0.5 });
    }

    // Population badge
    if (s.containsCount > 0) {
      const badgeR = size * 0.2;
      const bx = cx + size * 0.5;
      const by = cy - size * 0.5;
      g.circle(bx, by, badgeR);
      g.fill({ color: 0x222222, alpha: 0.85 });
      g.stroke({ color: 0xffffff, width: 0.5 });
    }
  }
}
