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
  hexCenter, type HexRegion, type SettlementEntry,
  pixelToHex, boardPixelSize, HEX_SIZE, playerColorHex, worldToCanvas,
} from "./render/grid";
import { getViewportBounds } from "./render/camera";
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
  onEntityClick?: (entityId: number) => void;
  focusRegion?: HexRegion | null;
}

interface TooltipData {
  screenX: number;
  screenY: number;
  row: number;
  col: number;
  owner: number | null;
  roadLevel: number;
  entities: { id: number; role?: string; owner?: number | null }[];
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
  let lastAppliedTick: number | null = null;
  let cameraInitialized = false;
  let userAdjustedCamera = false;

  // Entity map — outside SolidJS reactivity
  const entityMap = new EntityMap();

  // Tooltip signal
  const [tooltip, setTooltip] = createSignal<TooltipData | null>(null);

  function applyCamera() {
    if (!world) return;
    world.scale.set(camZoom);
    world.position.set(camX, camY);
  }

  function fitCameraToFrame(frame: V3Snapshot | null) {
    if (!canvasRef) return;
    const canvasW = canvasRef.clientWidth || 800;
    const canvasH = canvasRef.clientHeight || 600;
    const focusRegion = props.focusRegion;

    if (focusRegion) {
      const [minX, minY] = hexCenter(focusRegion.minRow, focusRegion.minCol, HEX_SIZE);
      const [maxX, maxY] = hexCenter(focusRegion.maxRow, focusRegion.maxCol, HEX_SIZE);
      const contentW = Math.max(maxX - minX + HEX_SIZE * 2.5, HEX_SIZE * 5);
      const contentH = Math.max(maxY - minY + HEX_SIZE * 2.5, HEX_SIZE * 3);
      const pad = 36;
      const zoomX = (canvasW - pad * 2) / contentW;
      const zoomY = (canvasH - pad * 2) / contentH;
      camZoom = Math.min(16.0, Math.max(1.25, Math.min(zoomX, zoomY)));
      const centerX = (minX + maxX) / 2;
      const centerY = (minY + maxY) / 2;
      camX = canvasW / 2 - centerX * camZoom;
      camY = canvasH / 2 - centerY * camZoom;
      applyCamera();
      cameraInitialized = true;
      return;
    }

    const points = (frame?.entities ?? []).map((e) => worldToCanvas(e.x, e.y));

    if (points.length === 0) {
      const [boardW, boardH] = boardPixelSize(props.width, props.height, HEX_SIZE);
      camZoom = 1.0;
      camX = (canvasW - boardW) / 2;
      camY = (canvasH - boardH) / 2;
      applyCamera();
      cameraInitialized = true;
      return;
    }

    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const [x, y] of points) {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    }

