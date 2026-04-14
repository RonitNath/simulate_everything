import type {
  SpectatorEntityInfo,
  StackInfo,
  V3Snapshot,
  V3SnapshotDelta,
} from "../v3types";

export function applySnapshotDelta(base: V3Snapshot, delta: V3SnapshotDelta): V3Snapshot {
  const entityMap = new Map<number, SpectatorEntityInfo>();
  for (const entity of base.entities) entityMap.set(entity.id, entity);
  for (const id of delta.entities_removed) entityMap.delete(id);
  for (const entity of delta.entities_appeared) entityMap.set(entity.id, entity);
  for (const update of delta.entities_updated) {
    const existing = entityMap.get(update.id);
    if (!existing) continue;
    entityMap.set(update.id, { ...existing, ...update } as SpectatorEntityInfo);
  }

  const stackMap = new Map<number, StackInfo>();
  for (const stack of base.stacks) stackMap.set(stack.id, stack);
  for (const id of delta.stacks_dissolved) stackMap.delete(id);
  for (const stack of delta.stacks_created) stackMap.set(stack.id, stack);
  for (const update of delta.stacks_updated) {
    const existing = stackMap.get(update.id);
    if (!existing) continue;
    stackMap.set(update.id, { ...existing, ...update });
  }

  return {
    tick: delta.tick,
    dt: delta.dt,
    full_state: delta.full_state,
    entities: Array.from(entityMap.values()),
    projectiles: [
      ...base.projectiles.filter((projectile) => !delta.projectiles_removed.includes(projectile.id)),
      ...delta.projectiles_spawned,
    ],
    stacks: Array.from(stackMap.values()),
    hex_ownership: delta.hex_changes.length > 0
      ? applyHexChanges(base.hex_ownership, delta.hex_changes)
      : base.hex_ownership,
    hex_roads: base.hex_roads,
    hex_structures: base.hex_structures,
    players: delta.players,
  };
}

function applyHexChanges(
  ownership: (number | null)[],
  changes: V3SnapshotDelta["hex_changes"],
): (number | null)[] {
  const next = [...ownership];
  for (const change of changes) {
    if (change.owner !== undefined) next[change.index] = change.owner;
  }
  return next;
}
