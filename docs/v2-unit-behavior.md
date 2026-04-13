# V2 Unit Behavior Research and Overhaul Plan

> This document records the design reasoning from the April 2026 unit-behavior review. It exists to preserve the research, tradeoffs, and proposed next steps for V2 movement/combat behavior without relying on chat history.

## Summary

The current V2 simulation should stay **hex-first** and **fixed-step**, but unit and agent behavior on those hexes should become more deterministic, stable, and tactically legible.

The immediate trigger for this review was a movement/combat rules gap: enemy units can currently end up on the same hex because V2 movement applies one-step position updates without any occupancy or conflict-resolution pass. That produces behavior that is both strategically muddy and visually confusing.

After reviewing the current engine plus external RTS/hex best practices, the recommended direction is:

- Keep the hex topology and ticked simulation.
- Add a simultaneous movement-intent resolution phase.
- Enforce single-occupancy for military units after resolution.
- Add limited hybrid auto-lock for obvious contact situations.
- Improve agent positioning with hysteresis, local spatial scoring, and better role/task memory.

This is a better fit than either:

- adding finer-grained timestamp ordering, which increases complexity without solving the core clarity problem, or
- switching to continuous positions, which would force a much larger redesign of combat, walls, roads, forts, vision, and agent reasoning.

## Why Not Continuous Positions

The long-term roadmap is explicitly centered on a hex-based RTS where strategic legibility comes from:

- edge-based engagements
- roads and supply corridors
- depots and frontier logistics
- wall edges, gates, trench lines, and siege lines
- region identity and map-shape-driven warfare

Those systems are a natural fit for hex topology and much less natural in a fully continuous engine.

Continuous positions would help with:

- smoother movement
- convoy interception
- lane congestion
- cavalry/scout differentiation
- message-delay and road-throughput simulation

But it would also force redesign of:

- current edge-based engagement rules
- future wall and gate systems
- cheap region/influence-map agent reasoning
- spectator readability
- deterministic replay and harness throughput

The key insight from this review is that the current problem is **not** a lack of sub-tick precision. The problem is that movement resolution is under-specified. The right fix is better move resolution and better local behavior, not a genre shift.

## Current V2 Issues

### 1. No movement conflict resolution

Today, `move_units` updates unit positions directly once a unit is eligible to move. There is no reservation, contest, or occupancy resolution pass, so multiple units can land on the same hex.

This causes:

- enemy stacking with no clear tactical meaning
- confusing spectator states
- reduced value of fronts and choke points
- weak tactical signals for agents

### 2. Destination churn

The current agents are lightweight placeholders. They frequently recompute broad destinations using simple lane/sector heuristics. That is fast, but it tends to produce unstable movement intent and under-expressive army behavior.

Symptoms:

- units drift rather than hold coherent fronts
- rally behavior is brittle
- local repositioning is simplistic
- movement can oscillate when multiple options are similar

### 3. Weak front definition

Because movement is permissive and local positioning is crude, the map lacks a clear concept of:

- contested fronts
- blocked advances
- reserves
- screening units
- reinforcement lines

That undermines both gameplay and watchability.

### 4. Tactical decisions are too binary

The current tactical layer mostly asks:

- can I engage now?
- should I disengage now?

It does not do enough of the in-between work:

- choose a support hex
- hold rather than advance
- reinforce an ally already anchoring a front
- shift laterally to improve local odds
- decline a bad path through enemy influence

## Research Notes

The external material reviewed during this pass consistently pointed toward the same practical principles:

### Fixed-step simulation beats finer ordering

For multiplayer-style or replayable deterministic simulations, stable fixed-step updates are preferable to “higher precision” ordering tricks. Finer timestamp ordering tends to create complexity and hidden fairness problems instead of solving game-rule ambiguity.

Applied to V2:

- stay fixed-step
- resolve simultaneous intents explicitly
- prefer deterministic conflict resolution over time-order escalation

