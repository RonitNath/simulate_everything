import { Component, createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { Application, Container, Graphics } from "pixi.js";
import type { V3Snapshot } from "../v3types";
import type { BiomeName } from "../v2types";
import type { V3RenderLayer } from "./LayerToggles";
import {
  drawTerrain, drawTerritory, drawRoads, drawSettlements,
  type TerrainData, type SettlementEntry,
  pixelToHex, boardPixelSize, HEX_SIZE, playerColorHex,
} from "./render/grid";
import * as css from "../styles/v3.css";

interface V3HexCanvasProps {
  // Init data (terrain)
  width: number;
  height: number;
  biomes: BiomeName[];
  heights: number[];
  rivers: boolean[];

  // Frame data
  frame: V3Snapshot | null;

  // Layer visibility
  layers: Set<V3RenderLayer>;
}

interface TooltipData {
  screenX: number;
  screenY: number;
  row: number;
  col: number;
  owner: number | null;
  roadLevel: number;
}

const V3HexCanvas: Component<V3HexCanvasProps> = (props) => {
  let containerRef: HTMLDivElement | undefined;
  let canvasRef: HTMLDivElement | undefined;

  let app: Application | null = null;
  let world: Container | null = null;
  let terrainGfx: Graphics | null = null;
  let territoryGfx: Graphics | null = null;
  let roadsGfx: Graphics | null = null;
  let settlementsGfx: Graphics | null = null;

  // Camera state — plain variables, PixiJS manages rendering
  let camZoom = 1.0;
  let camX = 0;
  let camY = 0;
  let isDragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartCamX = 0;
  let dragStartCamY = 0;

  // Tooltip signal — SolidJS renders HTML overlay
  const [tooltip, setTooltip] = createSignal<TooltipData | null>(null);

  function applyCamera() {
    if (!world) return;
    world.scale.set(camZoom);
    world.position.set(camX, camY);
  }

  // --- Event handlers ---

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    if (!canvasRef) return;
    const rect = canvasRef.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;
    const newZoom = Math.min(5.0, Math.max(0.1, camZoom * zoomFactor));

    // Zoom centered on mouse cursor
    camX = mouseX - (mouseX - camX) * (newZoom / camZoom);
    camY = mouseY - (mouseY - camY) * (newZoom / camZoom);
    camZoom = newZoom;
    applyCamera();
  }

  function handlePointerDown(e: PointerEvent) {
    isDragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    dragStartCamX = camX;
    dragStartCamY = camY;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function handlePointerMove(e: PointerEvent) {
    if (isDragging) {
      camX = dragStartCamX + (e.clientX - dragStartX);
      camY = dragStartCamY + (e.clientY - dragStartY);
      applyCamera();
      setTooltip(null);
      return;
    }

    // Hover detection — screen → world → hex
    if (!canvasRef) return;
    const rect = canvasRef.getBoundingClientRect();
    const screenX = e.clientX - rect.left;
    const screenY = e.clientY - rect.top;
    const worldX = (screenX - camX) / camZoom;
    const worldY = (screenY - camY) / camZoom;

    const [row, col] = pixelToHex(worldX, worldY, HEX_SIZE);

    // Bounds check
    if (row < 0 || row >= props.height || col < 0 || col >= props.width) {
      setTooltip(null);
      return;
    }

    const idx = row * props.width + col;
    const frame = props.frame;
    const owner = frame?.hex_ownership?.[idx] ?? null;
    const roadLevel = frame?.hex_roads?.[idx] ?? 0;

    setTooltip({
      screenX: e.clientX - rect.left,
      screenY: e.clientY - rect.top,
      row,
      col,
      owner,
      roadLevel,
    });
  }

  function handlePointerUp(e: PointerEvent) {
    isDragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
  }

  // --- Lifecycle ---

  onMount(async () => {
    if (!canvasRef || !containerRef) return;

    app = new Application();
    await app.init({
      background: 0x0a0a0f,
      resizeTo: canvasRef,
      antialias: true,
    });
    canvasRef.appendChild(app.canvas);

    world = new Container();
    app.stage.addChild(world);

    // Create rendering layers (bottom to top)
    terrainGfx = new Graphics();
    world.addChild(terrainGfx);

    territoryGfx = new Graphics();
    world.addChild(territoryGfx);

    roadsGfx = new Graphics();
    world.addChild(roadsGfx);

    settlementsGfx = new Graphics();
    world.addChild(settlementsGfx);

    // Center initial view
    const [boardW, boardH] = boardPixelSize(props.width, props.height, HEX_SIZE);
    const canvasW = canvasRef.clientWidth || 800;
    const canvasH = canvasRef.clientHeight || 600;
    camX = (canvasW - boardW) / 2;
    camY = (canvasH - boardH) / 2;
    applyCamera();

    // Draw initial terrain
    const terrainData: TerrainData = {
      width: props.width,
      height: props.height,
      biomes: props.biomes,
      heights: props.heights,
      rivers: props.rivers,
    };
    drawTerrain(terrainGfx, terrainData);

    // Draw initial dynamic content
    redrawDynamic();

    // Register event listeners
    const canvas = app.canvas as HTMLCanvasElement;
    canvas.addEventListener("wheel", handleWheel, { passive: false });
    canvas.addEventListener("pointerdown", handlePointerDown);
    canvas.addEventListener("pointermove", handlePointerMove);
    canvas.addEventListener("pointerup", handlePointerUp);
  });

  function redrawDynamic() {
    const frame = props.frame;
    const layers = props.layers;

    // Territory
    if (territoryGfx) {
      if (layers.has("territory") && frame) {
        drawTerritory(territoryGfx, props.width, props.height, frame.hex_ownership);
      } else {
        territoryGfx.clear();
      }
    }

    // Roads
    if (roadsGfx) {
      if (layers.has("roads") && frame) {
        drawRoads(roadsGfx, props.width, props.height, frame.hex_roads);
      } else {
        roadsGfx.clear();
      }
    }

    // Settlements — extract from entities in the frame
    if (settlementsGfx) {
      if (layers.has("settlements") && frame) {
        const settlements: SettlementEntry[] = [];
        for (const e of frame.entities) {
          if (!e.structure_type) continue;
          // V3 entities use axial (hex_q, hex_r) → offset even-r (row, col)
          const row = e.hex_r;
          const col = e.hex_q + Math.floor((e.hex_r - (e.hex_r & 1)) / 2);
          settlements.push({
            row,
            col,
            owner: e.owner ?? 0,
            structureType: e.structure_type,
            containsCount: e.contains_count,
          });
        }
        drawSettlements(settlementsGfx, settlements);
      } else {
        settlementsGfx.clear();
      }
    }
  }

  // Redraw terrain when static data changes (rare — only on new game init)
  createEffect(() => {
    if (!terrainGfx) return;
    const terrainData: TerrainData = {
      width: props.width,
      height: props.height,
      biomes: props.biomes,
      heights: props.heights,
      rivers: props.rivers,
    };
    drawTerrain(terrainGfx, terrainData);
  });

  // Redraw dynamic layers when frame or layers change
  createEffect(() => {
    // Access reactive props inside tracking scope
    const _frame = props.frame;
    const _layers = props.layers;
    if (!app) return;
    redrawDynamic();
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
    terrainGfx = null;
    territoryGfx = null;
    roadsGfx = null;
    settlementsGfx = null;
  });

  return (
    <div
      ref={containerRef}
      style={{ width: "100%", height: "100%", position: "relative", overflow: "hidden" }}
    >
      <div
        ref={canvasRef}
        style={{ width: "100%", height: "100%", display: "block" }}
      />
      <Show when={tooltip()}>
        {(tip) => {
          const t = tip();
          const left = Math.min(t.screenX + 12, (containerRef?.clientWidth ?? 600) - 180);
          const top = Math.min(t.screenY + 12, (containerRef?.clientHeight ?? 400) - 80);
          return (
            <div
              class={css.v3Tooltip}
              style={{
                left: `${left}px`,
                top: `${top}px`,
              }}
            >
              <div style={{ "font-weight": "bold", "margin-bottom": "3px" }}>
                Hex ({t.row}, {t.col})
              </div>
              <Show when={t.owner != null}>
                <div>
                  Owner:{" "}
                  <span style={{ color: playerColorHex(t.owner!) }}>
                    P{t.owner}
                  </span>
                </div>
              </Show>
              <Show when={t.roadLevel > 0}>
                <div>Road level: {t.roadLevel}</div>
              </Show>
            </div>
          );
        }}
      </Show>
    </div>
  );
};

export default V3HexCanvas;
