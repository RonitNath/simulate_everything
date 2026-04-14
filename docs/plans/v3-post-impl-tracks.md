# V3 Post-Implementation: 5 Parallel Tracks

All V3.0 implementation is done. These 5 tracks run in parallel to make the
simulation actually interesting. Each is a standalone prompt for a separate
session.

## Current state

The bench shows the problem: 500-tick games end in draws with 0-1 deaths.
Armies barely engage because:
- Operations routes stacks to hex centers (150m grid), not to actual enemies
- Economy commands (Farm, Build, Craft, Garrison, Train) are stubbed in apply_commands
- Soldiers start 5 per side with iron swords, no production of new equipment
- 15x15 map = 2250m across, armies start far apart, approach speed ~3m/s

Arena mode proves combat works when entities are close. The gap is getting
armies to converge and sustaining a war economy.

---

## Track 1: Arena Replay Capture

**Goal:** Write replay files from CLI arena/bench that the V3 renderer can play back.

**Key files:**
- `crates/cli/src/v3bench.rs` — `run_arena()` and `run_bench_game()` need replay output
- `crates/web/src/v3_protocol.rs` — `build_snapshot()` (line 348), `DeltaTracker::build_delta()` (line 672), `V3Init` (line 112), `V3ServerToSpectator` enum (line 68)
- `frontend/src/V3App.tsx` — WS message handler (line 151+), switches on `v3_init`, `v3_snapshot`, `v3_snapshot_delta`

**What to build:**

1. Add `--replay <path>` flag to v3bench. When present, write a JSONL file:
   - Line 1: `V3ServerToSpectator::Init { ... }` (serialized as JSON)
   - Subsequent lines: `V3ServerToSpectator::Snapshot` for tick 0 (full state),
     then `V3ServerToSpectator::SnapshotDelta` for each subsequent tick

2. In `run_arena()`: create a `DeltaTracker`, call `build_snapshot` for tick 0,
   `build_delta` for subsequent ticks. Write each as a JSON line to the file.
   Same for `run_bench_game()` but only when `--replay` is set.

3. Need to construct a `V3Init` in the CLI. The init needs terrain/height/material
   data from the heightfield. Look at how `v3_roundrobin.rs` builds it.

4. Add a `/v3/replay` route or page in the frontend that:
   - Accepts a JSONL file upload or URL
   - Feeds messages to the same V3App handler as if they came from WS
   - Plays back at configurable speed (tick_ms slider)

**Dependencies:** None. Pure CLI + frontend, no engine changes.

**Verify:** Run `v3bench --arena --replay /tmp/test.jsonl`, then load in browser.

---

## Track 2: Larger Arena Scenarios

**Goal:** 5v5 and 10v10 arena fights with mixed weapons to test tactical target selection.

**Key files:**
- `crates/cli/src/v3bench.rs` — `run_arena()` function
- `crates/engine/src/v3/weapon.rs` — `iron_sword()`, `wooden_bow()` constructors
- `crates/engine/src/v3/armor.rs` — `leather_cuirass()`, `bronze_breastplate()`
- `crates/engine/src/v3/tactical.rs` — `SharedTacticalLayer::assign_targets()`,
  `score_target()` — this is where the damage table matchup reasoning lives

**What to build:**

1. Add `--arena-size N` flag (default 1). Spawns N soldiers per side.
   Place them in a cluster: side A around (50,50), side B around (200,50),
   each soldier offset randomly within a 30m radius.

2. Add `--arena-weapons mixed` flag. When set:
   - Side A: 60% swords, 40% bows
   - Side B: 60% swords, 40% bows
   Give some soldiers `leather_cuirass()` as torso armor.

3. Pre-form one stack per side containing all soldiers (up to 32 members,
   which is the SmallVec capacity on Stack.members).

4. Both sides get striker agents (mutual combat).

5. Output: same tick-by-tick format but summarize per-side (alive count,
   total wounds, avg blood) instead of per-entity.

**What to watch for:**
- Do archers hang back or walk into melee? (Archers should attack from range
  via projectile system — check if tactical layer handles ranged differently)
- Does `score_target` prefer armored or unarmored enemies?
- Does the formation system space out entities or do they clump?

**Dependencies:** None. CLI only.

**Verify:** `v3bench --arena --arena-mode mutual --arena-size 5 --arena-weapons mixed`

---

## Track 3: Economy Loop Wiring

**Goal:** Wire the remaining operational commands so agents can farm, build,
produce equipment, and grow their economy.

**Key files:**
- `crates/cli/src/v3bench.rs` — `apply_commands()` (line ~700). Currently stubs:
  Farm, Build, Craft, Garrison, Train, Idle, DisbandStack, ProduceEquipment,
  EquipEntity, EstablishSupplyRoute, FoundSettlement.
- `crates/engine/src/v3/operations.rs` — `SharedOperationsLayer::execute()` —
  produces the commands that need wiring