### Simultaneous resolution improves tactical clarity

Turning “every unit mutates the world in sequence” into “everyone declares intent, then the engine resolves conflicts” produces more readable tactics and fewer rule accidents.

Applied to V2:

- collect candidate one-step moves first
- resolve same-target contests intentionally
- convert obvious blocked contacts into combat states

### Good unit behavior depends on position scoring, not just pathfinding

Strong RTS behavior is often less about finding any path and more about selecting good local positions:

- support positions
- staging points
- reserve lines
- fallback hexes
- non-clumping destinations

Applied to V2:

- score nearby hexes using local support/threat/terrain/road/supply context
- preserve destination inertia unless there is a meaningful improvement

### Hysteresis is required to prevent oscillation

If agents always switch to the best currently scored option, they will thrash whenever options are close.

Applied to V2:

- keep the current task/destination unless a new option exceeds it by a threshold
- treat task stability as a feature, not wasted responsiveness

### Influence and control maps are worth the cost

Local influence maps and zone-of-control style rules are one of the cleanest ways to make fronts emerge without scripting them.

Applied to V2:

- build lightweight local tactical scores near units
- add a small movement-facing control model so defended lines feel defended

## Recommended Direction

Status: Phase 1 of this direction is now implemented in the engine. V2 movement uses simultaneous one-step intent resolution, prevents same-hex overlap through normal movement, auto-locks obvious movement-contact cases, and applies a light movement-facing zone of control.

### 1. Add simultaneous movement resolution

Introduce a movement phase with three stages:

1. Gather one-step intents for all eligible units.
2. Resolve conflicts deterministically.
3. Commit only the winning moves.

Desired properties:

- no enemy same-hex stacking
- no arbitrary dependence on iteration order
- deterministic outcomes for replay and benchmarking
- understandable spectator outcomes

Recommended baseline resolution rules:

- units already holding a hex keep it unless displaced by explicit combat/capture rules
- same-target friendly moves: one enters, the rest wait or remain in place
- same-target enemy moves: contest triggers contact rather than overlap
- swap attempts across the same edge should be treated as contact
- stable tiebreaks should use explicit rule priority plus unit id, never hashmap order

### 2. Add hybrid auto-lock for obvious contact

Keep explicit `Engage` / `Disengage` as the main tactical API, but auto-create engagement in a small number of obvious cases:

- two enemies contest the same destination hex
- two enemies attempt to pass through each other across the same edge
- a unit advances into a strongly held enemy-adjacent front and is blocked

This keeps combat legible without turning V2 into full auto-combat.

### 3. Add a movement-facing zone of control

This should be light-touch and movement-oriented, not a hard simulation of historical frontage.

Suggested behavior:

- enemy-adjacent hexes are legal to enter
- deeper movement through enemy-controlled space in the same step is blocked or heavily deprioritized
- routing through threatened hexes should be worse than routing around them

The purpose is to make fronts, screens, and choke defense visible on the board.

### 4. Stabilize agent intent

Agents should retain:

- a current task
- a current destination
- a short-term role

Only replace them when:

- the task completes
- the position becomes invalid
- the strategic posture changes
- a materially better option appears

This prevents movement churn and makes armies look intentional.

### 5. Upgrade local tactical positioning

Before issuing a `Move`, the agent should score a local candidate set of nearby hexes rather than always aiming at broad global targets.

Candidate scoring should consider:

- distance to assigned objective
- nearby friendly support
- nearby enemy threat
- roads and movement ease
- rations / retreat urgency
- terrain / height advantage
- crowding / friendly overconcentration
- settlement proximity for fallback behavior

The point is not expensive global search. The point is better local hex choice.

### 6. Move toward Centurion incrementally

The current agents are placeholders; the long-term architecture already calls for strategic, operational, and tactical layers.

Behavior work should move in that direction incrementally:

