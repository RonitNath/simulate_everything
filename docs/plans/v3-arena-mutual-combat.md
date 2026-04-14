# V3 Arena: Mutual Combat Scenario

## Context

The arena duel mode (`v3bench --arena`) works end-to-end. A null-agent soldier
stands still while a striker-agent soldier approaches, attacks, wounds, and
kills it over 43 ticks. The full combat loop is validated:
movement → melee approach → attack → wound → bleed → death.

## Task: Mutual Combat (both agents fight back)

### What to implement

Modify `run_arena()` in `crates/cli/src/v3bench.rs` to support a second
scenario: **both soldiers have active tactical agents**. When soldier B attacks
soldier A, A should counter-attack.

#### Specific changes:

1. **Add `--arena-mode` flag** with values `null-vs-striker` (current default)
   and `mutual`. In `run_arena()`, check this flag.

2. **`mutual` mode**: Both players get a striker agent with SharedTacticalLayer.
   Both soldiers get initial waypoints toward each other. Pre-form stacks for
   both. Expected behavior: they converge, trade blows, one dies from
   accumulated wounds.

3. **Verify the counter-attack chain**: When A has an active tactical agent and
   B attacks, A's tactical layer should issue `TacticalCommand::Attack` back at
   B. The `apply_commands` function already handles this. The key question is
   whether the tactical layer fires for A's stack — it should, because both
   stacks are within ENGAGEMENT_RADIUS (300m) of each other.

4. **Print outcome summary**: After the loop, print wound counts, blood levels,
   who won and by how much. Show whether it was one-sided or competitive.

### Key files

- `crates/cli/src/v3bench.rs` — `run_arena()` function (~line 700), `make_agent()`, flag parsing
- `crates/engine/src/v3/sim.rs` — melee approach steering (entities with active attacks steer toward target)
- `crates/engine/src/v3/tactical.rs` — `SharedTacticalLayer::assign_targets` picks best target per stack member

### What to watch for

- **Separation oscillation**: Two attacking entities might oscillate around each
  other if both steer toward the other. The melee approach steering uses
  `arrive` with a 2.0m slow radius — they should converge to ~0m and stay
  there trading blows.

- **Attack cooldown overlap**: Both attacking simultaneously means they take
  turns hitting each other. With iron sword: 4 tick windup + ~3 tick cooldown.
  If both start on the same tick, they'll hit each other on tick 4, then
  cooldown together, creating synchronized combat.

- **Stagger**: If a hit staggers the defender mid-windup, the defender's attack
  should degrade (committed) or cancel (uncommitted). Check that stagger
  propagates correctly.

### Build and test

```bash
cargo build --release --bin simulate_everything_cli
./target/release/simulate_everything_cli v3bench --arena --arena-mode mutual
```

Expected: both soldiers wound each other, one dies first from accumulated bleed.
The fight should last 60-100+ ticks since both are dealing damage.

### Commit message

`feat(cli): add mutual combat arena mode — both soldiers fight back`

### Don't change

- Engine code (sim.rs, tactical.rs, etc.) — arena-only changes in v3bench.rs
- The existing `null-vs-striker` mode (keep it as default)
