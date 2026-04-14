import {
  Component, createSignal, createEffect, onCleanup, batch, Show,
} from "solid-js";
import type {
  V3Init, V3Snapshot, V3SnapshotDelta, V3ServerToSpectator,
  PlayerInfo,
} from "./v3types";
import type { BiomeName } from "./v2types";
import V3HexCanvas from "./v3/HexCanvas";
import PlaybackControls from "./v3/PlaybackControls";
import ScoreBar from "./v3/ScoreBar";
import LayerToggles, { type V3RenderLayer } from "./v3/LayerToggles";
import Inspector from "./v3/Inspector";
import { applySnapshotDelta } from "./v3/applySnapshotDelta";
import * as css from "./styles/v3.css";

const MAX_FRAMES = 600;
const COMPACT_KEEP_EVERY = 5;

const V3App: Component = () => {
  // --- Connection state ---
  const [phase, setPhase] = createSignal<"connecting" | "playing" | "game_over">("connecting");

  // --- Init data ---
  const [initData, setInitData] = createSignal<V3Init | null>(null);

  // --- Frame buffer ---
  const [frames, setFrames] = createSignal<V3Snapshot[]>([]);
  const [viewIdx, setViewIdx] = createSignal(0);
  const [playing, setPlaying] = createSignal(true);
  const [following, setFollowing] = createSignal(true);
  const [tickMs, setTickMs] = createSignal(100);
  const [serverPaused, setServerPaused] = createSignal(false);
  const [gameNumber, setGameNumber] = createSignal(0);

  // --- Layer toggles ---
  const [layers, setLayers] = createSignal<Set<V3RenderLayer>>(
    new Set(["territory", "roads", "settlements", "depots"]),
  );

  // --- Winner ---
  const [winner, setWinner] = createSignal<number | null>(null);

  // --- Inspector ---
  const [selectedEntityId, setSelectedEntityId] = createSignal<number | null>(null);

  const selectedEntity = () => {
    const id = selectedEntityId();
    if (id == null) return null;
    const frame = currentFrame();
    if (!frame) return null;
    return frame.entities.find((e) => e.id === id) ?? null;
  };

  // Current frame for rendering
  const currentFrame = () => {
    const f = frames();
    const idx = viewIdx();
    return idx < f.length ? f[idx] : null;
  };

  // Players from current frame
  const currentPlayers = (): PlayerInfo[] => {
    return currentFrame()?.players ?? [];
  };

  // Agent names from init
  const agentNames = (): string[] => {
    return initData()?.agent_names ?? [];
  };

  // --- Frame compaction ---
  function compactFrames(frms: V3Snapshot[]): V3Snapshot[] {
    if (frms.length <= MAX_FRAMES) return frms;
    const cutoff = frms.length - 100; // Keep last 100 untouched
    const compacted: V3Snapshot[] = [];
    for (let i = 0; i < cutoff; i++) {
      if (i % COMPACT_KEEP_EVERY === 0) compacted.push(frms[i]);
    }
    for (let i = cutoff; i < frms.length; i++) compacted.push(frms[i]);
    return compacted;
  }

  // --- WebSocket connection ---
  let wsRef: WebSocket | null = null;

  function resync() {
    // Close and reconnect — server sends full catchup (init + snapshot) on new connection
    if (wsRef) {
      wsRef.close();
    }
  }

  createEffect(() => {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/ws/v3/rr`;

    let retryDelay = 500;
    let dead = false;
    let ws: WebSocket | null = null;

    function connect() {
      ws = new WebSocket(wsUrl);
      wsRef = ws;

      ws.onopen = () => {
        retryDelay = 500;
      };

      ws.onmessage = (ev: MessageEvent) => {
        const msg: V3ServerToSpectator = JSON.parse(ev.data);

        switch (msg.type) {
          case "v3_init": {
            batch(() => {
              setInitData(msg);
              setGameNumber(msg.game_number);
              setFrames([]);
              setViewIdx(0);
              setFollowing(true);
              setPlaying(true);
              setPhase("playing");
              setWinner(null);
            });
            break;
          }

          case "v3_snapshot": {
            const snap = msg as V3Snapshot;
            setFrames((prev) => {
              const next = [...prev, snap];
              return compactFrames(next);
            });
            if (following()) {
              setViewIdx(frames().length - 1);
            }
            break;
          }

          case "v3_snapshot_delta": {
            const delta = msg as V3SnapshotDelta;
            setFrames((prev) => {
              const base = prev.length > 0 ? prev[prev.length - 1] : null;
              if (!base) return prev;
              const merged = applySnapshotDelta(base, delta);
              const next = [...prev, merged];
              return compactFrames(next);
            });
            if (following()) {
              setViewIdx(frames().length - 1);
            }
            break;
          }

          case "v3_game_end": {
            batch(() => {
              setPhase("game_over");
              setWinner(msg.winner);
              setPlaying(false);
            });
            break;
          }

          case "v3_config": {
            if (msg.tick_ms != null) setTickMs(msg.tick_ms);
            break;
          }

          case "v3_rr_status": {
            batch(() => {
              setServerPaused(msg.paused);
              setTickMs(msg.tick_ms);
              setGameNumber(msg.game_number);
            });
            break;
          }
        }
      };

      ws.onclose = () => {
        if (!dead) {
          setTimeout(connect, retryDelay);
          retryDelay = Math.min(retryDelay * 2, 2000);
        }
      };

      ws.onerror = () => {};
    }

    connect();

    onCleanup(() => {
      dead = true;
      ws?.close();
    });
  });

  // --- Playback tick ---
  createEffect(() => {
    if (!playing()) return;

    const interval = setInterval(() => {
      const f = frames();
      if (f.length === 0) return;

      if (following()) {
        // Catch up to live
        const behind = f.length - 1 - viewIdx();
        if (behind > 0) {
          const step = behind > 10 ? Math.ceil(behind / 10) : 1;
          setViewIdx((i) => Math.min(i + step, f.length - 1));
        }
      } else {
        // Step forward
        setViewIdx((i) => Math.min(i + 1, f.length - 1));
      }
    }, following() ? 33 : Math.max(tickMs(), 16));

    onCleanup(() => clearInterval(interval));
  });

  // --- REST API helpers ---
  async function setSpeed(ms: number) {
    setTickMs(ms);
    await fetch("/api/v3/rr/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tick_ms: ms }),
    });
  }

  async function serverPause() {
    await fetch("/api/v3/rr/pause", { method: "POST" });
  }

  async function serverResume() {
    await fetch("/api/v3/rr/resume", { method: "POST" });
  }

  async function resetGame() {
    await fetch("/api/v3/rr/reset", { method: "POST" });
  }

  // --- UI callbacks ---
  function togglePlay() {
    setPlaying((p) => !p);
    if (playing()) setFollowing(false);
  }

  function stepForward() {
    setPlaying(false);
    setFollowing(false);
    setViewIdx((i) => Math.min(i + 1, frames().length - 1));
  }

  function seek(tick: number) {
    setFollowing(false);
    setViewIdx(tick);
  }

  function backToLive() {
    setFollowing(true);
    setPlaying(true);
    setViewIdx(frames().length - 1);
  }

  function toggleLayer(layer: V3RenderLayer) {
    setLayers((prev) => {
      const next = new Set(prev);
      if (next.has(layer)) next.delete(layer);
      else next.add(layer);
      return next;
    });
  }

  return (
    <Show
      when={phase() !== "connecting"}
      fallback={<div class={css.v3Connecting}>Connecting to V3 server...</div>}
    >
      <div class={css.v3App}>
        {/* Score bar */}
        <ScoreBar
          players={currentPlayers()}
          agentNames={agentNames()}
          gameNumber={gameNumber()}
        />

        {/* Main area: canvas */}
        <div class={css.v3Main}>
          <div class={css.v3Canvas}>
            <Show when={initData()}>
              {(init) => {
                // V3 backend sends raw height floats, not biome indices.
                // Normalize height_map to [0,1] and derive biomes from height.
                const hm = init().height_map;
                const minH = hm.length > 0 ? Math.min(...hm) : 0;
                const maxH = hm.length > 0 ? Math.max(...hm) : 1;
                const range = maxH - minH || 1;
                const normHeights = hm.map((h: number) => (h - minH) / range);
                const biomes = normHeights.map((h: number): BiomeName => {
                  if (h > 0.85) return "mountain";
                  if (h > 0.7) return "tundra";
                  if (h > 0.5) return "forest";
                  if (h > 0.3) return "grassland";
                  if (h > 0.15) return "steppe";
                  return "desert";
                });
                return (
                  <V3HexCanvas
                    width={init().width}
                    height={init().height}
                    biomes={biomes}
                    heights={normHeights}
                    rivers={[]}
                    frame={currentFrame()}
                    layers={layers()}
                    tickIntervalMs={tickMs()}
                    onEntityClick={(id) => setSelectedEntityId(id)}
                  />
                );
              }}
            </Show>
          </div>

          {/* Inspector panel (right side) */}
          <Inspector
            entity={selectedEntity()}
            onClose={() => setSelectedEntityId(null)}
          />
        </div>

        {/* Footer: playback controls + layer toggles */}
        <div class={css.v3Footer}>
          <LayerToggles layers={layers()} onToggle={toggleLayer} />
          <Show when={winner() != null}>
            <span style={{ color: "#ffd700", "font-weight": "bold", "font-size": "12px" }}>
              Winner: P{winner()}
            </span>
          </Show>
        </div>

        <PlaybackControls
          tick={currentFrame()?.tick ?? 0}
          maxTick={frames().length - 1}
          playing={playing()}
          following={following()}
          tickMs={tickMs()}
          serverPaused={serverPaused()}
          onTogglePlay={togglePlay}
          onStep={stepForward}
          onSeek={seek}
          onBackToLive={backToLive}
          onSetSpeed={setSpeed}
          onServerPause={serverPause}
          onServerResume={serverResume}
          onReset={resetGame}
          onResync={resync}
        />
      </div>
    </Show>
  );
};

export default V3App;
