import { Component, createEffect, createSignal, onCleanup, onMount, Show } from "solid-js";
import { Application, Container, Graphics } from "pixi.js";
import type { V3Snapshot } from "../v3types";
import type { BiomeName } from "../v2types";
import type { V3RenderLayer } from "./LayerToggles";
import {
  EntityMap, interpT, getInterpPos, getInterpFacing,
} from "./entityMap";
import type { RenderEntity } from "./render/entities";
import {
  getLodTier,
  drawEntitiesClose, drawStackBadges, drawDensityHeatmap,
} from "./render/entities";
import { drawCorpsesClose, drawCorpsesMid, drawCorpsesFar } from "./render/corpses";
import { drawProjectiles } from "./render/projectiles";
import {
  drawTerrain, drawTerritory, drawRoads, drawSettlements,
  type SettlementEntry,
  pixelToHex, boardPixelSize, HEX_SIZE, playerColorHex,
} from "./render/grid";
import * as css from "../styles/v3.css";

interface V3HexCanvasProps {
  width: number;
  height: number;
  biomes: BiomeName[];
  heights: number[];
  rivers: boolean[];
  frame: V3Snapshot | null;
  layers: Set<V3RenderLayer>;
  tickIntervalMs: number;
}

interface TooltipData {
  screenX: number;
  screenY: number;
  row: number;
  col: number;
  owner: number | null;
  roadLevel: number;
  entityCount: number;
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
  let entityGfx: Graphics | null = null;
  let corpseGfx: Graphics | null = null;
  let projectileGfx: Graphics | null = null;

  // Camera state — plain variables, PixiJS manages rendering
  let camZoom = 1.0;
  let camX = 0;
  let camY = 0;
  let isDragging = false;
  let dragStartX = 0;
  let dragStartY = 0;
  let dragStartCamX = 0;
  let dragStartCamY = 0;

  // Entity map — outside SolidJS reactivity
  const entityMap = new EntityMap();

  // Tooltip signal
  const [tooltip, setTooltip] = createSignal<TooltipData | null>(null);

  function applyCamera() {
    if (!world) return;
    world.scale.set(camZoom);
    world.position.set(camX, camY);
  }

  // --- Entity map update from frame data ---

  function updateEntityMap(frame: V3Snapshot | null) {
    if (!frame) return;
    const now = performance.now();

    if (frame.full_state) {
      entityMap.applyFullSnapshot(frame.entities, frame.projectiles, now);
    } else {
      // Full snapshots always have full_state=true in our current protocol.
      // When delta mode lands, the delta will be applied here.
      // For now, treat every frame as a full snapshot replacement.
      entityMap.applyFullSnapshot(frame.entities, frame.projectiles, now);
    }
  }

  // --- 60fps render tick ---

