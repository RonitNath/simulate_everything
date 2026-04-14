// Entity rendering at continuous world positions.
// Supports close zoom (individual entities) and mid zoom (stack badges).
// Render functions receive pre-computed interpolated positions from HexCanvas.

import { Graphics } from "pixi.js";
import type { Vec3 } from "../entityMap";
import type { SpectatorEntityInfo } from "../../v3types";
import { playerColorNum, HEX_SIZE, hexCenter, pixelToHex } from "./grid";

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

// --- Wound lookup helpers ---

type WoundMap = {
  Head: number;
  Torso: number;
  LeftArm: number;
  RightArm: number;
  Legs: number;
};

const SEVERITY_WEIGHT: Record<string, number> = {
  Light: 0.3,
  Serious: 0.6,
  Critical: 1.0,
};

function buildWoundMap(wounds: [string, string][] | undefined): WoundMap {
  const m: WoundMap = { Head: 0, Torso: 0, LeftArm: 0, RightArm: 0, Legs: 0 };
  if (!wounds) return m;
  for (const [zone, severity] of wounds) {
    const key = zone as keyof WoundMap;
    if (key in m) {
      m[key] = Math.min(1.0, m[key] + (SEVERITY_WEIGHT[severity] ?? 0.3));
    }
  }
  return m;
}

/** Blend a base color toward damage red by a 0–1 factor. */
function woundTint(base: number, damage: number): number {
  if (damage <= 0) return base;
  const t = Math.min(damage, 1.0);
  const br = (base >> 16) & 0xff;
  const bg = (base >> 8) & 0xff;
  const bb = base & 0xff;
  // Tint toward dark crimson (0x8b1a1a)
  const r = Math.round(br + (0x8b - br) * t);
  const g = Math.round(bg + (0x1a - bg) * t);
  const b = Math.round(bb + (0x1a - bb) * t);
  return (r << 16) | (g << 8) | b;
}

/** Darken a color by a factor (0 = unchanged, 1 = black). */
function darken(color: number, amount: number): number {
  const t = 1 - Math.min(amount, 1.0);
  const r = Math.round(((color >> 16) & 0xff) * t);
  const g = Math.round(((color >> 8) & 0xff) * t);
  const b = Math.round((color & 0xff) * t);
  return (r << 16) | (g << 8) | b;
}

// --- Close zoom: individual entities ---

