// Camera state and coordinate transforms for PixiJS hex renderer.
// Camera is plain variables — not reactive. PixiJS manages rendering directly.

export interface CameraState {
  x: number;       // world-space offset applied to container
  y: number;
  zoom: number;    // scale factor: 0.1 – 5.0
}

export interface ViewportBounds {
  minWorldX: number;
  minWorldY: number;
  maxWorldX: number;
  maxWorldY: number;
}

export function worldToScreen(
  worldX: number,
  worldY: number,
  cam: CameraState,
): [number, number] {
  return [worldX * cam.zoom + cam.x, worldY * cam.zoom + cam.y];
}

export function screenToWorld(
  screenX: number,
  screenY: number,
  cam: CameraState,
): [number, number] {
  return [(screenX - cam.x) / cam.zoom, (screenY - cam.y) / cam.zoom];
}

export function getViewportBounds(
  canvasWidth: number,
  canvasHeight: number,
  cam: CameraState,
): ViewportBounds {
  const [minX, minY] = screenToWorld(0, 0, cam);
  const [maxX, maxY] = screenToWorld(canvasWidth, canvasHeight, cam);
  return {
    minWorldX: minX,
    minWorldY: minY,
    maxWorldX: maxX,
    maxWorldY: maxY,
  };
}
