# Plan: V3 Integration Gaps

**Date:** 2026-04-14
**Scope:** Close the remaining integration seams between Streams A-F.
**Verification:** All 5 builtin scenarios complete in <15s (release), invariants pass.

## Context

All six V3 streams (Phase 0 + A-F) are merged and engine-live. But the modules
were developed independently and several bridge calls are missing. This plan
closes those gaps in one pass.

## Current state

- **Sim tick performance is fine.** `settlement_stability_200` (500 ticks × 5 runs,
  30×30 map, 70+ entities) completes in 0.26s release. The sim is not the bottleneck.
- **Forensic mode is the bottleneck.** Per-tick PNG rendering via tiny-skia takes
  ~50-70ms per frame at 1024×1024. A 300-tick scenario = ~15-20s just rendering.
  This is what causes all forensic-default scenarios to exceed 15s.
- **Farmer scenario panics** at `terrain_ops` JSON serialization ("key must be a
  string") — `HashMap<Axial, ...>` can't be a JSON object key.
- **BehaviorState not allocated** in mapgen — spawn_civilian/spawn_soldier don't
  set `entity.behavior`, so needs/HTN/social are dormant for all mapgen entities.
- **Combat learning bridge missing** — observations drained but never fed to
  damage tables.
- **Strategy ignores terrain** — `TerrainAssessment` computed but never read.

## Tasks

### 1. Fix BehaviorState allocation in mapgen

**Files:** `crates/engine/src/v3/mapgen.rs`

In `spawn_civilian` (around line 180) and `spawn_soldier` (around line 155),
after `spawn_entity` returns, set:
```rust
if let Some(entity) = state.entities.get_mut(key) {
    entity.behavior = Some(Box::new(BehaviorState::default()));
}
```

Also fix `seed_settlement_behavior_state`:
- Line ~327: replace hardcoded `offset_to_axial(1, 1)` with the actual hex
  derived from the settlement's world position via `world_to_hex(center)` or
  the hex mapping utility.

**Test:** Run `settlement_stability_200` — entities should now have non-null
needs in tick snapshots. Run farmer scenario — behavior should activate.

### 2. Wire combat learning bridge

**Files:** `crates/engine/src/v3/damage_table.rs`, `crates/web/src/v3_roundrobin.rs`,
`crates/cli/src/v3bench.rs`

Add a conversion function in `damage_table.rs`:
```rust
pub fn observation_to_matchup(obs: &CombatObservation) -> Option<MatchupObservation> {
    // Map CombatObservation fields to MatchupObservation
    // Return None for unarmored targets
}
```

After `state.combat_log.drain()` in:
- `v3_roundrobin.rs` (~line 343): convert observations, call
  `agent.tactical_mut().damage_table.observe(...)` for each agent whose entities
  participated
- `v3bench.rs` arena paths: same bridge

The tactical and operations layers each own a separate `DamageEstimateTable`.
For now, update both per-agent tables from the same observations. Share later
if needed.

**Test:** Run the 1v1 arena bench with RUST_LOG=debug, verify "damage table
updated" log lines appear after combat resolves.

### 3. Fix forensic mode performance

**Files:** `crates/cli/src/headless_renderer.rs`, `crates/cli/src/v3_behavior_bench.rs`

The per-tick PNG render re-rasterizes the full terrain heightfield every frame.
Cache the terrain base layer:

1. In `headless_renderer.rs`: add a `render_terrain_base(state) -> Pixmap` function
   that renders heightfield + terrain ops (these change rarely).
2. Add a `render_entities_overlay(state, snapshot) -> Pixmap` that renders only
   entities on a transparent pixmap.
3. In the bench loop: render the base once before the tick loop. Each tick,
   composite base + entities overlay. Invalidate base only when `terrain_ops`
   changes.

This should reduce per-frame cost from ~50ms to ~5ms (entity overlay is just a
few circles + text).

Also fix the coordinate mismatch: `world_to_canvas_from_raw` (line ~213) uses
a fixed ±1000 range but `world_to_canvas` uses map dimensions. Unify them.

**Target:** All forensic scenarios complete in <5s release.

### 4. Fix terrain_ops JSON serialization

**Files:** `crates/engine/src/v3/terrain_ops.rs` or `crates/cli/src/v3_behavior_bench.rs`

`HashMap<Axial, Vec<TerrainOp>>` can't serialize to JSON because `Axial` isn't
a valid JSON object key. Options:
- (a) Serialize as `Vec<(Axial, Vec<TerrainOp>)>` in the bench output
- (b) Add a custom serializer that converts Axial to a string key like `"q,r"`

Option (a) is simpler — just change the bench serialization line:
```rust
let ops_list: Vec<_> = state.terrain_ops.ops_by_hex().collect();
serde_json::to_vec_pretty(&ops_list).unwrap()
```

**Test:** Farmer scenario no longer panics in forensic mode.

### 5. Wire strategy to read terrain assessment

**Files:** `crates/engine/src/v3/strategy.rs`

In each personality's `update_posture`/`update_directives`:
- **Turtle:** when `view.terrain.fortification_density < 0.3` and posture is
  Defend, emit `SetEconomicFocus(Infrastructure)`. When `road_coverage < 0.2`,
  bias toward infrastructure before expansion.
- **Spread:** when `view.terrain.damage_density > 0.5`, shift to Infrastructure
  focus temporarily.
- **Striker:** no terrain-driven changes (military-first ignores infrastructure).

These are tunable constants — use named consts at the top of the file.

**Test:** Construct a `StrategicView` with low `fortification_density`, call
Turtle strategy, assert `Infrastructure` focus emitted.

### 6. Starting condition polish

**Files:** `crates/engine/src/v3/mapgen.rs`

After Task 1 (BehaviorState allocation) lands:
- Spawn a basic tool entity (hoe for Farmer, hand-axe for Worker) per civilian
  with `durability` in 0.3-0.7 range, contained in the civilian entity.
- Deepen social seeding in `seed_settlement_behavior_state`: 2-3 relationships
  per entity with scores 5-15 and distinct summaries (neighbor, coworker, family).

**Test:** Check mapgen output — civilians have equipment, social state has >1
relationship per entity.

## Execution order

```
1 (BehaviorState) → 4 (JSON fix) → 3 (renderer perf) → 2 (combat bridge) → 5 (strategy terrain) → 6 (starting conditions)
```

Tasks 1 and 4 are prerequisites for the others to be testable via forensic mode.
Task 3 makes iteration fast. Tasks 2, 5, 6 are independent after that.

## Verification

After all tasks:
```bash
# All scenarios complete in <15s
for s in solo_farmer_harvest 1v1_sword_engagement patrol_responds_to_threat settlement_stability_200 terrain_road_emergence; do
    time target/release/simulate_everything_cli v3behavior --scenario "$s" --forensic --out var/v3bench_verify
done

# Engine tests pass
cargo test -p simulate-everything-engine

# Review bundles build
./scripts/review-scenario.sh solo_farmer_harvest
```

## Doc updates needed after completion

- `docs/v3-engine-audit-2026-04-14.md` — mark all items as landed
- `docs/v3-capability-matrix.md` — update A5, P3 status
- `docs/plans/v3-sequencing.md` — update wave status table
