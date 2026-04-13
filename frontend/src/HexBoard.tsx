import { Component, createMemo, For } from "solid-js";
import type { V2UnitSnapshot } from "./v2types";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

const SQRT3 = Math.sqrt(3);

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

interface HexBoardProps {
  terrain: number[];
  units: V2UnitSnapshot[];
  width: number;
  height: number;
  showStrength?: boolean;
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

  // Build unit lookup: "row,col" -> { strongest unit, total count, total strength }
  const unitMap = createMemo(() => {
    const map = new Map<string, { unit: V2UnitSnapshot; count: number; totalStrength: number }>();
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

      {/* Units */}
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
              const color = PLAYER_COLORS[unit.owner % PLAYER_COLORS.length];
              // Scale radius slightly with stack size for visual weight
              const baseRadius = s * 0.45;
              const radius = count > 1 ? Math.min(baseRadius * (1 + Math.log2(count) * 0.15), s * 0.55) : baseRadius;
              const strengthPct = Math.min(unit.strength / 100, 1);
              const opacity = 0.5 + 0.5 * strengthPct;

              return (
                <g>
                  {/* Unit circle */}
                  <circle
                    cx={cx}
                    cy={cy}
                    r={radius}
                    fill={color}
                    fill-opacity={opacity}
                    stroke={unit.engaged ? "#ff0" : unit.is_general ? "#ffd700" : "none"}
                    stroke-width={unit.is_general || unit.engaged ? Math.max(1, s * 0.08) : 0}
                  />
                  {/* General star marker */}
                  {unit.is_general && (
                    <text
                      x={cx}
                      y={cy + s * 0.05}
                      text-anchor="middle"
                      dominant-baseline="middle"
                      font-size={`${s * 0.5}`}
                      fill="#ffd700"
                      style={{ "pointer-events": "none" }}
                    >
                      ★
                    </text>
                  )}
                  {/* Stack count badge */}
                  {count > 1 && s > 8 && (
                    <text
                      x={cx}
                      y={cy + (unit.is_general ? s * 0.35 : s * 0.1)}
                      text-anchor="middle"
                      dominant-baseline="middle"
                      font-size={`${Math.max(7, s * 0.3)}`}
                      font-weight="bold"
                      fill="#fff"
                      style={{ "pointer-events": "none" }}
                    >
                      {count}
                    </text>
                  )}
                  {/* Strength label (only for single units) */}
                  {props.showStrength && count === 1 && !unit.is_general && s > 10 && (
                    <text
                      x={cx}
                      y={cy + s * 0.1}
                      text-anchor="middle"
                      dominant-baseline="middle"
                      font-size={`${Math.max(7, s * 0.35)}`}
                      fill="#fff"
                      style={{ "pointer-events": "none" }}
                    >
                      {Math.round(unit.strength)}
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
