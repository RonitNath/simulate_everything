import { Component, createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { Application, Container, Graphics } from "pixi.js";
import type { BoardStaticData, BoardFrameData, BiomeName, SpectatorEntity } from "./v2types";

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

// --- Entity classification ---

type EntityKind = "combatant" | "structure" | "resource" | "civilian";

function classifyEntity(e: SpectatorEntity): EntityKind {
  if (e.structure_type) return "structure";
  if (e.resource_type) return "resource";
  if (e.health != null) return "combatant";
  return "civilian";
}

interface HexStack {
  row: number;
  col: number;
  entities: SpectatorEntity[];
  combatants: SpectatorEntity[];
  structures: SpectatorEntity[];
  resources: SpectatorEntity[];
  civilians: SpectatorEntity[];
  totalHealth: number;
  anyEngaged: boolean;
  dominantOwner: number | null;
}

function buildHexStacks(entities: SpectatorEntity[]): Map<string, HexStack> {
  const stacks = new Map<string, HexStack>();
  for (const e of entities) {
    // Axial (q,r) → offset even-r (row,col)
    const row = e.r;
    const col = e.q + (e.r - (e.r & 1)) / 2;
    const key = `${row},${col}`;
    let stack = stacks.get(key);
    if (!stack) {
      stack = {
        row, col, entities: [], combatants: [], structures: [],
        resources: [], civilians: [], totalHealth: 0,
        anyEngaged: false, dominantOwner: null,
      };
      stacks.set(key, stack);
    }
    stack.entities.push(e);
    const kind = classifyEntity(e);
    if (kind === "combatant") {
      stack.combatants.push(e);
      stack.totalHealth += e.health ?? 0;
      if (e.engaged) stack.anyEngaged = true;
    } else if (kind === "structure") {
      stack.structures.push(e);
    } else if (kind === "resource") {
      stack.resources.push(e);
    } else {
      stack.civilians.push(e);
    }
    // Dominant owner = most common non-null owner
    if (e.owner != null) stack.dominantOwner = e.owner;
  }
  return stacks;
}

// --- Color utilities ---

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

function healthBrightness(totalHealth: number, maxHealth: number): number {
  if (totalHealth <= 0) return 0.5;
  return 0.5 + 0.5 * Math.log1p(totalHealth) / Math.log1p(Math.max(maxHealth, 1));
}

// --- Hex geometry ---

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
    if (i === 0) g.moveTo(px, py);
    else g.lineTo(px, py);
  }
  g.closePath();
}

const EVEN_NEIGHBORS: [number, number][] = [[-1, -1], [-1, 0], [0, 1], [1, 0], [1, -1], [0, -1]];
const ODD_NEIGHBORS: [number, number][] = [[-1, 0], [-1, 1], [0, 1], [1, 1], [1, 0], [0, -1]];

