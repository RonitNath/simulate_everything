# V3 Terrain Streaming Verification Handoff

Date: 2026-04-14

## Scope

This handoff covers verification and validation status for:

- `f749fab` `feat(v3): add terrain op raster streaming [IA-79]`
- `9ddef4d` `fix(viewer): pad interpolation uniforms for webgpu [IA-79]`

The terrain-op implementation itself is merged on `main`. This document is only about what has and has not been validated end-to-end.

## Current Status

### Verified in code/tests

These checks passed on `main` before the follow-up viewer fix:

- `cargo test -p simulate-everything-engine`
- `cargo test -p simulate-everything-protocol`
- `cargo test -p simulate-everything-web v3_protocol`
- `cargo check -p simulate-everything-viewer`
- `cargo check -p simulate-everything-viewer --target wasm32-unknown-unknown`
- `cargo check --workspace`

After the viewer uniform fix, these checks were rerun and passed:

- `cargo check -p simulate-everything-viewer`
- `cargo check -p simulate-everything-viewer --target wasm32-unknown-unknown`

### Verified in browser/runtime

Using a real local browser path (`chromium` under `Xvfb`), the viewer now:

- acquires a WebGPU adapter
- initializes GPU state successfully
- reaches the `GPU initialized, terrain + hex overlay + entities ready` log in `crates/viewer/src/lib.rs`

The previous blocker was a real WebGPU validation error in `crates/viewer/src/entities.rs`, where the interpolation uniform binding was undersized. That is what `9ddef4d` fixed.

## What Was Observed

### 1. Playwright headless path is not sufficient here

Playwright headless Chromium exposed `navigator.gpu`, but not a usable adapter for this app.

Observed failure:

- `No available adapters.`
- panic from `crates/viewer/src/gpu.rs:29`

Conclusion:

- Do not treat Playwright headless as a reliable visual verification path for this viewer.
- Use a local browser path with an actual WebGPU-capable session.

### 2. Local browser path found a real rendering bug

Using `chromium` inside `Xvfb` with WebGPU/Vulkan flags, the app got a WebGPU adapter and started, but every frame failed due to a uniform-binding validation error:

- `[Buffer "interp uniforms"] ... requires a buffer binding which is at least 64 bytes`

That error came from `crates/viewer/src/entities.rs`, in the interpolation compute pass. The fix was:

- align `InterpolationUniforms`
- add explicit padding fields
- allocate the uniform buffer at `64` bytes

That fix is committed in `9ddef4d`.

### 3. After the fix, the validation error is gone

The same local browser path no longer reports the interpolation-buffer validation error after `9ddef4d`.

This is important because it means the viewer is no longer obviously failing frame submission on startup.

## What Is Still Not Fully Verified

The terrain streaming feature is still not fully validated end-to-end.

The remaining gaps are:

- No confirmed live websocket connection from the standalone Trunk-served viewer to `/ws/v3/rr`
- No confirmed application of live `terrain_patches` into the viewer textures
- No conclusive screenshot showing terrain geometry/material variation after the viewer fix
- No confirmation that init raster origin/cell size line up exactly with live RR terrain in practice

## Why Live RR Was Not Yet Confirmed

The standalone viewer derives its websocket URL from `window.location.host`, so when served by Trunk on `127.0.0.1:8081`, it tries to connect to `/ws/v3/rr` on `:8081`.

Trunk was started with a websocket proxy attempt, but that proxy was misconfigured for ws traffic:

- current attempt used `--proxy-backend http://127.0.0.1:3333 --proxy-rewrite /ws/v3/rr --proxy-ws`
- Trunk logged: `UnsupportedUrlScheme`

Practical implication:

- the standalone viewer can be loaded locally
- but live RR streaming was not actually reaching it through that proxy setup

## Recommended Next Validation Steps

### Option A: Fix the Trunk websocket proxy and validate the standalone viewer

This is the fastest path if the goal is to validate the actual `crates/viewer` app.

Suggested direction:

- use a websocket backend URL instead of `http://...` for the proxied RR socket
- verify that `/ws/v3/rr` on the Trunk origin actually upgrades and forwards to the RR server

Once that works:

1. Start Trunk in `crates/viewer`
2. Load the viewer in a real browser session
3. Confirm entity motion updates over time
4. Trigger a terrain edit source and verify that only patch regions change visually

### Option B: Mount the viewer behind a real app route temporarily

If Trunk proxying remains awkward, expose the wgpu viewer through a proper app route in `crates/web` and let it connect directly to the server-origin websocket.

This avoids the split-origin problem entirely.

Good validation sequence:

1. Serve the viewer from the same origin as `/ws/v3/rr`
2. Join the route in a real browser
3. Confirm `v3_init` terrain raster arrives
4. Confirm subsequent `terrain_patches` update the scene without full rebuild

### Option C: Add explicit in-viewer debug instrumentation

If visual confirmation remains ambiguous, add temporary diagnostics:

- log `V3Init.terrain_raster` dimensions/origin/cell size
- log number of `terrain_patches` applied per delta
- count changed texels in `mutate_terrain_patch`
- add a temporary overlay showing patch rectangles

This would turn validation from “looks correct” into “known patches arrived and were applied”.

## High-Value Checks To Run Next

### Browser/runtime checks

- verify the viewer receives `Init { terrain_raster: ... }`
- verify `SnapshotDelta { terrain_patches: ... }` arrives with non-empty patches
- verify `HeightmapRenderer::mutate_terrain_patch(...)` is called
- verify both height and material textures change after patch application

### Visual checks

- terrain is not a flat clear-color screen
- entity markers move over terrain
- hex overlay remains aligned with terrain after init
- a local terrain edit visibly changes a bounded region rather than rebuilding the whole map
- material coloration changes with patches, not just height

## Relevant Files

- `crates/engine/src/v3/terrain_ops.rs`
- `crates/engine/src/v3/spatial.rs`
- `crates/protocol/src/terrain.rs`
- `crates/web/src/v3_protocol.rs`
- `crates/viewer/src/lib.rs`
- `crates/viewer/src/heightmap.rs`
- `crates/viewer/src/entities.rs`
- `crates/viewer/src/shaders/interpolate.wgsl`

## Bottom Line

The terrain streaming implementation is merged and code-level checks are green.

The viewer had one real WebGPU startup bug, and that bug is now fixed in `9ddef4d`.

What remains is not an obvious engine/protocol correctness failure, but an unfinished runtime validation pass:

- get the standalone viewer connected to live RR cleanly
- confirm real `terrain_patches` arrive
- confirm those patches produce visible terrain/material updates in a real browser session
