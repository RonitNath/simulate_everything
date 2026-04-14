// Entity rendering at continuous world positions.
// Supports close zoom (individual entities) and mid zoom (stack badges).
// Render functions receive pre-computed interpolated positions from HexCanvas.

import { Graphics } from "pixi.js";
import type { Vec3 } from "../entityMap";
import type { SpectatorEntityInfo } from "../../v3types";
import { playerColorNum, HEX_SIZE, hexCenter } from "./grid";

// --- LOD tier ---

export type LodTier = "close" | "mid" | "far";

export function getLodTier(zoom: number): LodTier {
  const hexPx = HEX_SIZE * zoom;
  if (hexPx > 80) return "close";
  if (hexPx >= 20) return "mid";
  return "far";
}

/** Entity with pre-computed render position and facing. */
export interface RenderEntity {
  info: SpectatorEntityInfo;
  pos: Vec3;
  facing: number;
  state: "alive" | "dying" | "corpse";
}

// --- Close zoom: individual entities ---

export function drawEntitiesClose(
  g: Graphics,
  entities: RenderEntity[],
  zoom: number,
): void {
  g.clear();

  for (const e of entities) {
    if (e.state !== "alive") continue;
    if (e.info.entity_kind === "Structure") continue;

    const owner = e.info.owner ?? 0;
    const color = playerColorNum(owner);

    // Person circle
    const radius = 3;
    g.circle(e.pos.x, e.pos.y, radius);
    g.fill({ color });
    g.stroke({ color: 0x1a1a2e, width: 0.5 });

    // Facing arrow
    if (e.info.facing != null) {
      const arrowLen = 6;
      const tipX = e.pos.x + Math.cos(e.facing) * arrowLen;
      const tipY = e.pos.y + Math.sin(e.facing) * arrowLen;
      g.moveTo(e.pos.x, e.pos.y);
      g.lineTo(tipX, tipY);
      g.stroke({ color: 0xffffff, alpha: 0.7, width: 1, cap: "round" });
    }

    // Blood bar (when blood < 1.0)
    const blood = e.info.blood;
    if (blood != null && blood < 1.0 && blood > 0) {
      const barW = 8;
      const barH = 2;
      const bx = e.pos.x - barW / 2;
      const by = e.pos.y - radius - 4;
      g.rect(bx, by, barW, barH);
      g.fill({ color: 0x333333, alpha: 0.8 });
      g.rect(bx, by, barW * blood, barH);
      g.fill({ color: blood > 0.5 ? 0x44cc44 : blood > 0.25 ? 0xcccc44 : 0xcc4444 });
    }

    // Stamina bar (below blood bar)
    const stamina = e.info.stamina;
    if (stamina != null && stamina < 1.0) {
      const barW = 8;
      const barH = 1.5;
      const bx = e.pos.x - barW / 2;
      const by = e.pos.y - radius - 2;
      g.rect(bx, by, barW, barH);
      g.fill({ color: 0x222222, alpha: 0.7 });
      g.rect(bx, by, barW * stamina, barH);
      g.fill({ color: 0x4488cc });
    }

    // Wound zone indicators (colored dots by body zone)
    if (e.info.wounds && e.info.wounds.length > 0) {
      const woundColors: Record<string, number> = {
        Head: 0xff2222,
        Torso: 0xff6644,
        LeftArm: 0xff8844,
        RightArm: 0xff8844,
        Legs: 0xffaa44,
      };
      const severitySize: Record<string, number> = {
        Light: 1.0,
        Serious: 1.5,
        Critical: 2.0,
      };
      let wIdx = 0;
      for (const [zone, severity] of e.info.wounds) {
        const wx = e.pos.x + radius + 2 + wIdx * 3;
        const wy = e.pos.y - radius;
        const wColor = woundColors[zone] ?? 0xff4444;
        const wSize = severitySize[severity] ?? 1.5;
        g.circle(wx, wy, wSize);
        g.fill({ color: wColor, alpha: 0.9 });
        wIdx++;
        if (wIdx >= 4) break; // Max 4 wound indicators
      }
    }

    // Equipment indicators at very close zoom
    if (HEX_SIZE * zoom >= 120) {
      if (e.info.weapon_type) {
        const wx = e.pos.x - 5;
        const wy = e.pos.y + 5;
        const wt = e.info.weapon_type.toLowerCase();
        if (wt.includes("slash")) {
          g.moveTo(wx - 2, wy + 2);
          g.lineTo(wx + 2, wy - 2);
          g.stroke({ color: 0xcccccc, width: 0.8 });
        } else if (wt.includes("pierce")) {
          g.moveTo(wx, wy - 2);
          g.lineTo(wx, wy + 2);
          g.stroke({ color: 0xcccccc, width: 0.8 });
        } else {
          g.circle(wx, wy, 1.5);
          g.stroke({ color: 0xcccccc, width: 0.8 });
        }
      }
      if (e.info.armor_type) {
        g.rect(e.pos.x + 4, e.pos.y + 4, 3, 3);
        g.stroke({ color: 0x8888aa, width: 0.6 });
      }
    }
  }
}