export function drawEntitiesClose(
  g: Graphics,
  entities: RenderEntity[],
  _zoom: number,
): void {
  g.clear();

  for (const e of entities) {
    if (e.state !== "alive") continue;
    if (e.info.entity_kind === "Structure") continue;

    const owner = e.info.owner ?? 0;
    const baseColor = playerColorNum(owner);
    const facing = e.facing;
    const cos = Math.cos(facing);
    const sin = Math.sin(facing);
    // Perpendicular direction (left of facing).
    const perpCos = -sin;
    const perpSin = cos;

    const wounds = buildWoundMap(e.info.wounds);

    // --- Ground shadow (subtle facing cue) ---
    const shadowOff = 1.0;
    g.ellipse(
      e.pos.x + shadowOff, e.pos.y + shadowOff,
      5.5, 3.5,
    );
    g.fill({ color: 0x000000, alpha: 0.2 });

    // --- Legs (two short lines behind torso, angled outward) ---
    const legColor = woundTint(darken(baseColor, 0.25), wounds.Legs);
    const legLen = 4.0;
    const legSpread = 0.4; // radians from facing-backward
    const backAngle = facing + Math.PI;
    for (const side of [-1, 1]) {
      const lAngle = backAngle + side * legSpread;
      const lx = e.pos.x + Math.cos(lAngle) * legLen;
      const ly = e.pos.y + Math.sin(lAngle) * legLen;
      g.moveTo(e.pos.x, e.pos.y);
      g.lineTo(lx, ly);
      g.stroke({ color: legColor, width: 1.2, cap: "round" });
    }

    // --- Torso (elongated oval rotated to facing) ---
    const torsoColor = woundTint(baseColor, wounds.Torso);
    const torsoLen = 4.0;  // half-length along facing
    const torsoWid = 2.8;  // half-width perpendicular
    // Approximate rotated ellipse with a pill shape (two arcs + lines).
    // PixiJS Graphics doesn't support rotated ellipses, so draw as a polygon.
    const torsoPoints: number[] = [];
    const torsoSegs = 12;
    for (let i = 0; i <= torsoSegs; i++) {
      const t = (i / torsoSegs) * Math.PI * 2;
      const lx = Math.cos(t) * torsoLen;
      const ly = Math.sin(t) * torsoWid;
      // Rotate by facing.
      torsoPoints.push(
        e.pos.x + lx * cos - ly * sin,
        e.pos.y + lx * sin + ly * cos,
      );
    }
    g.poly(torsoPoints);
    g.fill({ color: torsoColor });
    g.stroke({ color: darken(torsoColor, 0.35), width: 0.6 });

    // --- Shoulders + Arms (cross-lines perpendicular to facing) ---
    const shoulderOffset = 1.0; // slightly forward of center
    const sx = e.pos.x + cos * shoulderOffset;
    const sy = e.pos.y + sin * shoulderOffset;
    const armLen = 3.8;
    // Left arm.
    const laColor = woundTint(darken(baseColor, 0.15), wounds.LeftArm);
    g.moveTo(sx, sy);
    g.lineTo(sx + perpCos * armLen, sy + perpSin * armLen);
    g.stroke({ color: laColor, width: 1.4, cap: "round" });
    // Right arm.
    const raColor = woundTint(darken(baseColor, 0.15), wounds.RightArm);
    g.moveTo(sx, sy);
    g.lineTo(sx - perpCos * armLen, sy - perpSin * armLen);
    g.stroke({ color: raColor, width: 1.4, cap: "round" });

    // --- Head (circle offset toward facing) ---
    const headDist = 3.8;
    const headR = 2.2;
    const hx = e.pos.x + cos * headDist;
    const hy = e.pos.y + sin * headDist;
    const headColor = woundTint(darken(baseColor, -0.1), wounds.Head);
    g.circle(hx, hy, headR);
    g.fill({ color: headColor });
    g.stroke({ color: darken(baseColor, 0.4), width: 0.6 });

    // Facing visor (small chevron on head).
    const visorLen = 1.4;
    const visorSpread = 0.5;
    const v1x = hx + Math.cos(facing - visorSpread) * visorLen;
    const v1y = hy + Math.sin(facing - visorSpread) * visorLen;
    const v2x = hx + cos * (visorLen + 0.6);
    const v2y = hy + sin * (visorLen + 0.6);
    const v3x = hx + Math.cos(facing + visorSpread) * visorLen;
    const v3y = hy + Math.sin(facing + visorSpread) * visorLen;
    g.moveTo(v1x, v1y);
    g.lineTo(v2x, v2y);
    g.lineTo(v3x, v3y);
    g.stroke({ color: 0xdddddd, alpha: 0.8, width: 0.7, cap: "round", join: "round" });

    // --- Weapon (distinct from facing, uses attack state) ---
    drawWeapon(g, e, sx, sy, perpCos, perpSin);

    // --- Status bars (blood + stamina, above entity) ---
    const barY = e.pos.y - 8;
    const blood = e.info.blood;
    if (blood != null && blood < 1.0 && blood > 0) {
      const barW = 10;
      const barH = 1.8;
      const bx = e.pos.x - barW / 2;
      g.rect(bx, barY, barW, barH);
      g.fill({ color: 0x1a1a1a, alpha: 0.8 });
      g.rect(bx, barY, barW * blood, barH);
      g.fill({ color: blood > 0.5 ? 0x44cc44 : blood > 0.25 ? 0xcccc44 : 0xcc4444 });
    }
    const stamina = e.info.stamina;
    if (stamina != null && stamina < 1.0) {
      const barW = 10;
      const barH = 1.3;
      const bx = e.pos.x - barW / 2;
      const sy2 = barY + 2.2;
      g.rect(bx, sy2, barW, barH);
      g.fill({ color: 0x1a1a1a, alpha: 0.7 });
      g.rect(bx, sy2, barW * stamina, barH);
      g.fill({ color: 0x4488cc });
    }
  }
}

// --- Weapon rendering (separate from body) ---