- strategic posture and front selection every ~50 ticks
- task assignment every poll
- tactical decisions only for units near enemies

Do not jump straight to a fully elaborate architecture before the movement rules are trustworthy.

## Phased Overhaul Plan

### Phase 1: Fairness and clarity

- Add simultaneous one-step move resolution.
- Enforce single-occupancy for military units.
- Add deterministic contest handling.
- Add hybrid auto-lock for same-edge / same-target contacts.
- Add tests for conflict resolution and replay determinism.

This phase addresses the specific concern that started the review.

### Phase 2: Stable positioning

- Add per-unit task/destination memory.
- Add hysteresis thresholds.
- Replace broad destination churn with local candidate scoring.
- Add friendly spacing pressure to reduce self-clumping.

This phase should make units look much more intentional.

### Phase 3: Better fronts and support behavior

- Add a movement-facing zone-of-control model.
- Add reserve, line-holder, and support task categories.
- Add better reinforcement and retreat behavior.
- Improve rally and strike group formation.

This phase should make battles look more coherent and easier to read.

### Phase 4: Centurion-aligned behavior

- Fold the improved movement/combat rules into the planned strategic/operational/tactical split.
- Promote influence maps and region priorities from concept to implementation.
- Keep the per-poll compute budget under control.

## Acceptance Criteria

The overhaul is successful when:

- enemy units never end a tick stacked on the same hex
- contested movement resolves the same way every run
- swap/contact situations produce clear combat outcomes
- units stop oscillating between equivalent destinations
- rally groups form and hold shape more reliably
- fronts visibly resist casual slip-through movement
- spectators can explain why a blocked or contested move resolved the way it did

## Suggested Engine/Agent Changes

These are concrete code-level targets for implementation planning:

- `crates/engine/src/v2/sim.rs`
  - add intent gathering and move resolution before applying movement
  - add conflict/contact handling for same-target and same-edge contests
- `crates/engine/src/v2/combat.rs`
  - support auto-created engagements from movement conflicts
- `crates/engine/src/v2/agent.rs`
  - add task/destination persistence
  - add local hex scoring and hysteresis
  - add support/reserve/front behaviors
- `docs/architecture.md`
  - update behavior docs once movement resolution rules change

## Sources

The following references informed this review:

- Glenn Fiedler, “Fix Your Timestep!”
  - https://gafferongames.com/post/fix_your_timestep/
- Dave Pottinger, “Implementing Coordinated Unit Movement”
  - https://media.gdcvault.com/GD_Mag_Archives/GDM_January_1999.pdf
- Eric Johnson, “Taming Spatial Queries: Tips for Natural Position Selection”
  - https://www.gameaipro.com/GameAIProOnlineEdition2021/GameAIProOnlineEdition2021_Chapter05_Taming_Spatial_Queries_Tips_for_Natural_Position_Selection.pdf
- Matthias Siemonsmeier, “Gearing the Tactics Genre: Simultaneous AI Actions in Gears Tactics”
  - https://www.gameaipro.com/GameAIProOnlineEdition2021/GameAIProOnlineEdition2021_Chapter03_Gearing_the_Tactics_Genre_Simultaneous_AI_Actions_in_Gears_Tactics.pdf
- Dave Mark, “Modular Tactical Influence Maps”
  - https://www.gameaipro.com/GameAIPro2/GameAIPro2_Chapter30_Modular_Tactical_Influence_Maps.pdf
- Red Blob Games, “Zone of Control pathfinding”
  - https://www.redblobgames.com/x/1920-zone-of-control-paths/

## Final Recommendation

Do not switch V2 to continuous positions.

Fix the real problem instead:

- resolve movement simultaneously
- make fronts and contests explicit
- keep combat mostly explicit but auto-lock obvious contact
- make agents choose and keep better hexes

That path fits the existing roadmap, improves watchability immediately, and preserves the long-term value of the hex-based design.
