import { Component, createMemo, For } from "solid-js";
import type { V2UnitSnapshot } from "./v2types";

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

// Dim a player color by brightness factor t (0..1)
function playerRgbDim(owner: number, t: number): string {
  const [r, g, b] = parseHex(PLAYER_COLORS[owner % PLAYER_COLORS.length]);
  return `rgb(${Math.round(r * t)},${Math.round(g * t)},${Math.round(b * t)})`;
}

// Log-scaled brightness relative to max stack on board (matches V1)
function stackBrightness(totalStrength: number, maxStrength: number): number {
  if (totalStrength <= 0) return 0.3;
  return 0.3 + 0.7 * Math.log1p(totalStrength) / Math.log1p(Math.max(maxStrength, 1));
}

// Terrain value 0..3 mapped to green intensity
function terrainColor(value: number): string {
  const t = Math.min(value / 3.0, 1.0);
  const r = Math.round(20 + 15 * t);
  const g = Math.round(25 + 40 * t);
  const b = Math.round(20 + 10 * t);
  return `rgb(${r},${g},${b})`;
}

// Pointy-top hex vertices (matches even-r offset layout)
function hexPoints(cx: number, cy: number, size: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 6; i++) {
    const angle = (Math.PI / 180) * (60 * i - 30);
    pts.push(`${cx + size * Math.cos(angle)},${cy + size * Math.sin(angle)}`);
  }
  return pts.join(" ");
}

// Even-r offset to pixel center (pointy-top)
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

  // Build unit lookup: "row,col" -> { representative unit, count, total strength }
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

  // Max total strength across all cells (for relative color grading)
  const maxStackStrength = createMemo(() => {
    let max = 1;
    for (const entry of unitMap().values()) {
      if (entry.totalStrength > max) max = entry.totalStrength;
    }
    return max;
  });

  return (
    <svg
      width={svgWidth()}
      height={svgHeight()}
      viewBox={`${-hexSize()} ${-hexSize()} ${svgWidth()} ${svgHeight()}`}
      style={{ "max-width": "100%", "max-height": "100%" }}
    >
      {/* Terrain hexes */}
      <For each={Array.from({ length: props.height }, (_, r) => r)}>
        {(row) => (
          <For each={Array.from({ length: props.width }, (_, c) => c)}>
            {(col) => {
              const s = hexSize();
              const [cx, cy] = hexCenter(row, col, s);
              const idx = row * props.width + col;
              const tv = props.terrain[idx] ?? 0;
              return (
                <polygon
                  points={hexPoints(cx, cy, s * 0.96)}
                  fill={terrainColor(tv)}
                  stroke="#1a1a2e"
                  stroke-width={Math.max(0.5, s * 0.04)}
                />
              );
            }}
          </For>
        )}
      </For>

      {/* Units — color graded by stack strength relative to max */}
      <For each={Array.from({ length: props.height }, (_, r) => r)}>
        {(row) => (
          <For each={Array.from({ length: props.width }, (_, c) => c)}>
            {(col) => {
              const key = `${row},${col}`;
              const entry = unitMap().get(key);
              if (!entry) return null;

              const { unit, count, totalStrength } = entry;
              const s = hexSize();
              const [cx, cy] = hexCenter(row, col, s);
              const t = stackBrightness(totalStrength, maxStackStrength());
              const fill = playerRgbDim(unit.owner, unit.is_general ? Math.max(t, 0.85) : t);

              return (
                <g>
                  {/* Hex fill with player color */}
                  <polygon
                    points={hexPoints(cx, cy, s * 0.92)}
                    fill={fill}
                    stroke={unit.engaged ? "#ff0" : unit.is_general ? "#ffd700" : "none"}
                    stroke-width={unit.is_general || unit.engaged ? Math.max(1, s * 0.06) : 0}
                  />
                  {/* General star marker */}
                  {unit.is_general && (
                    <text
                      x={cx}
                      y={cy + (props.showNumbers ? -s * 0.15 : s * 0.05)}
                      text-anchor="middle"
                      dominant-baseline="middle"
                      font-size={`${s * 0.5}`}
                      fill="#ffd700"
                      style={{ "pointer-events": "none" }}
                    >
                      ★
                    </text>
                  )}
                  {/* Stack count (toggle with #) */}
                  {props.showNumbers && s > 8 && (
                    <text
                      x={cx}
                      y={cy + (unit.is_general ? s * 0.3 : s * 0.1)}
                      text-anchor="middle"
                      dominant-baseline="middle"
                      font-size={`${Math.max(7, s * 0.35)}`}
                      font-weight="bold"
                      fill="#fff"
                      style={{ "pointer-events": "none", "text-shadow": "0 1px 2px rgba(0,0,0,0.8)" }}
                    >
                      {count}
                    </text>
                  )}
                </g>
              );
            }}
          </For>
        )}
      </For>
    </svg>
  );
};

export default HexBoard;
