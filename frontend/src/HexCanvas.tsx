import { Component, createEffect, onCleanup, onMount } from "solid-js";
import { Application, Container, Graphics, Text, TextStyle } from "pixi.js";
import type { BoardStaticData, BoardFrameData, BiomeName, V2UnitSnapshot } from "./v2types";

export type RenderLayer =
  | "territory" | "roads" | "depots" | "settlements"
  | "convoys" | "destinations";

interface HexCanvasProps {
  staticData: BoardStaticData;
  frameData: BoardFrameData;
  numPlayers: number;
  showNumbers?: boolean;
  layers?: Set<RenderLayer>;
}

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

const SQRT3 = Math.sqrt(3);
const HEX_SIZE = 20;

const BIOME_BASE: Record<BiomeName, [number, number, number]> = {
  desert: [180, 160, 90],
  steppe: [140, 150, 80],
  grassland: [80, 140, 60],
  forest: [40, 100, 45],
  jungle: [20, 80, 35],
  tundra: [130, 155, 170],
  mountain: [100, 95, 95],
};

function parseHexColor(hex: string): [number, number, number] {
  return [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ];
}

function rgbToNum(r: number, g: number, b: number): number {
  return (r << 16) | (g << 8) | b;
}

function biomeColorNum(biome: BiomeName, height: number, isRiver: boolean): number {
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
  return rgbToNum(r, g, b);
}

function playerColorNum(owner: number): number {
  const [r, g, b] = parseHexColor(PLAYER_COLORS[owner % PLAYER_COLORS.length]);
  return rgbToNum(r, g, b);
}

function playerColorDimNum(owner: number, t: number): number {
  const [r, g, b] = parseHexColor(PLAYER_COLORS[owner % PLAYER_COLORS.length]);
  return rgbToNum(Math.round(r * t), Math.round(g * t), Math.round(b * t));
}

function stackBrightness(totalStrength: number, maxStrength: number): number {
  if (totalStrength <= 0) return 0.5;
  return 0.5 + 0.5 * Math.log1p(totalStrength) / Math.log1p(Math.max(maxStrength, 1));
}

function hexCenter(row: number, col: number, size: number): [number, number] {
  const x = SQRT3 * size * (col + 0.5 * (row & 1));
  const y = 1.5 * size * row;
  return [x, y];
}

function drawHexPath(g: Graphics, cx: number, cy: number, radius: number): void {
  for (let i = 0; i < 6; i++) {
    const angle = (Math.PI / 180) * (60 * i - 30);
    const px = cx + radius * Math.cos(angle);
    const py = cy + radius * Math.sin(angle);
    if (i === 0) {
      g.moveTo(px, py);
    } else {
      g.lineTo(px, py);
    }
  }
  g.closePath();
}

function hexEdgeVertices(
  cx: number, cy: number, size: number, edge: number,
): [[number, number], [number, number]] {
  const a1 = (Math.PI / 180) * (60 * edge - 30);
  const a2 = (Math.PI / 180) * (60 * ((edge + 1) % 6) - 30);
  return [
    [cx + size * Math.cos(a1), cy + size * Math.sin(a1)],
    [cx + size * Math.cos(a2), cy + size * Math.sin(a2)],
  ];
}

const EVEN_NEIGHBORS: [number, number][] = [[-1, -1], [-1, 0], [0, 1], [1, 0], [1, -1], [0, -1]];
const ODD_NEIGHBORS: [number, number][] = [[-1, 0], [-1, 1], [0, 1], [1, 1], [1, 0], [0, -1]];

interface CellStack {
  unit: V2UnitSnapshot;
  count: number;
  totalStrength: number;
}

