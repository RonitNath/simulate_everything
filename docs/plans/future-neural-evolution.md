# Future: Neural Evolution System

Status: **post-V3 design** — requires Streams E+F as prerequisite infrastructure
This document captures design direction for neuroevolution integration.
Implementation planning happens when V3 agent behavior is stable.

## Vision

Replace hand-tuned classical systems with NEAT-evolved neural networks at five
insertion points. Agents learn in real-time within the simulation — weights
updated across generations, skill expressed as network complexity. A novice
swordsman has a 20-node net. A master has a 200-node net that evolved feints,
distance management, and combo sequences. Cultural evolution emerges as
different lineages develop genuinely different behavioral patterns.

## Prerequisite: classical interface contracts

Each insertion point has a stable interface defined by the classical system
(Streams E+F). The neural net implements the same interface. The classical
system is the bootstrap policy, fallback, and training scaffold.

## Five insertion points

### 1. Utility scoring
- **Interface**: `(EntityNeeds, LocalContext) → Vec<(Goal, f32)>`
- **Classical**: Response curves with geometric mean (E2)
- **Neural**: Small feedforward net, ~32-64 nodes
- **Fitness**: Entity survival duration × need satisfaction × faction contribution
- **Evolution**: Per-entity network inherited from parent with mutation.
  rtNEAT continuous replacement — worst-fitness entity's policy replaced by
  mutated best-fitness offspring on death.
- **Impact**: Population-level behavioral divergence. Different lineages
  prioritize differently. Cultural evolution begins here.

### 2. HTN method selection
- **Interface**: `(EntityState, Vec<HtnMethod>) → MethodIndex`
- **Classical**: Cost/duration heuristic (E3)
- **Neural**: ~32-node net scoring method suitability
- **Fitness**: Plan success rate, task completion efficiency
- **Evolution**: Same rtNEAT pattern. Entities that pick better methods
  complete goals faster, accumulate more fitness.
- **Impact**: Non-obvious method combinations discovered. Classical heuristic
  always picks "shortest path." Evolved net may learn that the longer path
  through allied territory is safer.

### 3. Body control (motor skills)
- **Interface**: `(BodyState, OpponentState, Environment) → JointForces`
- **Classical**: Discrete action menu (AttackMotion enum, block, dodge) from
  Stream A body model
- **Neural**: Continuous joint force output, ~64-200 nodes
- **Fitness**: Combat outcomes (win/loss/damage dealt/survived)
- **Evolution**: Strongest selection pressure of all points. Winners reproduce,
  losers don't. Over generations, martial traditions emerge — not because we
  coded kendo vs fencing, but because different environments (tight spaces vs
  open fields, armored vs unarmored opponents) produce different optima.
- **Impact**: Most visually dramatic. Emergent feints, combos, distance
  management, angle exploitation. Skill IS network complexity.

### 4. Tactical coordination
- **Interface**: `(GroupState, Allies, Enemies) → CoordinationSignals`
- **Classical**: Formation commands + focus-fire targeting (current TacticalLayer)
- **Neural**: ~64-node net per stack leader
- **Fitness**: Group combat outcomes (casualties inflicted vs taken)
- **Evolution**: Stack leaders inherit nets. Successful commanders' tactics
  propagate.
- **Impact**: Emergent formation innovations, flanking patterns, combined arms.

### 5. Social reasoning
- **Interface**: `(PersonalityVec, Relationships, Context) → CommStrategy`
- **Classical**: Opinion dynamics + simple heuristics (E6)
- **Neural**: ~32-node net for social decision making
- **Fitness**: Influence gained, alliance stability, faction loyalty maintained
- **Evolution**: Slower — social fitness evaluates over longer timescales
- **Impact**: Emergent deception, persuasion strategies, coalition building

## Technical approach

### TensorNEAT for batch inference

Variable-topology NEAT networks tensorized into uniform-shaped tensors.
All networks padded to same tensor shape, inactive connections masked.
Batch inference via `vmap` across population — 100k forward passes through
a 64-node net in <1ms on GPU.