    const contentW = Math.max(maxX - minX, HEX_SIZE * 8);
    const contentH = Math.max(maxY - minY, HEX_SIZE * 6);
    const pad = 80;
    const zoomX = (canvasW - pad * 2) / contentW;
    const zoomY = (canvasH - pad * 2) / contentH;
    camZoom = Math.min(16.0, Math.max(1.0, Math.min(zoomX, zoomY)));
    const centerX = (minX + maxX) / 2;
    const centerY = (minY + maxY) / 2;
    camX = canvasW / 2 - centerX * camZoom;
    camY = canvasH / 2 - centerY * camZoom;
    applyCamera();
    cameraInitialized = true;
  }

  // --- Entity map update from frame data ---

  function updateEntityMap(frame: V3Snapshot | null) {
    if (!frame) return;
    const now = performance.now();
    if (lastAppliedTick != null && frame.tick <= lastAppliedTick) {
      entityMap.clear();
    }
    lastAppliedTick = frame.tick;

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

    // Viewport culling — compute visible world bounds with margin
    const canvasW = canvasRef?.clientWidth ?? 800;
    const canvasH = canvasRef?.clientHeight ?? 600;
    const vp = getViewportBounds(canvasW, canvasH, { x: camX, y: camY, zoom: camZoom });
    const margin = HEX_SIZE * 2; // Render slightly beyond viewport edge
    const vpMinX = vp.minWorldX - margin;
    const vpMinY = vp.minWorldY - margin;
    const vpMaxX = vp.maxWorldX + margin;
    const vpMaxY = vp.maxWorldY + margin;

    function inViewport(x: number, y: number): boolean {
      return x >= vpMinX && x <= vpMaxX && y >= vpMinY && y <= vpMaxY;
    }

    // Build render entity list with interpolated positions, culled by viewport
    const renderEntities: RenderEntity[] = [];
    const corpses: import("./entityMap").EntityState[] = [];

    for (const e of entityMap.entities.values()) {
      const scaledCurr = worldToCanvas(e.currPos.x, e.currPos.y);
      if (e.state === "corpse" || e.state === "dying") {
        if (inViewport(scaledCurr[0], scaledCurr[1])) {
          corpses.push(e);
        }
      } else {
        const pos = getInterpPos(e, t);
        const [x, y] = worldToCanvas(pos.x, pos.y);
        if (inViewport(x, y)) {
          renderEntities.push({
            info: e.info,
            pos: { x, y, z: pos.z },
            facing: getInterpFacing(e, t),
            state: e.state,
          });
        }
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
    userAdjustedCamera = true;
    if (!canvasRef) return;
    const rect = canvasRef.getBoundingClientRect();
    const mouseX = e.clientX - rect.left;
    const mouseY = e.clientY - rect.top;

    const zoomFactor = e.deltaY < 0 ? 1.1 : 0.9;
    const newZoom = Math.min(16.0, Math.max(0.1, camZoom * zoomFactor));

    camX = mouseX - (mouseX - camX) * (newZoom / camZoom);
    camY = mouseY - (mouseY - camY) * (newZoom / camZoom);
    camZoom = newZoom;
    applyCamera();
  }

  function handlePointerDown(e: PointerEvent) {
    userAdjustedCamera = true;
    isDragging = true;
    dragStartX = e.clientX;
    dragStartY = e.clientY;
    dragStartCamX = camX;
    dragStartCamY = camY;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function handlePointerMove(e: PointerEvent | MouseEvent) {
    if (!canvasRef) return;
    const rect = canvasRef.getBoundingClientRect();
    const insideCanvas =
      e.clientX >= rect.left &&
      e.clientX <= rect.right &&
      e.clientY >= rect.top &&
      e.clientY <= rect.bottom;

    if (isDragging) {
      camX = dragStartCamX + (e.clientX - dragStartX);
      camY = dragStartCamY + (e.clientY - dragStartY);
      applyCamera();
      setTooltip(null);
      return;
    }

    if (!insideCanvas) {
      setTooltip(null);
      return;
    }

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

    // Collect entities in this hex from entity map
    const hexEntities: { id: number; role?: string; owner?: number | null }[] = [];
    for (const es of entityMap.entities.values()) {
      const [ex, ey] = worldToCanvas(es.currPos.x, es.currPos.y);
      const [er, ec] = pixelToHex(ex, ey, HEX_SIZE);
      if (er === row && ec === col && es.state === "alive") {
        hexEntities.push({ id: es.info.id, role: es.info.role, owner: es.info.owner });
      }
    }

    setTooltip({
      screenX: e.clientX - rect.left,
      screenY: e.clientY - rect.top,
      row, col, owner, roadLevel, entities: hexEntities,
    });
  }

  function handlePointerUp(e: PointerEvent) {
    const wasDragging = isDragging &&
      (Math.abs(e.clientX - dragStartX) > 3 || Math.abs(e.clientY - dragStartY) > 3);
    isDragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);

    // Click detection — find nearest entity within threshold
    if (!wasDragging && props.onEntityClick && canvasRef) {
      const rect = canvasRef.getBoundingClientRect();
      const worldX = (e.clientX - rect.left - camX) / camZoom;
      const worldY = (e.clientY - rect.top - camY) / camZoom;

      let bestId: number | null = null;
      let bestDist = (20 / camZoom) ** 2; // 20px threshold in screen space

      for (const es of entityMap.entities.values()) {
        if (es.state !== "alive") continue;
        const [px, py] = worldToCanvas(es.currPos.x, es.currPos.y);
        const dx = px - worldX;
        const dy = py - worldY;
        const dist = dx * dx + dy * dy;
        if (dist < bestDist) {
          bestDist = dist;
          bestId = es.info.id;
        }
      }

      if (bestId != null) {
        props.onEntityClick(bestId);
      }
    }
  }

  // --- Lifecycle ---

  onMount(async () => {
    if (!canvasRef || !containerRef) return;

    app = new Application();
    await app.init({
      background: 0x0a0a0f,
      resizeTo: canvasRef,
      antialias: true,
      // We handle wheel zoom ourselves on the raw canvas element.
      // Disable PixiJS wheel interception to avoid hit-test crashes on
      // synthetic events while keeping pointer events for tooltips.
      eventFeatures: { move: true, globalMove: true, click: true, wheel: false },
      // Prevent GPU stalls from ReadPixels in headless/screenshot contexts.
      preserveDrawingBuffer: true,
    });
    canvasRef.appendChild(app.canvas);

    // All pointer/wheel events are handled via raw canvas listeners.
    // Disable PixiJS hit-testing to avoid isInteractive errors on Graphics.
    app.stage.interactiveChildren = false;

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
    fitCameraToFrame(props.frame);

    // Draw terrain (static, only redraws on new game)
    drawTerrain(terrainGfx, {
      width: props.width, height: props.height,
      biomes: props.biomes, heights: props.heights, rivers: props.rivers,
      region: props.focusRegion,
    });

    // Draw initial dynamic content
    redrawDynamic();

    // Register 60fps render tick via PixiJS ticker
    app.ticker.add(renderTick);

    // Register event listeners
    const canvas = app.canvas as HTMLCanvasElement;
    canvas.addEventListener("wheel", handleWheel, { passive: false });
    canvas.addEventListener("pointerdown", handlePointerDown);
    canvas.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("mousemove", handlePointerMove);
  });

  function redrawDynamic() {
    const frame = props.frame;
    const layers = props.layers;

    if (territoryGfx) {
      if (layers.has("territory") && frame) {
        drawTerritory(
          territoryGfx,
          props.width,
          props.height,
          frame.hex_ownership,
          props.focusRegion,
        );
      } else {
        territoryGfx.clear();
      }
    }

    if (roadsGfx) {
      if (layers.has("roads") && frame) {
        drawRoads(roadsGfx, props.width, props.height, frame.hex_roads, props.focusRegion);
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
      region: props.focusRegion,
    });
  });

  // Update entity map and redraw dynamic layers on frame or layer change
  createEffect(() => {
    const frame = props.frame;
    if (!app) return;
    updateEntityMap(frame);
    if (frame && !userAdjustedCamera && (!cameraInitialized || frame.tick <= 1)) {
      fitCameraToFrame(frame);
    }
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
      canvas.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("mousemove", handlePointerMove);
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
              <Show when={t.entities.length > 0}>
                <div>{t.entities.length} entity{t.entities.length > 1 ? "s" : ""}</div>
                {t.entities.slice(0, 5).map((ent) => (
                  <div style={{ "padding-left": "6px", "font-size": "10px" }}>
                    <span style={{ color: ent.owner != null ? playerColorHex(ent.owner) : "#888" }}>
                      #{ent.id}
                    </span>
                    {ent.role ? ` ${ent.role}` : ""}
                  </div>
                ))}
                <Show when={t.entities.length > 5}>
                  <div style={{ "padding-left": "6px", "font-size": "10px", color: "#666" }}>
                    +{t.entities.length - 5} more
                  </div>
                </Show>
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