const HexCanvas: Component<HexCanvasProps> = (props) => {
  let containerDiv: HTMLDivElement | undefined;
  let canvasDiv: HTMLDivElement | undefined;

  let app: Application | null = null;
  let world: Container | null = null;
  let terrainLayer: Graphics | null = null;
  let dynamicLayer: Container | null = null;

  // Camera state — not signals, PixiJS manages rendering
  let scale = 1.0;
  let offsetX = 0;
  let offsetY = 0;
  let isDragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartOffsetX = 0;
  let dragStartOffsetY = 0;

  function applyCamera() {
    if (!world) return;
    world.scale.set(scale);
    world.position.set(offsetX, offsetY);
  }

  function drawTerrain(staticData: BoardStaticData) {
    if (!terrainLayer) return;
    terrainLayer.clear();

    const { width, height, biomes, heights, rivers } = staticData;
    const size = HEX_SIZE;

    for (let row = 0; row < height; row++) {
      for (let col = 0; col < width; col++) {
        const idx = row * width + col;
        const biome = (biomes[idx] ?? "grassland") as BiomeName;
        const h = heights[idx] ?? 0.5;
        const isRiver = rivers[idx] ?? false;
        const color = biomeColorNum(biome, h, isRiver);
        const [cx, cy] = hexCenter(row, col, size);
        drawHexPath(terrainLayer, cx, cy, size * 0.96);
        terrainLayer.fill({ color });
        terrainLayer.stroke({ color: 0x1a1a2e, width: Math.max(0.5, size * 0.04) });
      }
    }
  }

  function drawDynamic(staticData: BoardStaticData, frameData: BoardFrameData, showNumbers: boolean, layers: Set<RenderLayer>) {
    if (!dynamicLayer) return;
    dynamicLayer.removeChildren();

    const { width, height } = staticData;
    const { units, territory, roads, depots, population, settlements } = frameData;
    const size = HEX_SIZE;

    // Build unit map
    const unitMap = new Map<string, CellStack>();
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

    // Build settlement map
    const settlementMap = new Map<number, { owner: number; type: "Farm" | "Village" | "City" }>();
    if (settlements.length > 0) {
      for (const s of settlements) {
        const type = s.settlement_type ?? "Village";
        const r = s.r;
        const col = s.q + (s.r - (s.r & 1)) / 2;
        const idx = r * width + col;
        settlementMap.set(idx, { owner: s.owner, type });
      }
    } else {
      const popByHexOwner = new Map<string, number>();
      for (const pop of population) {
        const key = `${pop.q},${pop.r},${pop.owner}`;
        popByHexOwner.set(key, (popByHexOwner.get(key) ?? 0) + pop.count);
      }
      for (const [key, count] of popByHexOwner) {
        if (count < 10) continue;
        const [q, r, owner] = key.split(",").map((v) => parseInt(v, 10));
        const row2 = r;
        const col2 = q + (r - (r & 1)) / 2;
        settlementMap.set(row2 * width + col2, { owner, type: "Village" });
      }
    }

    // Territory overlay layer
    const terrOverlay = new Graphics();
    if (layers.has("territory")) {
      for (let row = 0; row < height; row++) {
        for (let col = 0; col < width; col++) {
          const idx = row * width + col;
          const key = `${row},${col}`;
          const hasUnit = unitMap.has(key);
          if (hasUnit) continue;
          const owner = territory[idx];
          if (owner === null || owner === undefined) continue;
          const [cx, cy] = hexCenter(row, col, size);
          drawHexPath(terrOverlay, cx, cy, size * 0.88);
          terrOverlay.fill({ color: playerColorNum(owner), alpha: 0.35 });
        }
      }
    }
    dynamicLayer.addChild(terrOverlay);

    // Roads layer
    const roadsGfx = new Graphics();
    if (layers.has("roads")) {
      for (let row = 0; row < height; row++) {
        for (let col = 0; col < width; col++) {
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
            roadsGfx.moveTo(cx, cy);
            roadsGfx.lineTo((cx + nx) / 2, (cy + ny) / 2);
            roadsGfx.stroke({ color: roadColor, alpha: roadAlpha, width: Math.max(1.5, level * 0.8 + size * 0.04), cap: "round" });
          }
        }
      }
    }
    dynamicLayer.addChild(roadsGfx);

    // Depots layer
    const depotsGfx = new Graphics();
    if (layers.has("depots")) {
      for (let row = 0; row < height; row++) {
        for (let col = 0; col < width; col++) {
          const idx = row * width + col;
          if (!(depots[idx] ?? false)) continue;
          const [cx, cy] = hexCenter(row, col, size);
          const side = Math.max(3, size * 0.22);
          depotsGfx.rect(cx - side / 2, cy - side / 2, side, side);
          depotsGfx.fill({ color: 0xc0a000 });
          depotsGfx.stroke({ color: 0x8a7200, width: 0.5 });
        }
      }
    }
    dynamicLayer.addChild(depotsGfx);

    // Settlements layer
    const settlGfx = new Graphics();
    if (layers.has("settlements")) {
      for (const [idx, info] of settlementMap) {
        const row = Math.floor(idx / width);
        const col = idx % width;
        const [cx, cy] = hexCenter(row, col, size);
        const color = playerColorNum(info.owner);

        if (info.type === "Farm") {
          // Background plate
          settlGfx.circle(cx, cy, size * 0.3);
          settlGfx.fill({ color, alpha: 0.3 });
          // Farm circle
          settlGfx.circle(cx, cy, size * 0.25);
          settlGfx.fill({ color, alpha: 0.8 });
          settlGfx.stroke({ color: 0xffffff, width: 0.5 });
        } else if (info.type === "Village") {
          const hs = size * 0.45;
          // Background halo
          settlGfx.circle(cx, cy, size * 0.45);
          settlGfx.fill({ color, alpha: 0.3 });
          // House shape
          const bx = cx - hs;
          const by = cy - hs * 0.1;
          const bw = hs * 2;
          const bh = hs * 1.2;
          const peakY = cy - hs * 1.1;
          settlGfx.moveTo(bx, by);
          settlGfx.lineTo(bx, by + bh);
          settlGfx.lineTo(bx + bw, by + bh);
          settlGfx.lineTo(bx + bw, by);
          settlGfx.lineTo(cx, peakY);
          settlGfx.closePath();
          settlGfx.fill({ color, alpha: 0.9 });
          settlGfx.stroke({ color: 0xffffff, width: 0.5 });
        } else {
          // City
          const w = size * 0.4;
          const h = size * 0.45;
          // Background halo
          settlGfx.circle(cx, cy, size * 0.55);
          settlGfx.fill({ color, alpha: 0.35 });
          // Crenellated tower
          settlGfx.moveTo(cx - w, cy + h);
          settlGfx.lineTo(cx - w, cy - h);
          settlGfx.lineTo(cx - w * 0.6, cy - h);
          settlGfx.lineTo(cx - w * 0.6, cy - h * 1.3);
          settlGfx.lineTo(cx - w * 0.2, cy - h * 1.3);
          settlGfx.lineTo(cx - w * 0.2, cy - h);
          settlGfx.lineTo(cx + w * 0.2, cy - h);
          settlGfx.lineTo(cx + w * 0.2, cy - h * 1.3);
          settlGfx.lineTo(cx + w * 0.6, cy - h * 1.3);
          settlGfx.lineTo(cx + w * 0.6, cy - h);
          settlGfx.lineTo(cx + w, cy - h);
          settlGfx.lineTo(cx + w, cy + h);
          settlGfx.closePath();
          settlGfx.fill({ color, alpha: 0.95 });
          settlGfx.stroke({ color: 0xffffff, width: 1 });
        }
      }
    }
    dynamicLayer.addChild(settlGfx);

    // Units layer
    const unitsGfx = new Graphics();
    for (const [key, entry] of unitMap) {
      const [rowStr, colStr] = key.split(",");
      const row = parseInt(rowStr, 10);
      const col = parseInt(colStr, 10);
      const [cx, cy] = hexCenter(row, col, size);
      const t = stackBrightness(entry.totalStrength, maxStr);
      const color = playerColorDimNum(entry.unit.owner, t);

      drawHexPath(unitsGfx, cx, cy, size * 0.96);
      unitsGfx.fill({ color });
      if (entry.unit.engaged) {
        unitsGfx.stroke({ color: 0xff6644, width: Math.max(1, size * 0.06) });
      } else {
        unitsGfx.stroke({ color: 0x1a1a2e, width: Math.max(0.5, size * 0.04) });
      }
    }
    dynamicLayer.addChild(unitsGfx);

    // Unit count text labels
    if (showNumbers) {
      const labelStyle = new TextStyle({ fontSize: Math.max(7, size * 0.35), fill: 0xffffff, fontWeight: "bold" });
      for (const [key, entry] of unitMap) {
        const [rowStr, colStr] = key.split(",");
        const row = parseInt(rowStr, 10);
        const col = parseInt(colStr, 10);
        const [cx, cy] = hexCenter(row, col, size);
        const label = new Text({ text: String(entry.count), style: labelStyle });
        label.anchor.set(0.5, 0.5);
        label.position.set(cx, cy + size * 0.1);
        dynamicLayer.addChild(label);
      }
    }

    // Engagement edges layer
    const engageGfx = new Graphics();
    for (const [key, entry] of unitMap) {
      const [rowStr, colStr] = key.split(",");
      const row = parseInt(rowStr, 10);
      const col = parseInt(colStr, 10);
      const [cx, cy] = hexCenter(row, col, size);
      for (const engagement of entry.unit.engagements ?? []) {
        const visualEdge = (engagement.edge + 5) % 6;
        const [[x1, y1], [x2, y2]] = hexEdgeVertices(cx, cy, size * 0.96, visualEdge);
        engageGfx.moveTo(x1, y1);
        engageGfx.lineTo(x2, y2);
        engageGfx.stroke({ color: 0xff6644, width: Math.max(2, size * 0.1), cap: "round" });
      }
    }
    dynamicLayer.addChild(engageGfx);
  }

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    if (!canvasDiv) return;
    const rect = canvasDiv.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;
    const newScale = Math.min(5.0, Math.max(0.2, scale * zoomFactor));

    // Zoom toward cursor
    offsetX = mouseX - (mouseX - offsetX) * (newScale / scale);
    offsetY = mouseY - (mouseY - offsetY) * (newScale / scale);
    scale = newScale;
    applyCamera();
  }

  function handlePointerDown(e: PointerEvent) {
    isDragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    dragStartOffsetX = offsetX;
    dragStartOffsetY = offsetY;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function handlePointerMove(e: PointerEvent) {
    if (!isDragging) return;
    offsetX = dragStartOffsetX + (e.clientX - dragStartX);
    offsetY = dragStartOffsetY + (e.clientY - dragStartY);
    applyCamera();
  }

  function handlePointerUp(e: PointerEvent) {
    isDragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  }

  onMount(async () => {
    if (!canvasDiv || !containerDiv) return;

    app = new Application();
    await app.init({
      background: 0x0a0a0f,
      resizeTo: canvasDiv,
      antialias: true,
    });

    canvasDiv.appendChild(app.canvas);

    world = new Container();
    app.stage.addChild(world);

    terrainLayer = new Graphics();
    world.addChild(terrainLayer);

    dynamicLayer = new Container();
    world.addChild(dynamicLayer);

    // Center initial view on board
    const { width, height } = props.staticData;
    const boardPixelW = SQRT3 * HEX_SIZE * (width + 0.5);
    const boardPixelH = 1.5 * HEX_SIZE * height;
    const canvasW = canvasDiv.clientWidth || 800;
    const canvasH = canvasDiv.clientHeight || 600;
    offsetX = (canvasW - boardPixelW) / 2;
    offsetY = (canvasH - boardPixelH) / 2;
    applyCamera();

    // Event listeners on canvas element
    const canvas = app.canvas as HTMLCanvasElement;
    canvas.addEventListener("wheel", handleWheel, { passive: false });
    canvas.addEventListener("pointerdown", handlePointerDown);
    canvas.addEventListener("pointermove", handlePointerMove);
    canvas.addEventListener("pointerup", handlePointerUp);

    // Initial draw
    const ls = props.layers ?? new Set<RenderLayer>(["territory", "roads", "depots", "settlements", "convoys"]);
    drawTerrain(props.staticData);
    drawDynamic(props.staticData, props.frameData, props.showNumbers ?? false, ls);
  });

  // Redraw terrain when staticData changes
  createEffect(() => {
    const sd = props.staticData;
    if (!terrainLayer || !app) return;
    drawTerrain(sd);
    const ls = props.layers ?? new Set<RenderLayer>(["territory", "roads", "depots", "settlements", "convoys"]);
    drawDynamic(sd, props.frameData, props.showNumbers ?? false, ls);
  });

  // Redraw dynamic layers when frameData changes
  createEffect(() => {
    const fd = props.frameData;
    const ls = props.layers ?? new Set<RenderLayer>(["territory", "roads", "depots", "settlements", "convoys"]);
    const sn = props.showNumbers ?? false;
    if (!dynamicLayer || !app) return;
    drawDynamic(props.staticData, fd, sn, ls);
  });

  onCleanup(() => {
    if (app) {
      const canvas = app.canvas as HTMLCanvasElement;
      canvas.removeEventListener("wheel", handleWheel);
      canvas.removeEventListener("pointerdown", handlePointerDown);
      canvas.removeEventListener("pointermove", handlePointerMove);
      canvas.removeEventListener("pointerup", handlePointerUp);
      app.destroy(true, { children: true });
      app = null;
    }
    world = null;
    terrainLayer = null;
    dynamicLayer = null;
  });

  return (
    <div
      ref={containerDiv}
      style={{ width: "100%", height: "100%", position: "relative", overflow: "hidden" }}
    >
      <div
        ref={canvasDiv}
        style={{ width: "100%", height: "100%", display: "block" }}
      />
    </div>
  );
};

export default HexCanvas;
