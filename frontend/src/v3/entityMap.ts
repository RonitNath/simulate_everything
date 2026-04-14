// Entity map: plain Map<number, EntityState> outside SolidJS reactivity.
// WebSocket writes on each tick, PixiJS render loop reads at 60fps.

import type { SpectatorEntityInfo, ProjectileInfo } from "../v3types";

export interface Vec3 {
  x: number;
  y: number;
  z: number;
}

export interface EntityState {
  info: SpectatorEntityInfo;
  prevPos: Vec3;
  currPos: Vec3;
  prevFacing: number;
  currFacing: number;
  lastTickTime: number;
  state: "alive" | "dying" | "corpse";
  deathTime?: number;
}

export interface ProjectileState {
  info: ProjectileInfo;
  prevPos: Vec3;
  currPos: Vec3;
  lastTickTime: number;
}

const DEATH_ANIM_MS = 300;

// --- Interpolation helpers ---

export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

export function lerpVec3(a: Vec3, b: Vec3, t: number): Vec3 {
  return {
    x: lerp(a.x, b.x, t),
    y: lerp(a.y, b.y, t),
    z: lerp(a.z, b.z, t),
  };
}

/** Shortest-arc angle interpolation (handles wrapping around +-PI). */
export function slerpAngle(a: number, b: number, t: number): number {
  let diff = b - a;
  // Normalize to [-PI, PI]
  while (diff > Math.PI) diff -= 2 * Math.PI;
  while (diff < -Math.PI) diff += 2 * Math.PI;
  return a + diff * t;
}

/** Compute interpolation t for the current render frame. */
export function interpT(lastTickTime: number, tickIntervalMs: number, now: number): number {
  if (tickIntervalMs <= 0) return 1;
  return Math.min(1, Math.max(0, (now - lastTickTime) / tickIntervalMs));
}

/** Get interpolated position for an entity. */
export function getInterpPos(e: EntityState, t: number): Vec3 {
  if (e.state === "corpse") return e.currPos;
  return lerpVec3(e.prevPos, e.currPos, t);
}

/** Get interpolated facing for an entity. */
export function getInterpFacing(e: EntityState, t: number): number {
  if (e.state === "corpse") return e.currFacing;
  return slerpAngle(e.prevFacing, e.currFacing, t);
}

/** Get death animation progress (0 = just died, 1 = animation complete). */
export function getDeathProgress(e: EntityState, now: number): number {
  if (e.state !== "dying" || !e.deathTime) return 1;
  return Math.min(1, (now - e.deathTime) / DEATH_ANIM_MS);
}

// --- Entity Map ---

export class EntityMap {
  entities = new Map<number, EntityState>();
  projectiles = new Map<number, ProjectileState>();

  /** Apply a full snapshot — replaces all entity state. */
  applyFullSnapshot(
    entityInfos: SpectatorEntityInfo[],
    projectileInfos: ProjectileInfo[],
    now: number,
  ): void {
    // Mark all existing entities not in the snapshot
    const presentIds = new Set(entityInfos.map((e) => e.id));

    // Remove entities not in snapshot (unless they're corpses)
    for (const [id, state] of this.entities) {
      if (!presentIds.has(id) && state.state === "alive") {
        this.transitionToDying(state, now);
      }
    }

    // Upsert all entities from snapshot
    for (const info of entityInfos) {
      this.upsertEntity(info, now);
    }

    // Replace all projectiles
    this.projectiles.clear();
    for (const p of projectileInfos) {
      this.projectiles.set(p.id, {
        info: p,
        prevPos: { x: p.x, y: p.y, z: p.z },
        currPos: { x: p.x, y: p.y, z: p.z },
        lastTickTime: now,
      });
    }
  }

  /** Apply a delta update. */
  applyDelta(
    appeared: SpectatorEntityInfo[],
    updated: Array<{ id: number } & Partial<SpectatorEntityInfo>>,
    removed: number[],
    projectilesSpawned: ProjectileInfo[],
    projectilesRemoved: number[],
    now: number,
  ): void {
    // Entities appeared (new)
    for (const info of appeared) {
      this.upsertEntity(info, now);
    }

    // Entities updated (changed fields)
    for (const upd of updated) {
      const existing = this.entities.get(upd.id);
      if (!existing) continue;
      // Merge updated fields into info
      const merged = { ...existing.info, ...upd } as SpectatorEntityInfo;
      this.upsertEntity(merged, now);
    }

    // Entities removed (dead or despawned)
    for (const id of removed) {
      const existing = this.entities.get(id);
      if (!existing) continue;
      if (existing.state === "alive") {
        this.transitionToDying(existing, now);
      }
    }

    // Projectiles spawned
    for (const p of projectilesSpawned) {
      this.projectiles.set(p.id, {
        info: p,
        prevPos: { x: p.x, y: p.y, z: p.z },
        currPos: { x: p.x, y: p.y, z: p.z },
        lastTickTime: now,
      });
    }

    // Projectiles removed
    for (const id of projectilesRemoved) {
      this.projectiles.delete(id);
    }
  }

  /** Upsert a single entity. */
  private upsertEntity(info: SpectatorEntityInfo, now: number): void {
    const existing = this.entities.get(info.id);

    if (existing) {
      // Skip updates for corpses
      if (existing.state === "corpse") return;

      // Shift current to previous
      existing.prevPos = { ...existing.currPos };
      existing.prevFacing = existing.currFacing;

      // Update current
      existing.currPos = { x: info.x, y: info.y, z: info.z };
      existing.currFacing = info.facing ?? existing.currFacing;
      existing.info = info;
      existing.lastTickTime = now;

      // Check for death (blood dropped to 0)
      if (existing.state === "alive" && info.blood != null && info.blood <= 0) {
        this.transitionToDying(existing, now);
      }
    } else {
      // New entity — snap (no lerp on first frame)
      const pos = { x: info.x, y: info.y, z: info.z };
      const facing = info.facing ?? 0;
      const isDead = info.blood != null && info.blood <= 0;

      this.entities.set(info.id, {
        info,
        prevPos: { ...pos },
        currPos: { ...pos },
        prevFacing: facing,
        currFacing: facing,
        lastTickTime: now,
        state: isDead ? "corpse" : "alive",
        deathTime: isDead ? now - DEATH_ANIM_MS : undefined,
      });
    }
  }

  private transitionToDying(state: EntityState, now: number): void {
    state.state = "dying";
    state.deathTime = now;
  }

  /** Advance dying entities to corpse after animation completes. */
  advanceLifecycle(now: number): void {
    for (const state of this.entities.values()) {
      if (
        state.state === "dying" &&
        state.deathTime != null &&
        now - state.deathTime >= DEATH_ANIM_MS
      ) {
        state.state = "corpse";
      }
    }
  }

  /** Clear all state (on new game). */
  clear(): void {
    this.entities.clear();
    this.projectiles.clear();
  }
}
