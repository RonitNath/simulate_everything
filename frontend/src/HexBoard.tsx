import { Component, createMemo } from "solid-js";
import type { BoardFrameData, BoardStaticData, BiomeName, V2UnitSnapshot } from "./v2types";

export type RenderLayer =
  | "territory" | "roads" | "depots" | "settlements"
  | "convoys" | "destinations";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

const SQRT3 = Math.sqrt(3);

const BIOME_BASE: Record<BiomeName, [number, number, number]> = {
  desert: [180, 160, 90],
  steppe: [140, 150, 80],
  grassland: [80, 140, 60],
  forest: [40, 100, 45],
  jungle: [20, 80, 35],
  tundra: [130, 155, 170],
  mountain: [100, 95, 95],
};

function biomeColor(biome: BiomeName, height: number, isRiver: boolean): string {
  const [br, bg, bb] = BIOME_BASE[biome] ?? BIOME_BASE.grassland;
  const t = 0.6 + 0.4 * height;
  let r = Math.round(br * t);
  let g = Math.round(bg * t);
  let b = Math.round(bb * t);
  if (isRiver) {
    r = Math.round(r * 0.8 + 60 * 0.2);
    g = Math.round(g * 0.8 + 120 * 0.2);
    b = Math.round(b * 0.8 + 200 * 0.2);
  }
  return `rgb(${r},${g},${b})`;
}

function parseHex(hex: string): [number, number, number] {
  return [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ];
}

function playerRgbDim(owner: number, t: number): string {
  const [r, g, b] = parseHex(PLAYER_COLORS[owner % PLAYER_COLORS.length]);
  return `rgb(${Math.round(r * t)},${Math.round(g * t)},${Math.round(b * t)})`;
}

function playerRgba(owner: number, alpha: number): string {
  const [r, g, b] = parseHex(PLAYER_COLORS[owner % PLAYER_COLORS.length]);
  return `rgba(${r},${g},${b},${alpha})`;
}