// Reverse hex lookup: screen coords → (row, col)
function pixelToHex(wx: number, wy: number, size: number): [number, number] {
  // Approximate row from y
  const rowApprox = wy / (1.5 * size);
  const row = Math.round(rowApprox);
  const col = Math.round(wx / (SQRT3 * size) - 0.5 * (row & 1));

  // Check nearest candidates for closest center
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

// --- Tooltip types ---

interface TooltipData {
  screenX: number;
  screenY: number;
  row: number;
  col: number;
  stack: HexStack;
}

// --- Component ---

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

  // Hover state — signals for tooltip
  const [tooltip, setTooltip] = createSignal<TooltipData | null>(null);

  // Keep latest hex stacks for hover lookup
  let currentStacks: Map<string, HexStack> = new Map();

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

  function drawDynamic(
    staticData: BoardStaticData,
    frameData: BoardFrameData,
    _showNumbers: boolean,
    layers: Set<RenderLayer>,
  ) {
    if (!dynamicLayer) return;
    dynamicLayer.removeChildren();

    const { width, height } = staticData;
    const { entities, units, territory, roads, depots, settlements } = frameData;
    const size = HEX_SIZE;

    // Build entity stacks from unified entities; fall back to old units path
    const useEntities = entities.length > 0;
    const hexStacks = useEntities ? buildHexStacks(entities) : buildHexStacksFromLegacy(units, settlements, width);
    currentStacks = hexStacks;

    // Find max health for brightness scaling
    let maxHealth = 1;
    for (const stack of hexStacks.values()) {
      if (stack.totalHealth > maxHealth) maxHealth = stack.totalHealth;
    }

    // Territory overlay
    const terrOverlay = new Graphics();
    if (layers.has("territory")) {
      for (let row = 0; row < height; row++) {
        for (let col = 0; col < width; col++) {
          const idx = row * width + col;
          const key = `${row},${col}`;
          // Skip hexes that have combatant entities (they get full fill)
          const stack = hexStacks.get(key);
          if (stack && stack.combatants.length > 0) continue;
          const owner = territory[idx];
          if (owner === null || owner === undefined) continue;
          const [cx, cy] = hexCenter(row, col, size);
          drawHexPath(terrOverlay, cx, cy, size * 0.88);
          terrOverlay.fill({ color: playerColorNum(owner), alpha: 0.35 });
        }
      }
    }
    dynamicLayer.addChild(terrOverlay);

    // Roads
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

    // Depots
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

    // Entity rendering per hex stack
    const structGfx = new Graphics();
    const combatGfx = new Graphics();
    const resourceGfx = new Graphics();
    const civilianGfx = new Graphics();
    const badgeGfx = new Graphics();
    const facingGfx = new Graphics();

    for (const stack of hexStacks.values()) {
      const [cx, cy] = hexCenter(stack.row, stack.col, size);

      // Structures (settlements) — render if layer enabled
      if (stack.structures.length > 0 && layers.has("settlements")) {
        const s = stack.structures[0];
        const owner = s.owner ?? 0;
        const color = playerColorNum(owner);
        const stype = s.structure_type ?? "Village";

        if (stype === "Farm") {
          structGfx.circle(cx, cy, size * 0.3);
          structGfx.fill({ color, alpha: 0.3 });
          structGfx.circle(cx, cy, size * 0.25);
          structGfx.fill({ color, alpha: 0.8 });
          structGfx.stroke({ color: 0xffffff, width: 0.5 });
        } else if (stype === "Village") {
          const hs = size * 0.45;
          structGfx.circle(cx, cy, size * 0.45);
          structGfx.fill({ color, alpha: 0.3 });
          const bx = cx - hs;
          const by = cy - hs * 0.1;
          const bw = hs * 2;
          const bh = hs * 1.2;
          const peakY = cy - hs * 1.1;
          structGfx.moveTo(bx, by);
          structGfx.lineTo(bx, by + bh);
          structGfx.lineTo(bx + bw, by + bh);
          structGfx.lineTo(bx + bw, by);
          structGfx.lineTo(cx, peakY);
          structGfx.closePath();
          structGfx.fill({ color, alpha: 0.9 });
          structGfx.stroke({ color: 0xffffff, width: 0.5 });
        } else {
          // City
          const w = size * 0.4;
          const h = size * 0.45;
          structGfx.circle(cx, cy, size * 0.55);
          structGfx.fill({ color, alpha: 0.35 });
          structGfx.moveTo(cx - w, cy + h);
          structGfx.lineTo(cx - w, cy - h);
          structGfx.lineTo(cx - w * 0.6, cy - h);
          structGfx.lineTo(cx - w * 0.6, cy - h * 1.3);
          structGfx.lineTo(cx - w * 0.2, cy - h * 1.3);
          structGfx.lineTo(cx - w * 0.2, cy - h);
          structGfx.lineTo(cx + w * 0.2, cy - h);
          structGfx.lineTo(cx + w * 0.2, cy - h * 1.3);
          structGfx.lineTo(cx + w * 0.6, cy - h * 1.3);
          structGfx.lineTo(cx + w * 0.6, cy - h);
          structGfx.lineTo(cx + w, cy - h);
          structGfx.lineTo(cx + w, cy + h);
          structGfx.closePath();
          structGfx.fill({ color, alpha: 0.95 });
          structGfx.stroke({ color: 0xffffff, width: 1 });
        }

        // Population badge on structures
        if (s.contains_count > 0) {
          const badgeR = size * 0.2;
          const bx = cx + size * 0.5;
          const by = cy - size * 0.5;
          badgeGfx.circle(bx, by, badgeR);
          badgeGfx.fill({ color: 0x222222, alpha: 0.85 });
          badgeGfx.stroke({ color: 0xffffff, width: 0.5 });
        }
      }

      // Combatants — colored hex fill
      if (stack.combatants.length > 0) {
        const owner = stack.combatants[0].owner ?? 0;
        const t = healthBrightness(stack.totalHealth, maxHealth);
        const color = playerColorDimNum(owner, t);

        drawHexPath(combatGfx, cx, cy, size * 0.96);
        combatGfx.fill({ color });
        if (stack.anyEngaged) {
          combatGfx.stroke({ color: 0xff6644, width: Math.max(1, size * 0.06) });
        } else {
          combatGfx.stroke({ color: 0x1a1a2e, width: Math.max(0.5, size * 0.04) });
        }

        // Stack count badge when multiple combatants share a hex
        if (stack.combatants.length > 1) {
          const badgeR = size * 0.22;
          const bx = cx + size * 0.55;
          const by = cy - size * 0.55;
          badgeGfx.circle(bx, by, badgeR);
          badgeGfx.fill({ color: 0x000000, alpha: 0.8 });
          badgeGfx.stroke({ color: 0xffffff, width: 0.5 });
          // Draw tick marks for count (1 line per unit, up to 5, then filled circle)
          const count = stack.combatants.length;
          if (count <= 5) {
            const spacing = badgeR * 1.2 / Math.max(count, 1);
            const startX = bx - (count - 1) * spacing / 2;
            for (let i = 0; i < count; i++) {
              badgeGfx.moveTo(startX + i * spacing, by - badgeR * 0.4);
              badgeGfx.lineTo(startX + i * spacing, by + badgeR * 0.4);
              badgeGfx.stroke({ color: 0xffffff, width: 0.8 });
            }
          } else {
            // Filled dot = "many"
            badgeGfx.circle(bx, by, badgeR * 0.5);
            badgeGfx.fill({ color: 0xffffff });
          }
        }

        // Facing arrow at close zoom (scale > 2.0)
        if (scale > 2.0) {
          for (const c of stack.combatants) {
            if (c.facing == null) continue;
            const angle = c.facing; // radians
            const arrowLen = size * 0.6;
            const tipX = cx + Math.cos(angle) * arrowLen;
            const tipY = cy + Math.sin(angle) * arrowLen;
            facingGfx.moveTo(cx, cy);
            facingGfx.lineTo(tipX, tipY);
            facingGfx.stroke({ color: 0xffffff, alpha: 0.7, width: 1.5, cap: "round" });
            // Arrowhead
            const headLen = size * 0.15;
            const a1 = angle + Math.PI * 0.8;
            const a2 = angle - Math.PI * 0.8;
            facingGfx.moveTo(tipX, tipY);
            facingGfx.lineTo(tipX + Math.cos(a1) * headLen, tipY + Math.sin(a1) * headLen);
            facingGfx.stroke({ color: 0xffffff, alpha: 0.7, width: 1.5 });
            facingGfx.moveTo(tipX, tipY);
            facingGfx.lineTo(tipX + Math.cos(a2) * headLen, tipY + Math.sin(a2) * headLen);
            facingGfx.stroke({ color: 0xffffff, alpha: 0.7, width: 1.5 });
            break; // Only show facing for first combatant in stack
          }
        }
      }

      // Resources (convoys) — small colored diamond
      if (stack.resources.length > 0 && layers.has("convoys")) {
        const r = stack.resources[0];
        const owner = r.owner ?? 0;
        const color = playerColorNum(owner);
        const ds = size * 0.3;
        resourceGfx.moveTo(cx, cy - ds);
        resourceGfx.lineTo(cx + ds, cy);
        resourceGfx.lineTo(cx, cy + ds);
        resourceGfx.lineTo(cx - ds, cy);
        resourceGfx.closePath();
        resourceGfx.fill({ color, alpha: 0.85 });
        resourceGfx.stroke({ color: 0xffffff, width: 0.5 });
      }

      // Civilians — small neutral circle
      if (stack.civilians.length > 0 && stack.combatants.length === 0 && stack.structures.length === 0) {
        const owner = stack.civilians[0].owner;
        const color = owner != null ? playerColorNum(owner) : 0x888888;
        civilianGfx.circle(cx, cy, size * 0.2);
        civilianGfx.fill({ color, alpha: 0.6 });
        civilianGfx.stroke({ color: 0xffffff, width: 0.3 });
      }
    }

    dynamicLayer.addChild(structGfx);
    dynamicLayer.addChild(combatGfx);
    dynamicLayer.addChild(resourceGfx);
    dynamicLayer.addChild(civilianGfx);
    dynamicLayer.addChild(badgeGfx);
    dynamicLayer.addChild(facingGfx);
  }

  // Legacy fallback: build HexStacks from old units[] + settlements[]
  function buildHexStacksFromLegacy(
    units: BoardFrameData["units"],
    settlements: BoardFrameData["settlements"],
    _width: number,
  ): Map<string, HexStack> {
    const fakeEntities: SpectatorEntity[] = [];

    for (const u of units) {
      if ((u as any)._dead) continue;
      fakeEntities.push({
        id: u.id,
        owner: u.owner,
        q: u.q,
        r: u.r,
        health: u.strength,
        role: "Soldier",
        engaged: u.engaged,
        contains_count: 0,
      });
    }

    for (const s of settlements) {
      fakeEntities.push({
        id: s.id ?? 0,
        owner: s.owner,
        q: s.q,
        r: s.r,
        structure_type: s.settlement_type ?? "Village",
        engaged: false,
        contains_count: s.population ?? 0,
      });
    }

    return buildHexStacks(fakeEntities);
  }

  // --- Event handlers ---

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    if (!canvasDiv) return;
    const rect = canvasDiv.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;
    const newScale = Math.min(5.0, Math.max(0.2, scale * zoomFactor));

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
    if (isDragging) {
      offsetX = dragStartOffsetX + (e.clientX - dragStartX);
      offsetY = dragStartOffsetY + (e.clientY - dragStartY);
      applyCamera();
      setTooltip(null);
      return;
    }

    // Hover detection — convert screen → world → hex
    if (!canvasDiv) return;
    const rect = canvasDiv.getBoundingClientRect();
    const screenX = e.clientX - rect.left;
    const screenY = e.clientY - rect.top;
    const worldX = (screenX - offsetX) / scale;
    const worldY = (screenY - offsetY) / scale;

    const [row, col] = pixelToHex(worldX, worldY, HEX_SIZE);
    const key = `${row},${col}`;
    const stack = currentStacks.get(key);

    if (stack && stack.entities.length > 0) {
      setTooltip({ screenX: e.clientX - rect.left, screenY: e.clientY - rect.top, row, col, stack });
    } else {
      setTooltip(null);
    }
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

    // Center initial view
    const { width, height } = props.staticData;
    const boardPixelW = SQRT3 * HEX_SIZE * (width + 0.5);
    const boardPixelH = 1.5 * HEX_SIZE * height;
    const canvasW = canvasDiv.clientWidth || 800;
    const canvasH = canvasDiv.clientHeight || 600;
    offsetX = (canvasW - boardPixelW) / 2;
    offsetY = (canvasH - boardPixelH) / 2;
    applyCamera();

    const canvas = app.canvas as HTMLCanvasElement;
    canvas.addEventListener("wheel", handleWheel, { passive: false });
    canvas.addEventListener("pointerdown", handlePointerDown);
    canvas.addEventListener("pointermove", handlePointerMove);
    canvas.addEventListener("pointerup", handlePointerUp);

    const ls = props.layers ?? new Set<RenderLayer>(["territory", "roads", "depots", "settlements", "convoys"]);
    drawTerrain(props.staticData);
    drawDynamic(props.staticData, props.frameData, props.showNumbers ?? false, ls);
  });

  createEffect(() => {
    const sd = props.staticData;
    if (!terrainLayer || !app) return;
    drawTerrain(sd);
    const ls = props.layers ?? new Set<RenderLayer>(["territory", "roads", "depots", "settlements", "convoys"]);
    drawDynamic(sd, props.frameData, props.showNumbers ?? false, ls);
  });

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
      <Show when={tooltip()}>
        {(tip) => {
          const t = tip();
          const stack = t.stack;
          // Position tooltip near cursor, offset slightly
          const left = Math.min(t.screenX + 12, (containerDiv?.clientWidth ?? 600) - 220);
          const top = Math.min(t.screenY + 12, (containerDiv?.clientHeight ?? 400) - 150);
          return (
            <div
              style={{
                position: "absolute",
                left: `${left}px`,
                top: `${top}px`,
                background: "rgba(10, 10, 20, 0.92)",
                color: "#e0e0e0",
                border: "1px solid #444",
                "border-radius": "4px",
                padding: "6px 8px",
                "font-size": "11px",
                "line-height": "1.4",
                "pointer-events": "none",
                "z-index": "10",
                "max-width": "200px",
                "font-family": "monospace",
              }}
            >
              <div style={{ "font-weight": "bold", "margin-bottom": "3px" }}>
                Hex ({t.row}, {t.col})
              </div>
              {stack.combatants.length > 0 && (
                <div>
                  <span style={{ color: stack.dominantOwner != null ? PLAYER_COLORS[stack.dominantOwner % PLAYER_COLORS.length] : "#888" }}>
                    {stack.combatants.length} unit{stack.combatants.length > 1 ? "s" : ""}
                  </span>
                  {" "} HP: {stack.totalHealth.toFixed(0)}
                  {stack.anyEngaged && <span style={{ color: "#ff6644" }}> [engaged]</span>}
                </div>
              )}
              {stack.structures.length > 0 && (
                <div>
                  {stack.structures[0].structure_type}
                  {stack.structures[0].contains_count > 0 && ` (pop: ${stack.structures[0].contains_count})`}
                </div>
              )}
              {stack.resources.length > 0 && (
                <div>
                  Convoy: {stack.resources[0].resource_type}
                  {stack.resources[0].resource_amount != null && ` (${stack.resources[0].resource_amount.toFixed(0)})`}
                </div>
              )}
              {stack.civilians.length > 0 && (
                <div>{stack.civilians.length} civilian{stack.civilians.length > 1 ? "s" : ""}</div>
              )}
            </div>
          );
        }}
      </Show>
    </div>
  );
};

export default HexCanvas;
