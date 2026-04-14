# V3 Capability Matrix

Updated: 2026-04-14

Legend:
- `engine-live` — shared engine/runtime path actively uses it
- `shared-engine-unused` — implemented in shared engine, but not yet wired into the live tick/RR path
- `bench-only` — works only via `v3bench` or other harness code
- `module-only` — primitives/modules/tests exist, but there is no shared execution path using them
- `placeholder` — type/surface exists, but the values are scaffolded or synthetic
- `not-landed` — still missing

The `v3-shared-exec` pass moved V3 command application into the shared engine and made `v3bench` consume it. That changes the baseline for `A*`/`E2`, but it does not yet make the shared sim tick or RR loop execute those commands.

| Item | Status | Notes |
|------|--------|-------|
| S1 | `engine-live` | Spatial types, hex projection, and index are used by the engine tick. |
| S2 | `engine-live` | Collision and spatial primitives are live for current movement/projectile handling. |
| D1 | `engine-live` | Body zones, wounds, bleed, and vitals are live. |
| W1 | `engine-live` | Weapon/armor/material property model is live. |
| R1 | `engine-live` | Frontend/renderer scaffold exists and is used. |
| M1 | `shared-engine-unused` | Core steering primitives exist; live tick uses only a subset. |
| D2 | `engine-live` | Impact resolution pipeline is live. |
| W2 | `engine-live` | Melee attack state and resolution are live. |
| A1 | `engine-live` | Layer types and agent output structure are live. |
| R2 | `engine-live` | Continuous entity rendering is live. |
| M2 | `module-only` | Pathfinding/smoothing/formation-slot modules exist but are not integrated into live routing. |
| W3 | `shared-engine-unused` | Projectile entities and arc physics exist, but the full spec path is still partial. |
| A2 | `shared-engine-unused` | Shared ops layer exists; command application is now engine-owned, but live tick/RR do not execute it yet. |
| E1 | `engine-live` | Entity model, containment, and mapgen are live. |
| P1 | `engine-live` | Core wire types/snapshot surface exist. |
| A3 | `shared-engine-unused` | Tactical reasoning exists; shared executor now applies core tactical mutations, but live runtime does not yet drive it. |
| A4 | `shared-engine-unused` | Strategy personalities exist, but their outputs are not yet executed by sim tick/RR. |
| A5 | `shared-engine-unused` | Damage tables and combat observations exist; learning loop wiring remains partial. |
| E2 | `shared-engine-unused` | Shared command executor now exists, but sim tick still has an agent-command TODO. |
| P2 | `placeholder` | RR/replay surface exists, but RR still validates and drops V3 commands. |
| R3 | `engine-live` | Projectile/wound/equipment presentation support exists in replay/render flows. |
| R4 | `engine-live` | Viewport culling exists. |
| P3 | `placeholder` | Live status surface exists, but some V3 fields remain scaffolded. |
| P4 | `placeholder` | Review/capture surface exists, but depends on partial runtime integration. |
| R5 | `not-landed` | Deferred from V3.0 scope. |

Primary references:
- [docs/plans/v3-sequencing.md](./plans/v3-sequencing.md)
- [docs/v3-engine-audit-2026-04-14.md](./v3-engine-audit-2026-04-14.md)