function stackBrightness(totalStrength: number, maxStrength: number): number {
  if (totalStrength <= 0) return 0.3;
  return 0.3 + 0.7 * Math.log1p(totalStrength) / Math.log1p(Math.max(maxStrength, 1));
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

function hexEdgeVertices(
  cx: number,
  cy: number,
  size: number,
  edge: number,
): [[number, number], [number, number]] {
  const a1 = (Math.PI / 180) * (60 * edge - 30);
  const a2 = (Math.PI / 180) * (60 * ((edge + 1) % 6) - 30);
  return [
    [cx + size * Math.cos(a1), cy + size * Math.sin(a1)],
    [cx + size * Math.cos(a2), cy + size * Math.sin(a2)],
  ];
}

function axialToIdx(q: number, r: number, width: number): number {
  const row = r;
  const col = q + (r - (r & 1)) / 2;
  return row * width + col;
}

interface CellStack {
  unit: V2UnitSnapshot;
  count: number;
  totalStrength: number;
}

const SETTLEMENT_THRESHOLD = 10;

interface CellRender {
  cx: number;
  cy: number;
  basePts: string;
  baseFill: string;
  baseStroke: string;
  baseStrokeWidth: number;
  terrPts?: string;
  terrFill?: string;
  roadLevel: number;
  hasDepot: boolean;
  depotSide: number;
  settlOwner?: number;
  settlPts?: string;
  entry?: CellStack;
  statusIcon?: string;
  statusColor?: string;
  engagements: Array<{ x1: number; y1: number; x2: number; y2: number }>;
}

interface ConvoyRender {
  id: number;
  cx: number;
  cy: number;
  pts: string;
  fill: string;
  label: string;
}

interface DestRender {
  id: number;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  stroke: string;
}

interface GhostRender {
  id: number;
  cx: number;
  cy: number;
  pts: string;
  fill: string;
  opacity: number;
  markerSize: number;
}

interface HexBoardProps {
  staticData: BoardStaticData;
  frameData: BoardFrameData;
  numPlayers: number;
  showNumbers?: boolean;
  layers?: Set<RenderLayer>;
}

const HexBoard: Component<HexBoardProps> = (props) => {
  const hexSize = createMemo(() => {
    const maxW = (window.innerWidth - 320) * 0.88;
    const maxH = (window.innerHeight - 140) * 0.88;
    const gridPixelW = SQRT3 * (props.staticData.width + 0.5);
    const gridPixelH = 1.5 * props.staticData.height + 0.5;
    return Math.max(4, Math.min(maxW / gridPixelW, maxH / gridPixelH));
  });

  const svgWidth = createMemo(
    () => SQRT3 * hexSize() * (props.staticData.width + 0.5) + hexSize(),
  );
  const svgHeight = createMemo(
    () => 1.5 * hexSize() * props.staticData.height + hexSize() * 1.5,
  );

  const staticFills = createMemo(() => {
    const { width, height, biomes, heights, rivers } = props.staticData;
    const result: string[] = new Array(width * height);
    for (let row = 0; row < height; row++) {
      for (let col = 0; col < width; col++) {
        const idx = row * width + col;
        const biome = (biomes[idx] ?? "grassland") as BiomeName;
        const h = heights[idx] ?? 0.5;
        const isRiver = rivers[idx] ?? false;
        result[idx] = biomeColor(biome, h, isRiver);
      }
    }
    return result;
  });

  const renderData = createMemo(() => {
    const s = hexSize();
    const { width, height } = props.staticData;
    const { units, territory, roads, depots, population, convoys, settlements } = props.frameData;
    const baseFills = staticFills();
    const ls: Set<RenderLayer> = props.layers ?? new Set(
      ["territory", "roads", "depots", "settlements", "convoys"],
    );

    const unitMap = new Map<string, CellStack>();
    const ghostRenders: GhostRender[] = [];
    let maxStr = 1;
    for (const u of units) {
      if (u._dead) continue;
      const row = u.r;
      const col = u.q + (u.r - (u.r & 1)) / 2;
      const key = `${row},${col}`;
      const current = unitMap.get(key);
      if (!current) {
        unitMap.set(key, { unit: u, count: 1, totalStrength: u.strength });
      } else {
        current.count++;
        current.totalStrength += u.strength;
        if (u.strength > current.unit.strength) current.unit = u;
      }
      maxStr = Math.max(maxStr, unitMap.get(key)!.totalStrength);
    }

    for (const u of units) {
      if (!u._dead) continue;
      const row = u.r;
      const col = u.q + (u.r - (u.r & 1)) / 2;
      const [cx, cy] = hexCenter(row, col, s);
      const age = props.frameData.tick - (u._deadTick ?? props.frameData.tick);
      const opacity = Math.max(0, 1 - age / 8);
      if (opacity <= 0) continue;
      ghostRenders.push({
        id: u.id,
        cx,
        cy,
        pts: hexPoints(cx, cy, s * 0.82),
        fill: playerRgba(u.owner, 0.45),
        opacity,
        markerSize: s * 0.46,
      });
    }

    const terrMap = new Map<number, number>();
    for (let i = 0; i < territory.length; i++) {
      const owner = territory[i];
      if (owner !== null && owner !== undefined) terrMap.set(i, owner);
    }

    const settlementOwners = new Map<number, number>();
    if (settlements.length > 0) {
      for (const settlement of settlements) {
        settlementOwners.set(axialToIdx(settlement.q, settlement.r, width), settlement.owner);
      }
    } else {
      const popByHexOwner = new Map<string, number>();
      for (const pop of population) {
        const key = `${pop.q},${pop.r},${pop.owner}`;
        popByHexOwner.set(key, (popByHexOwner.get(key) ?? 0) + pop.count);
      }
      for (const [key, count] of popByHexOwner) {
        if (count < SETTLEMENT_THRESHOLD) continue;
        const [q, r, owner] = key.split(",").map((value) => parseInt(value, 10));
        settlementOwners.set(axialToIdx(q, r, width), owner);
      }
    }

    const cells: CellRender[] = [];
    for (let row = 0; row < height; row++) {
      for (let col = 0; col < width; col++) {
        const idx = row * width + col;
        const [cx, cy] = hexCenter(row, col, s);
        const key = `${row},${col}`;
        const entry = unitMap.get(key);
        const basePts = hexPoints(cx, cy, s * 0.96);
        let fill = baseFills[idx] ?? "rgb(80,80,80)";
        let baseStroke = "#1a1a2e";
        let baseStrokeWidth = Math.max(0.5, s * 0.04);

        if (entry) {
          const t = stackBrightness(entry.totalStrength, maxStr);
          fill = playerRgbDim(entry.unit.owner, entry.unit.is_general ? Math.max(t, 0.85) : t);
          if (entry.unit.is_general) {
            baseStroke = "#ffd700";
            baseStrokeWidth = Math.max(1.5, s * 0.08);
          } else if (entry.unit.engaged) {
            baseStroke = "#ff0";
            baseStrokeWidth = Math.max(1, s * 0.06);
          }
        }

        let terrPts: string | undefined;
        let terrFill: string | undefined;
        if (!entry && ls.has("territory")) {
          const owner = terrMap.get(idx);
          if (owner !== undefined) {
            terrPts = hexPoints(cx, cy, s * 0.88);
            terrFill = playerRgba(owner, 0.25);
          }
        }

        let settlOwner: number | undefined;
        let settlPts: string | undefined;
        if (ls.has("settlements")) {
          const owner = settlementOwners.get(idx);
          if (owner !== undefined) {
            settlOwner = owner;
            const ts = s * 0.32;
            const tx = cx;
            const ty = cy - ts * 0.55;
            settlPts = `${tx},${ty - ts} ${tx - ts * 0.85},${ty + ts * 0.5} ${tx + ts * 0.85},${ty + ts * 0.5}`;
          }
        }

        const engagements: CellRender["engagements"] = [];
        if (entry) {
          for (const engagement of entry.unit.engagements ?? []) {
            const visualEdge = (engagement.edge + 5) % 6;
            const [[x1, y1], [x2, y2]] = hexEdgeVertices(cx, cy, s * 0.96, visualEdge);
            engagements.push({ x1, y1, x2, y2 });
          }
        }

        let statusIcon: string | undefined;
        let statusColor: string | undefined;
        if (entry) {
          const unit = entry.unit;
          if (unit.engaged) {
            statusIcon = "⚔";
            statusColor = "#ff6644";
          } else if (unit.destination && unit.move_cooldown === 0) {
            statusIcon = "→";
            statusColor = "#88cc88";
          } else if ((unit.move_cooldown ?? 0) > 0) {
            statusIcon = "◷";
            statusColor = "#aaa";
          } else {
            statusIcon = "·";
            statusColor = "#666";
          }
        }

        cells.push({
          cx,
          cy,
          basePts,
          baseFill: fill,
          baseStroke,
          baseStrokeWidth,
          terrPts,
          terrFill,
          roadLevel: ls.has("roads") ? (roads[idx] ?? 0) : 0,
          hasDepot: ls.has("depots") ? (depots[idx] ?? false) : false,
          depotSide: Math.max(3, s * 0.22),
          settlOwner,
          settlPts,
          entry,
          statusIcon,
          statusColor,
          engagements,
        });
      }
    }

    const convoyRenders: ConvoyRender[] = [];
    if (ls.has("convoys")) {
      for (const convoy of convoys) {
        const row = convoy.r;
        const col = convoy.q + (convoy.r - (convoy.r & 1)) / 2;
        const [cx, cy] = hexCenter(row, col, s);
        const ds = s * 0.3;
        convoyRenders.push({
          id: convoy.id,
          cx,
          cy,
          pts: `${cx},${cy - ds} ${cx + ds},${cy} ${cx},${cy + ds} ${cx - ds},${cy}`,
          fill: playerRgba(convoy.owner, 0.85),
          label: s > 12
            ? (convoy.cargo_type === "Food" ? "F" : convoy.cargo_type === "Material" ? "M" : "S")
            : "",
        });
      }
    }

    const destRenders: DestRender[] = [];
    if (ls.has("destinations")) {
      for (const unit of units) {
        if (!unit.destination) continue;
        const srcRow = unit.r;
        const srcCol = unit.q + (unit.r - (unit.r & 1)) / 2;
        const dstRow = unit.destination.r;
        const dstCol = unit.destination.q + (unit.destination.r - (unit.destination.r & 1)) / 2;
        const [sx, sy] = hexCenter(srcRow, srcCol, s);
        const [dx, dy] = hexCenter(dstRow, dstCol, s);
        destRenders.push({
          id: unit.id,
          x1: sx,
          y1: sy,
          x2: dx,
          y2: dy,
          stroke: playerRgba(unit.owner, 0.4),
        });
      }
    }

    return { cells, convoyRenders, destRenders, ghostRenders, s };
  });

  return (
    <svg
      width={svgWidth()}
      height={svgHeight()}
      viewBox={`${-hexSize()} ${-hexSize()} ${svgWidth()} ${svgHeight()}`}
      style={{ "max-width": "100%", "max-height": "100%" }}
    >
      {renderData().ghostRenders.map((ghost) => (
        <g opacity={ghost.opacity}>
          <polygon
            points={ghost.pts}
            fill={ghost.fill}
            stroke="rgba(255,255,255,0.7)"
            stroke-width={Math.max(0.8, renderData().s * 0.04)}
            stroke-dasharray="3,3"
          />
          <text
            x={ghost.cx}
            y={ghost.cy + 1}
            text-anchor="middle"
            dominant-baseline="middle"
            font-size={`${ghost.markerSize}`}
            font-weight="bold"
            fill="rgba(255,255,255,0.9)"
            style={{ "pointer-events": "none" }}
          >
            ×
          </text>
        </g>
      ))}

      {renderData().cells.map((cell) => {
        const s = renderData().s;
        const showNums = props.showNumbers;
        return (
          <>
            <polygon
              points={cell.basePts}
              fill={cell.baseFill}
              stroke={cell.baseStroke}
              stroke-width={cell.baseStrokeWidth}
            />

            {cell.terrPts && cell.terrFill && (
              <polygon points={cell.terrPts} fill={cell.terrFill} stroke="none" />
            )}

            {cell.roadLevel > 0 && (() => {
              const rc = cell.roadLevel >= 3
                ? "rgba(240,220,160,0.8)"
                : cell.roadLevel >= 2
                  ? "rgba(220,200,140,0.7)"
                  : "rgba(200,200,180,0.6)";
              const arm = s * 0.35;
              return (
                <g>
                  <line
                    x1={cell.cx - arm}
                    y1={cell.cy}
                    x2={cell.cx + arm}
                    y2={cell.cy}
                    stroke={rc}
                    stroke-width={cell.roadLevel}
                    stroke-linecap="round"
                  />
                  <line
                    x1={cell.cx}
                    y1={cell.cy - arm}
                    x2={cell.cx}
                    y2={cell.cy + arm}
                    stroke={rc}
                    stroke-width={cell.roadLevel}
                    stroke-linecap="round"
                  />
                </g>
              );
            })()}

            {cell.hasDepot && (
              <rect
                x={cell.cx - cell.depotSide / 2}
                y={cell.cy - cell.depotSide / 2}
                width={cell.depotSide}
                height={cell.depotSide}
                fill="#c0a000"
                stroke="#8a7200"
                stroke-width={0.5}
              />
            )}

            {cell.settlPts !== undefined && cell.settlOwner !== undefined && (
              <polygon
                points={cell.settlPts}
                fill={playerRgba(cell.settlOwner, 0.9)}
                stroke="#fff"
                stroke-width={0.5}
              />
            )}

            {cell.entry && cell.entry.unit.is_general && (
              <text
                x={cell.cx}
                y={cell.cy + (showNums ? -s * 0.15 : s * 0.05)}
                text-anchor="middle"
                dominant-baseline="middle"
                font-size={`${s * 0.5}`}
                fill="#ffd700"
                style={{ "pointer-events": "none" }}
              >
                ★
              </text>
            )}

            {cell.entry && showNums && s > 8 && (
              <text
                x={cell.cx}
                y={cell.cy + (cell.entry.unit.is_general ? s * 0.3 : s * 0.1)}
                text-anchor="middle"
                dominant-baseline="middle"
                font-size={`${Math.max(7, s * 0.35)}`}
                font-weight="bold"
                fill="#fff"
                style={{ "pointer-events": "none" }}
              >
                {cell.entry.count}
              </text>
            )}

            {cell.entry && s > 6 && cell.statusIcon && cell.statusIcon !== "·" && (
              <g>
                <circle
                  cx={cell.cx + s * 0.38}
                  cy={cell.cy - s * 0.32}
                  r={s * 0.12}
                  fill="rgba(255,255,255,0.85)"
                />
                <text
                  x={cell.cx + s * 0.38}
                  y={cell.cy - s * 0.32}
                  text-anchor="middle"
                  dominant-baseline="middle"
                  font-size={`${Math.max(6, s * 0.25)}`}
                  fill={cell.statusColor ?? "#888"}
                  style={{ "pointer-events": "none" }}
                >
                  {cell.statusIcon}
                </text>
              </g>
            )}

            {cell.engagements.map((engagement) => (
              <line
                x1={engagement.x1}
                y1={engagement.y1}
                x2={engagement.x2}
                y2={engagement.y2}
                stroke="#ff6644"
                stroke-width={Math.max(2, s * 0.1)}
                stroke-linecap="round"
              />
            ))}
          </>
        );
      })}

      {renderData().convoyRenders.map((convoy) => (
        <>
          <polygon points={convoy.pts} fill={convoy.fill} stroke="#fff" stroke-width={0.5} />
          {convoy.label && (
            <text
              x={convoy.cx}
              y={convoy.cy + 1}
              text-anchor="middle"
              dominant-baseline="middle"
              font-size={`${Math.max(6, renderData().s * 0.22)}`}
              font-weight="bold"
              fill="#fff"
              style={{ "pointer-events": "none" }}
            >
              {convoy.label}
            </text>
          )}
        </>
      ))}

      {renderData().destRenders.map((dest) => (
        <line
          x1={dest.x1}
          y1={dest.y1}
          x2={dest.x2}
          y2={dest.y2}
          stroke={dest.stroke}
          stroke-width={1}
          stroke-dasharray="4,4"
        />
      ))}
    </svg>
  );
};

export default HexBoard;