- `crates/engine/src/v3/state.rs` — `Entity`, `Structure`, `Resource` components
- `crates/engine/src/v3/mapgen.rs` — spawns starting entities (30 civilians,
  5 soldiers, structures, stockpiles)

**What to wire (priority order):**

1. **AssignTask::Farm** — entity with Farmer role at a Farm structure generates
   food. V2 had cell-level stockpiles. For V3 CLI: increment a per-player food
   float (simple bookkeeping, not entity-level). Track food per player in a
   `Vec<f32>` alongside the game loop.

2. **AssignTask::Train** — idle person becomes a Soldier. Change their `person.role`
   to `Role::Soldier`, add `Combatant::new()` and `Equipment::empty()`.

3. **ProduceEquipment** — workshop produces a weapon/armor entity. Spawn a new
   entity with `weapon_props` or `armor_props`, contained in the workshop.
   Costs material from the per-player stockpile.

4. **EquipEntity** — attach a weapon/armor entity to a soldier's equipment slots.
   Set `entity.equipment.weapon = Some(weapon_key)` and `contain()`.

5. **FormStack** — already wired (done in this session).

6. **DisbandStack** — remove the stack from `state.stacks`.

**What NOT to wire (defer):**
- FoundSettlement, EstablishSupplyRoute — complex placement logic
- Build — structure construction progress

**Also needed:** Per-player food/material tracking in the bench game loop.
Currently mapgen spawns stockpiles as hex-level data (from V2). The simplest
approach: add `food: Vec<f32>` and `material: Vec<f32>` per player to the
bench's game loop state, seeded from mapgen starting values.

**Dependencies:** None. Changes are in apply_commands + bench loop.

**Verify:** Run `v3bench --ascii --seeds 0 --ticks 1000 --size 20x20` and
observe: do soldiers get trained? Do they get weapons? Do stacks form and
march? The ASCII output should show entity counts growing over time.

---

## Track 4: Personality Differentiation

**Goal:** Measure whether Spread, Striker, and Turtle produce meaningfully
different gameplay.

**Key files:**
- `crates/cli/src/v3bench.rs` — bench infrastructure already supports
  `--matchups all` to run every pair
- `crates/engine/src/v3/strategy.rs` — three personalities with different
  posture thresholds and economic focus defaults
- `crates/engine/src/v3/operations.rs` — `route_stacks()` uses posture to
  decide destinations; `assign_tasks()` uses economic focus for role ratios

**What to measure:**

1. Run `--matchups all --seeds 0-99 --ticks 2000 --size 20x20` for all 6 pairs
   (spread-vs-striker, spread-vs-turtle, striker-vs-turtle, plus mirrors).
   Capture JSON output.

2. Parse results and report:
   - Win rate per personality across all matchups
   - Average game length per matchup
   - Average deaths per matchup (proxy for combat engagement)
   - Economic divergence: final entity counts (do spread agents grow faster?)
   - Territory control patterns

3. If all games are draws (likely given current state), diagnose why:
   - Are stacks forming? (check final_soldiers in snapshots)
   - Are stacks routing toward enemies? (deaths > 0?)
   - Is economy growing? (entity counts increasing over snapshots?)

4. Write findings to `docs/v3-personality-report.md` with data.

**Dependencies:** Partially depends on Track 3 (economy wiring). Without economy,
personalities differ only in posture/routing, not in army composition. Run the
measurement first to get a baseline, then re-run after Track 3 lands.

**Verify:** The report should show either meaningful differentiation OR a clear
diagnosis of why games are stalemates.

---

## Track 5: Inspector & Tooltip Components

**Goal:** Click an entity in the renderer to see its details. Hover a hex to
see entity info.

**Key files:**
- `frontend/src/V3App.tsx` — main V3 app, renders PixiJS + SolidJS overlay
- `frontend/src/v3types.ts` — TypeScript types matching V3 protocol
- `crates/web/src/v3_protocol.rs` — `SpectatorEntityInfo` (line 168) defines
  what data is available per entity

**What to build:**

1. **Entity inspector panel** — SolidJS component, shown when an entity is
   clicked in the PixiJS canvas. Displays:
   - Entity ID, owner, role
   - Position (x, y, z), hex (q, r)
   - Blood, stamina (progress bars)
   - Wound list (zone, severity, bleed rate)
   - Equipment (weapon type, armor zones)
   - Stack membership
   - Attack state (target, phase)
   Panel docked to right side, closes on click-away or Escape.

2. **Hex tooltip enhancement** — currently shows hex coords on hover. Add:
   - Entity count at hex
   - Entity names/roles on hover
   - Owner color coding

3. **Click detection** — PixiJS click event on entity sprites. The entity map
   (from R2) already tracks entity sprites by ID. Add hit testing: on canvas
   click, find nearest entity within 20px, dispatch to inspector.

**Data flow:** V3App already stores entity state from snapshots/deltas. The
inspector reads from this store. No new API calls needed.

**Dependencies:** None. Pure frontend, reads existing protocol data.

**Verify:** Open V3 in browser, start a round-robin game, click an entity,
see its stats. Hover a hex, see entity summary.