function drawWeapon(
  g: Graphics,
  e: RenderEntity,
  shoulderX: number,
  shoulderY: number,
  perpCos: number,
  perpSin: number,
): void {
  const phase = e.info.attack_phase ?? "idle";
  const weaponAngle = e.info.weapon_angle;
  const progress = e.info.attack_progress ?? 0;
  const weaponType = (e.info.weapon_type ?? "").toLowerCase();

  // Weapon grip point: offset to the right arm side.
  const gripX = shoulderX - perpCos * 3.2;
  const gripY = shoulderY - perpSin * 3.2;

  // Determine weapon angle.
  let angle: number;
  if (phase !== "idle" && weaponAngle != null) {
    angle = weaponAngle;
  } else {
    // Idle: weapon rests angled slightly down from facing.
    angle = e.facing + 0.3;
  }

  // Weapon visual properties by phase.
  let bladeLen = 7.0;
  let bladeWidth = 1.0;
  let bladeColor = 0xaaaaaa; // steel gray
  let bladeAlpha = 0.7;
  let glowColor = 0;
  let glowAlpha = 0;

  switch (phase) {
    case "windup":
      bladeWidth = 1.2;
      bladeAlpha = 0.85;
      bladeColor = 0xbbbbbb;
      // Subtle yellow glow building up.
      glowColor = 0xffdd44;
      glowAlpha = 0.15 + progress * 0.2;
      break;
    case "committed":
      bladeWidth = 1.6;
      bladeAlpha = 1.0;
      bladeColor = 0xeeeeee;
      // Bright slash glow.
      glowColor = 0xffaa22;
      glowAlpha = 0.4;
      bladeLen = 8.0;
      break;
    case "recovery":
      bladeWidth = 1.0;
      bladeAlpha = 0.5;
      bladeColor = 0x999999;
      break;
    default: // idle
      bladeWidth = 0.9;
      bladeAlpha = 0.55;
      break;
  }

  // Pierce weapons are thinner and longer.
  if (weaponType.includes("pierce")) {
    bladeLen += 1.5;
    bladeWidth *= 0.7;
  }

  const tipX = gripX + Math.cos(angle) * bladeLen;
  const tipY = gripY + Math.sin(angle) * bladeLen;

  // Glow trail for active attacks.
  if (glowAlpha > 0) {
    g.moveTo(gripX, gripY);
    g.lineTo(tipX, tipY);
    g.stroke({ color: glowColor, alpha: glowAlpha, width: bladeWidth + 2.0, cap: "round" });
  }

  // Blade line.
  g.moveTo(gripX, gripY);
  g.lineTo(tipX, tipY);
  g.stroke({ color: bladeColor, alpha: bladeAlpha, width: bladeWidth, cap: "round" });

  // Crossguard for slash weapons (short perpendicular line at grip).
  if (weaponType.includes("slash")) {
    const guardLen = 1.8;
    const guardPerp = angle + Math.PI / 2;
    g.moveTo(
      gripX + Math.cos(guardPerp) * guardLen,
      gripY + Math.sin(guardPerp) * guardLen,
    );
    g.lineTo(
      gripX - Math.cos(guardPerp) * guardLen,
      gripY - Math.sin(guardPerp) * guardLen,
    );
    g.stroke({ color: 0x886644, alpha: bladeAlpha, width: 1.0, cap: "round" });
  }

  // Attack motion arc indicator (committed phase only — shows swing path).
  if (phase === "committed" && e.info.attack_motion) {
    const motion = e.info.attack_motion;
    let arcStart = angle - 0.5;
    let arcEnd = angle + 0.5;
    if (motion === "forehand") {
      arcStart = angle - 0.8;
      arcEnd = angle + 0.2;
    } else if (motion === "backhand") {
      arcStart = angle - 0.2;
      arcEnd = angle + 0.8;
    } else if (motion === "overhead") {
      arcStart = angle - 0.6;
      arcEnd = angle + 0.6;
    }
    // Draw a faint arc to show the sweep path.
    const arcR = bladeLen * 0.8;
    const arcSegs = 8;
    for (let i = 0; i < arcSegs; i++) {
      const t0 = arcStart + (arcEnd - arcStart) * (i / arcSegs);
      const t1 = arcStart + (arcEnd - arcStart) * ((i + 1) / arcSegs);
      g.moveTo(gripX + Math.cos(t0) * arcR, gripY + Math.sin(t0) * arcR);
      g.lineTo(gripX + Math.cos(t1) * arcR, gripY + Math.sin(t1) * arcR);
      g.stroke({
        color: 0xffcc44,
        alpha: 0.25 * (1 - i / arcSegs),
        width: 1.5,
        cap: "round",
      });
    }
  }
}

// --- Mid zoom: stack badges ---

export function drawStackBadges(
  g: Graphics,
  entities: RenderEntity[],
): void {
  g.clear();

  // Bucket by stack when possible so the badge follows actual unit positions.
  const buckets = new Map<string, {
    x: number;
    y: number;
    count: number;
    dominantOwner: number;
  }>();

  for (const e of entities) {
    if (e.state !== "alive") continue;
    if (e.info.entity_kind === "Structure") continue;

    const key = e.info.stack_id != null
      ? `stack:${e.info.stack_id}`
      : `entity:${e.info.id}`;

    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = {
        x: 0,
        y: 0,
        count: 0,
        dominantOwner: e.info.owner ?? 0,
      };
      buckets.set(key, bucket);
    }
    bucket.x += e.pos.x;
    bucket.y += e.pos.y;
    bucket.count++;
    if (e.info.owner != null) bucket.dominantOwner = e.info.owner;
  }

  const size = HEX_SIZE;
  for (const bucket of buckets.values()) {
    if (bucket.count === 0) continue;
    const cx = bucket.x / bucket.count;
    const cy = bucket.y / bucket.count;
    const color = playerColorNum(bucket.dominantOwner);

    const badgeR = size * 0.35;
    g.circle(cx, cy, badgeR);
    g.fill({ color, alpha: 0.85 });
    g.stroke({ color: 0xffffff, width: 0.8 });

    const count = bucket.count;
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

    const [row, col] = pixelToHex(e.pos.x, e.pos.y, HEX_SIZE);
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
