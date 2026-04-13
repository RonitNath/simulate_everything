import { Component, createMemo } from "solid-js";
import type { BoardStaticData, BoardFrameData, BiomeName, V2UnitSnapshot } from "./v2types";

export type RenderLayer =
  | "territory" | "roads" | "depots" | "settlements"
  | "convoys" | "destinations";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

const SQRT3 = Math.sqrt(3);

// Each biome's base RGB for colorization
const BIOME_BASE: Record<BiomeName, [number, number, number]> = {
  desert:    [180, 160,  90],
  steppe:    [140, 150,  80],
  grassland: [ 80, 140,  60],
  forest:    [ 40, 100,  45],
  jungle:    [ 20,  80,  35],
  tundra:    [130, 155, 170],
  mountain:  [100,  95,  95],
};

function biomeColor(biome: BiomeName, height: number, isRiver: boolean): string {
  const [br, bg, bb] = BIOME_BASE[biome] ?? BIOME_BASE.grassland;
  const t = 0.6 + 0.4 * height;
  let r = Math.round(br * t), g = Math.round(bg * t), b = Math.round(bb * t);
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
  cx: number, cy: number, size: number, edge: number
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

// Precomputed cell data used in the flat SVG render
interface CellRender {
  idx: number;
  row: number;
  col: number;
  cx: number;
  cy: number;
  // Base hex
  basePts: string;
  baseFill: string;
  baseStroke: string;
  baseStrokeWidth: number;
  // Optional territory overlay
  terrPts?: string;
  terrFill?: string;
  // Optional road
  roadLevel: number;
  // Optional depot
  hasDepot: boolean;
  depotSide: number;
  // Optional settlement
  settlOwner?: number;
  settlPts?: string;
  // Optional unit
  entry?: CellStack;
  // Engagements for edge highlights
  engagements: Array<{ edge: number; x1: number; y1: number; x2: number; y2: number }>;
}

// Convoy render data
interface ConvoyRender {
  id: number;
  cx: number;
  cy: number;
  pts: string;
  fill: string;
  label: string;
}

// Destination line data
interface DestRender {
  id: number;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  stroke: string;
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

  const svgWidth = createMemo(() =>
    SQRT3 * hexSize() * (props.staticData.width + 0.5) + hexSize()
  );
  const svgHeight = createMemo(() =>
    1.5 * hexSize() * props.staticData.height + hexSize() * 1.5
  );

  // Static memo: biome fill color per cell. Reruns only when staticData changes.
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

  // Full render memo: recomputes on frame changes or hex size changes.
  const renderData = createMemo(() => {
    const s = hexSize();
    const { width, height } = props.staticData;
    const { units, territory, roads, depots, population, convoys } = props.frameData;
    const baseFills = staticFills();
    const ls: Set<RenderLayer> = props.layers ?? new Set(
      ["territory", "roads", "depots", "settlements", "convoys"]
    );

    // Build unit map (offset key)
    const unitMap = new Map<string, CellStack>();
    let maxStr = 1;
    for (const u of units) {
      const row = u.r;
      const col = u.q + (u.r - (u.r & 1)) / 2;
      const k = `${row},${col}`;
      const ex = unitMap.get(k);
      if (!ex) {
        unitMap.set(k, { unit: u, count: 1, totalStrength: u.strength });
      } else {
        ex.count++;
        ex.totalStrength += u.strength;
        if (u.strength > ex.unit.strength) ex.unit = u;
      }
      const cur = unitMap.get(k)!.totalStrength;
      if (cur > maxStr) maxStr = cur;
    }

    // Territory map: idx -> owner
    const terrMap = new Map<number, number>();
    for (let i = 0; i < territory.length; i++) {
      const owner = territory[i];
      if (owner !== null && owner !== undefined) terrMap.set(i, owner);
    }

    // Settlement detection
    const hexOwnerPop = new Map<string, number>();
    for (const p of population) {
      const k = `${p.q},${p.r},${p.owner}`;
      hexOwnerPop.set(k, (hexOwnerPop.get(k) ?? 0) + p.count);
    }
    const settlMap = new Map<number, number>();
    for (const [k, count] of hexOwnerPop) {
      if (count >= SETTLEMENT_THRESHOLD) {
        const parts = k.split(",");
        const q = parseInt(parts[0]), r = parseInt(parts[1]), owner = parseInt(parts[2]);
        settlMap.set(axialToIdx(q, r, width), owner);
      }
    }

    // Build cell render list
    const cells: CellRender[] = [];

    for (let row = 0; row < height; row++) {
      for (let col = 0; col < width; col++) {
        const idx = row * width + col;
        const [cx, cy] = hexCenter(row, col, s);
        const k = `${row},${col}`;
        const entry = unitMap.get(k);
        const baseFill = baseFills[idx] ?? "rgb(80,80,80)";
        const basePts = hexPoints(cx, cy, s * 0.96);
        let baseStroke = "#1a1a2e";
        let baseStrokeWidth = Math.max(0.5, s * 0.04);
        let fill = baseFill;

        if (entry) {
          const { unit, totalStrength } = entry;
          const t = stackBrightness(totalStrength, maxStr);
          fill = playerRgbDim(unit.owner, unit.is_general ? Math.max(t, 0.85) : t);
          if (unit.is_general) {
            baseStroke = "#ffd700";
            baseStrokeWidth = Math.max(1.5, s * 0.08);
          } else if (unit.engaged) {
            baseStroke = "#ff0";
            baseStrokeWidth = Math.max(1, s * 0.06);
          }
        }

        // Territory overlay
        let terrPts: string | undefined;
        let terrFill: string | undefined;
        if (!entry && ls.has("territory")) {
          const owner = terrMap.get(idx);
          if (owner !== undefined) {
            terrPts = hexPoints(cx, cy, s * 0.88);
            terrFill = playerRgba(owner, 0.25);
          }
        }

        // Settlement
        let settlOwner: number | undefined;
        let settlPts: string | undefined;
        if (ls.has("settlements")) {
          const so = settlMap.get(idx);
          if (so !== undefined) {
            settlOwner = so;
            const ts = s * 0.32;
            const tx = cx, ty = cy - ts * 0.55;
            settlPts = `${tx},${ty - ts} ${tx - ts * 0.85},${ty + ts * 0.5} ${tx + ts * 0.85},${ty + ts * 0.5}`;
          }
        }

        // Engagement edge highlights
        const engagements: CellRender["engagements"] = [];
        if (entry) {
          for (const eng of entry.unit.engagements ?? []) {
            const [[x1, y1], [x2, y2]] = hexEdgeVertices(cx, cy, s * 0.96, eng.edge);
            engagements.push({ edge: eng.edge, x1, y1, x2, y2 });
          }
        }

        cells.push({
          idx, row, col, cx, cy,
          basePts, baseFill: fill, baseStroke, baseStrokeWidth,
          terrPts, terrFill,
          roadLevel: ls.has("roads") ? (roads[idx] ?? 0) : 0,
          hasDepot: ls.has("depots") ? (depots[idx] ?? false) : false,
          depotSide: Math.max(3, s * 0.22),
          settlOwner, settlPts,
          entry,
          engagements,
        });
      }
    }

    // Convoy renders
    const convoyRenders: ConvoyRender[] = [];
    if (ls.has("convoys")) {
      for (const c of convoys) {
        const row = c.r;
        const col = c.q + (c.r - (c.r & 1)) / 2;
        const [cx, cy] = hexCenter(row, col, s);
        const ds = s * 0.3;
        convoyRenders.push({
          id: c.id,
          cx, cy,
          pts: `${cx},${cy - ds} ${cx + ds},${cy} ${cx},${cy + ds} ${cx - ds},${cy}`,
          fill: playerRgba(c.owner, 0.85),
          label: s > 12 ? (c.cargo_type === "Food" ? "F" : c.cargo_type === "Material" ? "M" : "S") : "",
        });
      }
    }

    // Destination line renders
    const destRenders: DestRender[] = [];
    if (ls.has("destinations")) {
      for (const u of units) {
        if (!u.destination) continue;
        const srcRow = u.r;
        const srcCol = u.q + (u.r - (u.r & 1)) / 2;
        const dstRow = u.destination.r;
        const dstCol = u.destination.q + (u.destination.r - (u.destination.r & 1)) / 2;
        const [sx, sy] = hexCenter(srcRow, srcCol, s);
        const [dx, dy] = hexCenter(dstRow, dstCol, s);
        destRenders.push({
          id: u.id,
          x1: sx, y1: sy, x2: dx, y2: dy,
          stroke: playerRgba(u.owner, 0.4),
        });
      }
    }

    return { cells, convoyRenders, destRenders, s };
  });

  return (
    <svg
      width={svgWidth()}
      height={svgHeight()}
      viewBox={`${-hexSize()} ${-hexSize()} ${svgWidth()} ${svgHeight()}`}
      style={{ "max-width": "100%", "max-height": "100%" }}
    >
      {renderData().cells.map((c) => {
        const s = renderData().s;
        const showNums = props.showNumbers;
        return (
          <>
            {/* Base hex */}
            <polygon points={c.basePts} fill={c.baseFill}
              stroke={c.baseStroke} stroke-width={c.baseStrokeWidth} />

            {/* Territory overlay */}
            {c.terrPts && c.terrFill && (
              <polygon points={c.terrPts} fill={c.terrFill} stroke="none" />
            )}

            {/* Roads */}
            {c.roadLevel > 0 && (() => {
              const lw = c.roadLevel;
              const rc = c.roadLevel >= 3 ? "rgba(240,220,160,0.8)"
                : c.roadLevel >= 2 ? "rgba(220,200,140,0.7)"
                : "rgba(200,200,180,0.6)";
              const arm = s * 0.35;
              return (
                <g>
                  <line x1={c.cx - arm} y1={c.cy} x2={c.cx + arm} y2={c.cy}
                    stroke={rc} stroke-width={lw} stroke-linecap="round" />
                  <line x1={c.cx} y1={c.cy - arm} x2={c.cx} y2={c.cy + arm}
                    stroke={rc} stroke-width={lw} stroke-linecap="round" />
                </g>
              );
            })()}

            {/* Depot */}
            {c.hasDepot && (
              <rect
                x={c.cx - c.depotSide / 2} y={c.cy - c.depotSide / 2}
                width={c.depotSide} height={c.depotSide}
                fill="#c0a000" stroke="#8a7200" stroke-width={0.5} />
            )}

            {/* Settlement */}
            {c.settlPts !== undefined && c.settlOwner !== undefined && (
              <polygon points={c.settlPts}
                fill={playerRgba(c.settlOwner, 0.9)}
                stroke="#fff" stroke-width={0.5} />
            )}

            {/* Unit overlays */}
            {c.entry && c.entry.unit.is_general && (
              <text
                x={c.cx}
                y={c.cy + (showNums ? -s * 0.15 : s * 0.05)}
                text-anchor="middle" dominant-baseline="middle"
                font-size={`${s * 0.5}`} fill="#ffd700"
                style={{ "pointer-events": "none" }}
              >★</text>
            )}
            {c.entry && showNums && s > 8 && (
              <text
                x={c.cx}
                y={c.cy + (c.entry.unit.is_general ? s * 0.3 : s * 0.1)}
                text-anchor="middle" dominant-baseline="middle"
                font-size={`${Math.max(7, s * 0.35)}`} font-weight="bold" fill="#fff"
                style={{ "pointer-events": "none" }}
              >{c.entry.count}</text>
            )}

            {/* Engagement edge highlights */}
            {c.engagements.map((eng) => (
              <line
                x1={eng.x1} y1={eng.y1} x2={eng.x2} y2={eng.y2}
                stroke="#ff6644" stroke-width={Math.max(2, s * 0.1)}
                stroke-linecap="round" />
            ))}
          </>
        );
      })}

      {/* Convoys */}
      {renderData().convoyRenders.map((c) => (
        <>
          <polygon points={c.pts} fill={c.fill} stroke="#fff" stroke-width={0.5} />
          {c.label && (
            <text
              x={c.cx} y={c.cy + 1}
              text-anchor="middle" dominant-baseline="middle"
              font-size={`${Math.max(6, renderData().s * 0.22)}`}
              font-weight="bold" fill="#fff"
              style={{ "pointer-events": "none" }}
            >{c.label}</text>
          )}
        </>
      ))}

      {/* Destination lines */}
      {renderData().destRenders.map((d) => (
        <line
          x1={d.x1} y1={d.y1} x2={d.x2} y2={d.y2}
          stroke={d.stroke} stroke-width={1} stroke-dasharray="4,4" />
      ))}
    </svg>
  );
};

export default HexBoard;