  function renderTick() {
    if (!entityGfx || !corpseGfx || !projectileGfx) return;

    const now = performance.now();
    const t = interpT(
      // Use the latest entity's tick time (they all share the same)
      entityMap.entities.size > 0
        ? entityMap.entities.values().next().value!.lastTickTime
        : now,
      props.tickIntervalMs,
      now,
    );

    // Advance death animations
    entityMap.advanceLifecycle(now);

    // Build render entity list with interpolated positions
    const renderEntities: RenderEntity[] = [];
    const corpses: import("./entityMap").EntityState[] = [];

    for (const e of entityMap.entities.values()) {
      if (e.state === "corpse" || e.state === "dying") {
        corpses.push(e);
      } else {
        renderEntities.push({
          info: e.info,
          pos: getInterpPos(e, t),
          facing: getInterpFacing(e, t),
          state: e.state,
        });
      }
    }

    // Render by LOD tier
    const lod = getLodTier(camZoom);

    if (lod === "close") {
      drawEntitiesClose(entityGfx, renderEntities, camZoom);
      drawCorpsesClose(corpseGfx, corpses, now);
    } else if (lod === "mid") {
      drawStackBadges(entityGfx, renderEntities);
      drawCorpsesMid(corpseGfx, corpses);
    } else {
      drawDensityHeatmap(entityGfx, renderEntities);
      drawCorpsesFar(corpseGfx, corpses);
    }

    // Projectiles (rendered on top of entities at close/mid zoom)
    drawProjectiles(projectileGfx, entityMap.projectiles, lod, props.tickIntervalMs, now);
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

    if (!canvasRef) return;
    const rect = canvasRef.getBoundingClientRect();
    const screenX = e.clientX - rect.left;
    const screenY = e.clientY - rect.top;
    const worldX = (screenX - camX) / camZoom;
    const worldY = (screenY - camY) / camZoom;

    const [row, col] = pixelToHex(worldX, worldY, HEX_SIZE);

    if (row < 0 || row >= props.height || col < 0 || col >= props.width) {
      setTooltip(null);
      return;
    }

    const idx = row * props.width + col;
    const frame = props.frame;
    const owner = frame?.hex_ownership?.[idx] ?? null;
    const roadLevel = frame?.hex_roads?.[idx] ?? 0;

    // Count entities in this hex from entity map
    let entityCount = 0;
    for (const e of entityMap.entities.values()) {
      const er = e.info.hex_r;
      const ec = e.info.hex_q + Math.floor((er - (er & 1)) / 2);
      if (er === row && ec === col && e.state === "alive") entityCount++;
    }

    setTooltip({
      screenX: e.clientX - rect.left,
      screenY: e.clientY - rect.top,
      row, col, owner, roadLevel, entityCount,
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

    // Rendering layers (bottom to top per spec)
    terrainGfx = new Graphics();      // 1. Hex grid
    world.addChild(terrainGfx);

    territoryGfx = new Graphics();    // 2. Territory overlay
    world.addChild(territoryGfx);

    roadsGfx = new Graphics();        // 3. Infrastructure
    world.addChild(roadsGfx);

    settlementsGfx = new Graphics();  // 3b. Structures
    world.addChild(settlementsGfx);

    corpseGfx = new Graphics();       // 4. Corpse layer
    world.addChild(corpseGfx);

    entityGfx = new Graphics();       // 5. Entity layer
    world.addChild(entityGfx);

    projectileGfx = new Graphics();   // 6. Projectile layer
    world.addChild(projectileGfx);

    // Center initial view
    const [boardW, boardH] = boardPixelSize(props.width, props.height, HEX_SIZE);
    const canvasW = canvasRef.clientWidth || 800;
    const canvasH = canvasRef.clientHeight || 600;
    camX = (canvasW - boardW) / 2;
    camY = (canvasH - boardH) / 2;
    applyCamera();

    // Draw terrain (static, only redraws on new game)
    drawTerrain(terrainGfx, {
      width: props.width, height: props.height,
      biomes: props.biomes, heights: props.heights, rivers: props.rivers,
    });

    // Draw initial dynamic content
    redrawDynamic();

    // Register 60fps render tick via PixiJS ticker
    app.ticker.add(renderTick);

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

    if (territoryGfx) {
      if (layers.has("territory") && frame) {
        drawTerritory(territoryGfx, props.width, props.height, frame.hex_ownership);
      } else {
        territoryGfx.clear();
      }
    }

    if (roadsGfx) {
      if (layers.has("roads") && frame) {
        drawRoads(roadsGfx, props.width, props.height, frame.hex_roads);
      } else {
        roadsGfx.clear();
      }
    }

    if (settlementsGfx) {
      if (layers.has("settlements") && frame) {
        const settlements: SettlementEntry[] = [];
        for (const e of frame.entities) {
          if (!e.structure_type) continue;
          const row = e.hex_r;
          const col = e.hex_q + Math.floor((e.hex_r - (e.hex_r & 1)) / 2);
          settlements.push({
            row, col,
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

  // Redraw terrain on static data change
  createEffect(() => {
    if (!terrainGfx) return;
    drawTerrain(terrainGfx, {
      width: props.width, height: props.height,
      biomes: props.biomes, heights: props.heights, rivers: props.rivers,
    });
  });

  // Update entity map and redraw dynamic layers on frame or layer change
  createEffect(() => {
    const frame = props.frame;
    if (!app) return;
    updateEntityMap(frame);
    redrawDynamic();
  });

  // Redraw dynamic when layers change
  createEffect(() => {
    const _ = props.layers;
    if (!app) return;
    redrawDynamic();
  });

  onCleanup(() => {
    if (app) {
      app.ticker.remove(renderTick);
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
    entityGfx = null;
    corpseGfx = null;
    projectileGfx = null;
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
          const left = Math.min(t.screenX + 12, (containerRef?.clientWidth ?? 600) - 200);
          const top = Math.min(t.screenY + 12, (containerRef?.clientHeight ?? 400) - 100);
          return (
            <div
              class={css.v3Tooltip}
              style={{ left: `${left}px`, top: `${top}px` }}
            >
              <div style={{ "font-weight": "bold", "margin-bottom": "3px" }}>
                Hex ({t.row}, {t.col})
              </div>
              <Show when={t.owner != null}>
                <div>
                  Owner:{" "}
                  <span style={{ color: playerColorHex(t.owner!) }}>P{t.owner}</span>
                </div>
              </Show>
              <Show when={t.entityCount > 0}>
                <div>{t.entityCount} entity{t.entityCount > 1 ? "s" : ""}</div>
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
