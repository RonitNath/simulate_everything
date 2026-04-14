// Corpse rendering: desaturated entities at frozen positions.
// Close zoom: visible equipment, fallen orientation.
// Mid zoom: dimmed count in hex badge.
// Far zoom: subtle ground darkening.

import { Graphics } from "pixi.js";
import type { EntityState } from "../entityMap";
import { getDeathProgress } from "../entityMap";
import { playerColorNum, HEX_SIZE, hexCenter, pixelToHex, worldToCanvas } from "./grid";

// Desaturate a color by blending toward gray
function desaturate(color: number, amount: number): number {
  const r = (color >> 16) & 0xff;
  const g = (color >> 8) & 0xff;
  const b = color & 0xff;
  const gray = Math.round(r * 0.299 + g * 0.587 + b * 0.114);
  const dr = Math.round(r + (gray - r) * amount);
  const dg = Math.round(g + (gray - g) * amount);
  const db = Math.round(b + (gray - b) * amount);
  return ((dr << 16) | (dg << 8) | db) >>> 0;
}

export function drawCorpsesClose(
  g: Graphics,
  corpses: EntityState[],
  now: number,
): void {
  g.clear();

  for (const e of corpses) {
    const [px, py] = worldToCanvas(e.currPos.x, e.currPos.y);
    const owner = e.info.owner ?? 0;
    const baseColor = playerColorNum(owner);
    const color = desaturate(baseColor, 0.7);

    if (e.state === "dying") {
      // Fall animation: entity tilts/shrinks
      const progress = getDeathProgress(e, now);
      const radius = 3 * (1 - progress * 0.3); // Slightly shrink
      const alpha = 0.9 - progress * 0.3;

      g.circle(px, py, radius);
      g.fill({ color, alpha });
      g.stroke({ color: 0x440000, width: 0.5 });
    } else {
      // Corpse: small desaturated mark
      g.circle(px, py, 2.5);
      g.fill({ color, alpha: 0.5 });
      g.stroke({ color: 0x333333, width: 0.3 });

      // Equipment indicators at very close zoom
      if (e.info.weapon_type) {
        // Small line near corpse representing dropped weapon
        const wx = px + 3;
        const wy = py + 1;
        g.moveTo(wx - 2, wy);
        g.lineTo(wx + 2, wy);
        g.stroke({ color: 0x888888, alpha: 0.5, width: 0.6 });
      }
    }
  }
}

export function drawCorpsesMid(
  g: Graphics,
  corpses: EntityState[],
): void {
  g.clear();

  // Bucket corpses by hex
  const buckets = new Map<string, { row: number; col: number; count: number }>();

  for (const e of corpses) {
    const [row, col] = pixelToHex(
      ...worldToCanvas(e.currPos.x, e.currPos.y),
      HEX_SIZE,
    );
    const key = `${row},${col}`;

    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = { row, col, count: 0 };
      buckets.set(key, bucket);
    }
    bucket.count++;
  }

  const size = HEX_SIZE;
  for (const bucket of buckets.values()) {
    if (bucket.count === 0) continue;
    const [cx, cy] = hexCenter(bucket.row, bucket.col, size);

    // Dimmed marker for corpse-heavy hexes
    const alpha = Math.min(0.4, 0.1 + 0.05 * bucket.count);
    g.circle(cx, cy, size * 0.25);
    g.fill({ color: 0x442222, alpha });
  }
}

export function drawCorpsesFar(
  g: Graphics,
  corpses: EntityState[],
): void {
  g.clear();

  // Subtle ground darkening for hexes with many corpses
  const buckets = new Map<string, { row: number; col: number; count: number }>();

  for (const e of corpses) {
    const [row, col] = pixelToHex(
      ...worldToCanvas(e.currPos.x, e.currPos.y),
      HEX_SIZE,
    );
    const key = `${row},${col}`;

    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = { row, col, count: 0 };
      buckets.set(key, bucket);
    }
    bucket.count++;
  }

  const size = HEX_SIZE;
  for (const bucket of buckets.values()) {
    if (bucket.count < 3) continue; // Only show for significant corpse piles
    const [cx, cy] = hexCenter(bucket.row, bucket.col, size);
    const alpha = Math.min(0.3, 0.05 * bucket.count);

    g.circle(cx, cy, size * 0.4);
    g.fill({ color: 0x331111, alpha });
  }
}