// --- Mid zoom: stack badges ---

export function drawStackBadges(
  g: Graphics,
  entities: RenderEntity[],
): void {
  g.clear();

  // Bucket by hex
  const buckets = new Map<string, {
    row: number; col: number;
    alive: number; dominantOwner: number;
  }>();

  for (const e of entities) {
    if (e.state !== "alive") continue;
    if (e.info.entity_kind === "Structure") continue;

    const q = e.info.hex_q;
    const r = e.info.hex_r;
    const row = r;
    const col = q + Math.floor((r - (r & 1)) / 2);
    const key = `${row},${col}`;

    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = { row, col, alive: 0, dominantOwner: e.info.owner ?? 0 };
      buckets.set(key, bucket);
    }
    bucket.alive++;
    if (e.info.owner != null) bucket.dominantOwner = e.info.owner;
  }

  const size = HEX_SIZE;
  for (const bucket of buckets.values()) {
    if (bucket.alive === 0) continue;
    const [cx, cy] = hexCenter(bucket.row, bucket.col, size);
    const color = playerColorNum(bucket.dominantOwner);

    const badgeR = size * 0.35;
    g.circle(cx, cy, badgeR);
    g.fill({ color, alpha: 0.85 });
    g.stroke({ color: 0xffffff, width: 0.8 });

    const count = bucket.alive;
    if (count <= 5) {
      const spacing = badgeR * 1.2 / Math.max(count, 1);
      const startX = cx - (count - 1) * spacing / 2;
      for (let i = 0; i < count; i++) {
        g.moveTo(startX + i * spacing, cy - badgeR * 0.4);
        g.lineTo(startX + i * spacing, cy + badgeR * 0.4);
        g.stroke({ color: 0xffffff, width: 0.8 });
      }
    } else {
      g.circle(cx, cy, badgeR * 0.5);
      g.fill({ color: 0xffffff });
    }
  }
}

// --- Far zoom: density heatmap ---

export function drawDensityHeatmap(
  g: Graphics,
  entities: RenderEntity[],
): void {
  g.clear();

  const counts = new Map<string, { row: number; col: number; owner: number; count: number }>();

  for (const e of entities) {
    if (e.state !== "alive") continue;
    if (e.info.entity_kind === "Structure") continue;

    const q = e.info.hex_q;
    const r = e.info.hex_r;
    const row = r;
    const col = q + Math.floor((r - (r & 1)) / 2);
    const key = `${row},${col}`;

    let entry = counts.get(key);
    if (!entry) {
      entry = { row, col, owner: e.info.owner ?? 0, count: 0 };
      counts.set(key, entry);
    }
    entry.count++;
    if (e.info.owner != null) entry.owner = e.info.owner;
  }

  const size = HEX_SIZE;
  const maxCount = Math.max(1, ...Array.from(counts.values()).map((c) => c.count));

  for (const entry of counts.values()) {
    const [cx, cy] = hexCenter(entry.row, entry.col, size);
    const alpha = 0.15 + 0.6 * (entry.count / maxCount);
    const color = playerColorNum(entry.owner);

    g.circle(cx, cy, size * 0.6);
    g.fill({ color, alpha });
  }
}
