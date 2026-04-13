import { Component, createMemo } from "solid-js";
import type { V2Settlement, V2UnitSnapshot } from "./v2types";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

const SQRT3 = Math.sqrt(3);

function parseHex(hex: string): [number, number, number] {
  return [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ];
}

function playerRgb(owner: number, alpha = 1): string {
  const [r, g, b] = parseHex(PLAYER_COLORS[owner % PLAYER_COLORS.length]);
  return `rgba(${r},${g},${b},${alpha})`;
}

function stackBrightness(totalStrength: number, maxStrength: number): number {
  if (totalStrength <= 0) return 0.3;
  return 0.3 + 0.7 * Math.log1p(totalStrength) / Math.log1p(Math.max(maxStrength, 1));
}

function terrainColor(value: number): string {
  const t = Math.min(value / 3.0, 1.0);
  const r = Math.round(20 + 15 * t);
  const g = Math.round(25 + 40 * t);
  const b = Math.round(20 + 10 * t);
  return `rgb(${r},${g},${b})`;
}

function mixTerrain(base: string, overlay: string): string {
  return `linear-gradient(135deg, ${base}, ${overlay})`;
}

function hexPoints(cx: number, cy: number, size: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 6; i++) {
    const angle = (Math.PI / 180) * (60 * i - 30);
    pts.push(`${cx + size * Math.cos(angle)},${cy + size * Math.sin(angle)}`);
  }
  return pts.join(" ");
}

function hexCenter(row: number, col: number, size: number): [number, number] {
  const x = SQRT3 * size * (col + 0.5 * (row & 1));
  const y = 1.5 * size * row;
  return [x, y];
}

interface CellStack {
  unit: V2UnitSnapshot;
  count: number;
  totalStrength: number;
}

interface HexBoardProps {
  terrain: number[];
  units: V2UnitSnapshot[];
  hexOwnership: (number | null)[];
  roadLevels: number[];
  settlements: V2Settlement[];
  width: number;
  height: number;
  showNumbers?: boolean;
}

const HexBoard: Component<HexBoardProps> = (props) => {
  const hexSize = createMemo(() => {
    const maxW = (window.innerWidth - 320) * 0.88;
    const maxH = (window.innerHeight - 140) * 0.88;
    const gridPixelW = SQRT3 * (props.width + 0.5);
    const gridPixelH = 1.5 * props.height + 0.5;
    return Math.max(4, Math.min(maxW / gridPixelW, maxH / gridPixelH));
  });

  const svgWidth = createMemo(() => SQRT3 * hexSize() * (props.width + 0.5) + hexSize());
  const svgHeight = createMemo(() => 1.5 * hexSize() * props.height + hexSize() * 1.5);

  const settlementMap = createMemo(() => {
    const map = new Map<string, V2Settlement>();
    for (const settlement of props.settlements) {
      map.set(`${settlement.q},${settlement.r}`, settlement);
    }
    return map;
  });

  const unitMap = createMemo(() => {
    const map = new Map<string, CellStack>();
    for (const u of props.units) {
      const row = u.r;
      const col = u.q + (u.r - (u.r & 1)) / 2;
      const key = `${row},${col}`;
      const existing = map.get(key);
      if (!existing) {
        map.set(key, { unit: u, count: 1, totalStrength: u.strength });
      } else {
        existing.count++;
        existing.totalStrength += u.strength;
        if (u.strength > existing.unit.strength) {
          existing.unit = u;
        }
      }
    }
    return map;
  });

  const maxStackStrength = createMemo(() => {
    let max = 1;
    for (const entry of unitMap().values()) {
      if (entry.totalStrength > max) max = entry.totalStrength;
    }
    return max;
  });

  const cells = createMemo(() => {
    const s = hexSize();
    const umap = unitMap();
    const maxStr = maxStackStrength();
    const settlements = settlementMap();
    const result: Array<{
      cx: number; cy: number; pts: string; fill: string; stroke: string; strokeWidth: number;
      entry: CellStack | undefined; owner: number | null; roadLevel: number; settlement?: V2Settlement;
    }> = [];

    for (let row = 0; row < props.height; row++) {
      for (let col = 0; col < props.width; col++) {
        const [cx, cy] = hexCenter(row, col, s);
        const idx = row * props.width + col;
        const q = col - Math.floor((row - (row & 1)) / 2);
        const r = row;
        const key = `${row},${col}`;
        const entry = umap.get(key);
        const owner = props.hexOwnership[idx] ?? null;
        const roadLevel = props.roadLevels[idx] ?? 0;
        const settlement = settlements.get(`${q},${r}`);

        let fill = terrainColor(props.terrain[idx] ?? 0);
        let stroke = roadLevel > 0 ? "#d3b36b" : "#1a1a2e";
        let strokeWidth = roadLevel > 0 ? Math.max(1, s * (0.04 + 0.015 * roadLevel)) : Math.max(0.5, s * 0.04);

        if (owner !== null) {
          fill = mixTerrain(fill, playerRgb(owner, 0.45));
        }
        if (entry) {
          const t = stackBrightness(entry.totalStrength, maxStr);
          fill = playerRgb(entry.unit.owner, entry.unit.is_general ? Math.max(t, 0.9) : t);
          if (entry.unit.is_general) {
            stroke = "#ffd700";
            strokeWidth = Math.max(1.5, s * 0.08);
          } else if (entry.unit.engaged) {
            stroke = "#ff0";
            strokeWidth = Math.max(1, s * 0.06);
          }
        }

        result.push({
          cx,
          cy,
          pts: hexPoints(cx, cy, s * 0.96),
          fill,
          stroke,
          strokeWidth,
          entry,
          owner,
          roadLevel,
          settlement,
        });
      }
    }

    return result;
  });

  return (
    <svg
      width={svgWidth()}
      height={svgHeight()}
      viewBox={`${-hexSize()} ${-hexSize()} ${svgWidth()} ${svgHeight()}`}
      style={{ "max-width": "100%", "max-height": "100%" }}
    >
      {cells().map((c) => {
        const s = hexSize();
        return (
          <g>
            <polygon points={c.pts} fill={c.fill} stroke={c.stroke} stroke-width={c.strokeWidth} />
            {c.settlement && (
              <circle cx={c.cx} cy={c.cy} r={Math.max(2, s * 0.18)} fill={playerRgb(c.settlement.owner, 0.9)} stroke="#fff" stroke-width={Math.max(0.5, s * 0.03)} />
            )}
            {c.entry?.unit.is_general && (
              <text
                x={c.cx} y={c.cy + (props.showNumbers ? -s * 0.15 : s * 0.05)}
                text-anchor="middle" dominant-baseline="middle"
                font-size={`${s * 0.5}`} fill="#ffd700"
                style={{ "pointer-events": "none" }}
              >★</text>
            )}
            {props.showNumbers && c.entry && s > 8 && (
              <text
                x={c.cx} y={c.cy + (c.entry.unit.is_general ? s * 0.3 : s * 0.1)}
                text-anchor="middle" dominant-baseline="middle"
                font-size={`${Math.max(7, s * 0.35)}`} font-weight="bold" fill="#fff"
                style={{ "pointer-events": "none" }}
              >{c.entry.count}</text>
            )}
          </g>
        );
      })}
    </svg>
  );
};

export default HexBoard;
