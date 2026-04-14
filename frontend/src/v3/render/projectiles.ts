// Projectile rendering: velocity-oriented line segments at interpolated positions.
// Close zoom: individual arrows/bolts. Mid zoom: volley clusters. Far: not rendered.

import { Graphics } from "pixi.js";
import type { ProjectileState, Vec3 } from "../entityMap";
import { lerpVec3, interpT } from "../entityMap";
import type { LodTier } from "./entities";

/** Pre-computed projectile for rendering. */
export interface RenderProjectile {
  pos: Vec3;
  velocity: Vec3;
}

function buildRenderProjectiles(
  projectiles: Map<number, ProjectileState>,
  tickIntervalMs: number,
  now: number,
): RenderProjectile[] {
  const result: RenderProjectile[] = [];
  for (const p of projectiles.values()) {
    const t = interpT(p.lastTickTime, tickIntervalMs, now);
    const pos = lerpVec3(p.prevPos, p.currPos, t);
    result.push({
      pos,
      velocity: { x: p.info.vx, y: p.info.vy, z: p.info.vz },
    });
  }
  return result;
}

/** Draw individual projectiles at close zoom — line segments oriented along velocity. */
function drawProjectilesClose(g: Graphics, projectiles: RenderProjectile[]): void {
  for (const p of projectiles) {
    const speed = Math.sqrt(p.velocity.x ** 2 + p.velocity.y ** 2);
    if (speed < 0.001) continue;

    const angle = Math.atan2(p.velocity.y, p.velocity.x);
    const len = 5; // Arrow length in world units
    const tailX = p.pos.x - Math.cos(angle) * len;
    const tailY = p.pos.y - Math.sin(angle) * len;

    // Arrow shaft
    g.moveTo(tailX, tailY);
    g.lineTo(p.pos.x, p.pos.y);
    g.stroke({ color: 0xffcc44, width: 1.5, cap: "round" });

    // Arrow tip
    const tipLen = 2;
    const a1 = angle + Math.PI * 0.8;
    const a2 = angle - Math.PI * 0.8;
    g.moveTo(p.pos.x, p.pos.y);
    g.lineTo(p.pos.x + Math.cos(a1) * tipLen, p.pos.y + Math.sin(a1) * tipLen);
    g.stroke({ color: 0xffcc44, width: 1.2 });
    g.moveTo(p.pos.x, p.pos.y);
    g.lineTo(p.pos.x + Math.cos(a2) * tipLen, p.pos.y + Math.sin(a2) * tipLen);
    g.stroke({ color: 0xffcc44, width: 1.2 });
  }
}

/** Draw volley clusters at mid zoom — aggregate nearby projectiles. */
function drawProjectilesMid(g: Graphics, projectiles: RenderProjectile[]): void {
  if (projectiles.length === 0) return;

  // Simple approach: draw each as a small dot
  for (const p of projectiles) {
    g.circle(p.pos.x, p.pos.y, 1.5);
    g.fill({ color: 0xffcc44, alpha: 0.8 });
  }
}

/** Main projectile draw function — selects rendering by LOD tier. */
export function drawProjectiles(
  g: Graphics,
  projectiles: Map<number, ProjectileState>,
  lod: LodTier,
  tickIntervalMs: number,
  now: number,
): void {
  g.clear();

  if (projectiles.size === 0) return;
  if (lod === "far") return; // Not rendered at far zoom

  const rendered = buildRenderProjectiles(projectiles, tickIntervalMs, now);

  if (lod === "close") {
    drawProjectilesClose(g, rendered);
  } else {
    drawProjectilesMid(g, rendered);
  }
}