Reference: TensorNEAT (EMI-Group, GECCO 2024 Best Paper) — 500x speedup
over NEAT-Python via JAX tensorization.

For our Rust/WASM context: implement tensorization in Rust, compute on CPU
with SIMD. GPU path via wgpu compute shaders (viewer crate already has
wgpu context). Batch size = entities needing decision this tick (~5-15k
after LOD amortization).

### rtNEAT for continuous evolution

Real-time NeuroEvolution of Augmenting Topologies. No generational pause.
On entity death or generation boundary:
1. Evaluate fitness of dying entity
2. Select parent from top performers (tournament selection)
3. Create offspring: parent topology + weights, mutated
4. Replace dying entity's policy with offspring
5. Speciation: protect novel topologies from immediate culling

Reference: rtNEAT (Kenneth Stanley, IEEE TEC 2005) — used in NERO game
for real-time agent evolution during play.

### Cross-generational inheritance

Offspring inherit parent's network topology + weights + mutation:
- Structural mutation: add node, add connection (NEAT's innovation numbers
  prevent structural incompatibility during crossover)
- Weight mutation: Gaussian perturbation of existing weights
- Crossover: two parents with matching genes → offspring with mixed weights

Over hundreds of generations, lineages specialize. This is not designed —
it emerges from differential survival in different environments.

### Variable timestep compatibility

Neural nets are inherently timestep-agnostic. The net sees "current state"
and outputs "next decision," regardless of how much sim-time elapsed between
calls. For batch resolution at strategic tier:
- Net outputs a goal/method choice
- HTN decomposes to action queue
- Batch resolver applies effects + durations
- On next decision tick, net sees updated state

The net never knows whether it's running at 20Hz or 1/hour. The action queue
abstracts this.

## Implementation timeline

### Phase 1: Inference infrastructure
- Tensor format for variable-topology nets in Rust
- Batch forward pass with SIMD
- Integration with entity decision system (trait-based dispatch:
  classical scorer or neural scorer)

### Phase 2: Evolution infrastructure
- Fitness evaluation hooks at each insertion point
- rtNEAT population management (replacement, speciation)
- Cross-generational inheritance (topology + weight transfer)
- Innovation number tracking (NEAT's key contribution)

### Phase 3: Utility scoring nets (insertion point 1)
- First neural insertion — narrowest interface, clearest fitness signal
- A/B comparison: classical vs neural scoring in same simulation
- Validation: evolved scorers outperform hand-tuned response curves

### Phase 4: Body control nets (insertion point 3)
- Most impactful insertion — emergent martial skill
- Requires stable body model (Stream A complete)
- Training environment: adversarial sparring matches
- Validation: evolved fighters develop non-trivial strategies

### Phase 5: Remaining insertion points (2, 4, 5)
- Method selection, coordination, social reasoning
- Each independently evolvable
- Later phases — need stable interfaces from E+F

## Memory budget

| Component | Per-entity | 100k entities |
|-----------|-----------|---------------|
| 64-node net weights | ~2KB | 200MB |
| Fitness accumulator | 16 bytes | 1.6MB |
| Innovation tracking | shared (not per-entity) | ~1MB |
| Speciation metadata | 32 bytes | 3.2MB |
| **Total** | ~2.1KB | ~206MB |

At 64-node nets, memory is dominated by weights. If memory is tight, share
policy networks across archetypes (AgentTorch pattern) — 200 archetypes ×
2KB = 400KB instead of 200MB. Per-entity variation comes from mutation on
reproduction, not per-entity storage.

## What neural nets DON'T replace

- **HTN decomposition engine**: Nets pick goals and methods. The decomposition
  machinery is deterministic infrastructure — no learning needed.
- **Action execution**: Physical simulation (steering, physics, damage) is
  not learned. It's the ground truth the nets learn against.
- **Spatial index / perception**: These are data structures, not decisions.
- **Terrain ops / world model**: Physical simulation, not policy.

The nets replace DECISION FUNCTIONS only. Everything else is simulation
infrastructure that provides the training environment.
