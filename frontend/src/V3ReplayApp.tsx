import {
  Component,
  For,
  Show,
  batch,
  createEffect,
  createMemo,
  createSignal,
  onCleanup,
} from "solid-js";
import type {
  PlayerInfo,
  V3Init,
  V3ServerToSpectator,
  V3Snapshot,
  V3SnapshotDelta,
} from "./v3types";
import type { BiomeName } from "./v2types";
import V3HexCanvas from "./v3/HexCanvas";
import Inspector from "./v3/Inspector";
import LayerToggles, { type V3RenderLayer } from "./v3/LayerToggles";
import PlaybackControls from "./v3/PlaybackControls";
import ScoreBar from "./v3/ScoreBar";
import { applySnapshotDelta } from "./v3/applySnapshotDelta";
import { pixelToHex, type HexRegion, worldToCanvas, HEX_SIZE } from "./v3/render/grid";
import * as css from "./styles/v3.css";

interface ReplayFileEntry {
  name: string;
  path: string;
  size_bytes: number;
  modified_unix_ms: number;
}

const V3ReplayApp: Component = () => {
  const [initData, setInitData] = createSignal<V3Init | null>(null);
  const [frames, setFrames] = createSignal<V3Snapshot[]>([]);
  const [viewIdx, setViewIdx] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [tickMs, setTickMs] = createSignal(100);
  const [winner, setWinner] = createSignal<number | null>(null);
  const [selectedEntityId, setSelectedEntityId] = createSignal<number | null>(null);
  const [status, setStatus] = createSignal("Select a replay from the server list or upload a JSONL file.");
  const [availableReplays, setAvailableReplays] = createSignal<ReplayFileEntry[]>([]);
  const [layers, setLayers] = createSignal<Set<V3RenderLayer>>(
    new Set(["territory", "roads", "settlements", "depots"]),
  );

  const currentFrame = () => {
    const all = frames();
    const idx = viewIdx();
    return idx >= 0 && idx < all.length ? all[idx] : null;
  };

  const selectedEntity = () => {
    const id = selectedEntityId();
    if (id == null) return null;
    return currentFrame()?.entities.find((e) => e.id === id) ?? null;
  };

  const currentPlayers = (): PlayerInfo[] => currentFrame()?.players ?? [];
  const agentNames = (): string[] => initData()?.agent_names ?? [];

  const focusRegion = createMemo<HexRegion | null>(() => {
    const init = initData();
    const replayFrames = frames();
    if (!init || replayFrames.length === 0) return null;

    let minRow = Infinity;
    let maxRow = -Infinity;
    let minCol = Infinity;
    let maxCol = -Infinity;

    for (const frame of replayFrames) {
      for (const entity of frame.entities) {
        const [x, y] = worldToCanvas(entity.x, entity.y);
        const [row, col] = pixelToHex(x, y, HEX_SIZE);
        minRow = Math.min(minRow, row);
        maxRow = Math.max(maxRow, row);
        minCol = Math.min(minCol, col);
        maxCol = Math.max(maxCol, col);
      }
    }

    if (!Number.isFinite(minRow)) return null;

    const margin = 2;
    return {
      minRow: Math.max(0, minRow - margin),
      maxRow: Math.min(init.height - 1, maxRow + margin),
      minCol: Math.max(0, minCol - margin),
      maxCol: Math.min(init.width - 1, maxCol + margin),
    };
  });

  const terrainData = createMemo(() => {
    const init = initData();
    if (!init) return null;
    const hm = init.height_map;
    const minH = hm.length > 0 ? Math.min(...hm) : 0;
    const maxH = hm.length > 0 ? Math.max(...hm) : 1;
    const range = maxH - minH || 1;
    const heights = hm.map((h) => (h - minH) / range);
    const biomes = heights.map((h): BiomeName => {
      if (h > 0.85) return "mountain";
      if (h > 0.7) return "tundra";
      if (h > 0.5) return "forest";
      if (h > 0.3) return "grassland";
      if (h > 0.15) return "steppe";
      return "desert";
    });
    return { heights, biomes };
  });

  async function parseReplayText(text: string, label: string) {
    const lines = text
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);

    let init: V3Init | null = null;
    let winnerValue: number | null = null;
    const replayFrames: V3Snapshot[] = [];

    for (const line of lines) {
      const msg = JSON.parse(line) as V3ServerToSpectator;
      switch (msg.type) {
        case "v3_init":
          init = msg;
          break;
        case "v3_snapshot":
          replayFrames.push(msg);
          break;
        case "v3_snapshot_delta": {
          const base = replayFrames[replayFrames.length - 1];
          if (!base) throw new Error("replay delta appeared before first snapshot");
          replayFrames.push(applySnapshotDelta(base, msg));
          break;
        }
        case "v3_game_end":
          winnerValue = msg.winner;
          break;
        default:
          break;
      }
    }

    if (!init) throw new Error("replay missing v3_init");
    if (replayFrames.length === 0) throw new Error("replay missing snapshots");

    batch(() => {
      setInitData(init);
      setFrames(replayFrames);
      setViewIdx(0);
      setPlaying(false);
      setWinner(winnerValue);
      setSelectedEntityId(null);
      setStatus(`Loaded ${label} with ${replayFrames.length} frames.`);
    });
  }

  async function loadReplayFile(file: File) {
    const text = await file.text();
    await parseReplayText(text, file.name);
  }

  async function fetchReplayList() {
    try {
      const res = await fetch("/api/v3/replay/files");
      if (!res.ok) throw new Error(`list request failed: ${res.status}`);
      const data = await res.json() as ReplayFileEntry[];
      setAvailableReplays(data);
    } catch (err) {
      console.error(err);
      setStatus(`Failed to fetch replay list: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  async function loadReplayPath(path: string, label: string) {
    try {
      const res = await fetch(`/api/v3/replay/file?path=${encodeURIComponent(path)}`);
      if (!res.ok) throw new Error(`load request failed: ${res.status}`);
      const text = await res.text();
      await parseReplayText(text, label);
    } catch (err) {
      console.error(err);
      setStatus(`Failed to load replay: ${err instanceof Error ? err.message : String(err)}`);
    }
  }

  function onFileInput(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    loadReplayFile(file).catch((err) => {
      console.error(err);
      setStatus(`Failed to load replay: ${err instanceof Error ? err.message : String(err)}`);
    });
  }

  function togglePlay() {
    if (!frames().length) return;
    setPlaying((p) => !p);
  }

  function stepForward() {
    setPlaying(false);
    setViewIdx((i) => Math.min(i + 1, frames().length - 1));
  }

  function seek(idx: number) {
    setPlaying(false);
    setViewIdx(idx);
  }

  function jumpToEnd() {
    setPlaying(false);
    setViewIdx(Math.max(0, frames().length - 1));
  }

  function toggleLayer(layer: V3RenderLayer) {
    setLayers((prev) => {
      const next = new Set(prev);
      if (next.has(layer)) next.delete(layer);
      else next.add(layer);
      return next;
    });
  }

  const onKey = (e: KeyboardEvent) => {
    if ((e.target as HTMLElement).tagName === "INPUT") return;
    switch (e.key) {
      case " ":
        e.preventDefault();
        togglePlay();
        break;
      case "ArrowRight":
        stepForward();
        break;
      case "End":
        jumpToEnd();
        break;
      case "ArrowLeft":
        setPlaying(false);
        setViewIdx((i) => Math.max(i - 1, 0));
        break;
      default:
        break;
    }
  };

  window.addEventListener("keydown", onKey);
  onCleanup(() => window.removeEventListener("keydown", onKey));

  createEffect(() => {
    void fetchReplayList();
  });

  createEffect(() => {
    if (!playing()) return;
    const id = window.setInterval(() => {
      setViewIdx((i) => {
        const max = frames().length - 1;
        if (i >= max) {
          setPlaying(false);
          return i;
        }
        return i + 1;
      });
    }, Math.max(tickMs(), 16));
    onCleanup(() => window.clearInterval(id));
  });

  return (
    <div class={css.v3App}>
      <div class={css.v3Header}>
        <div>
          <div class={css.v3Title}>V3 Replay Viewer</div>
          <div style={{ "font-size": "12px", color: "#8a90a5" }}>{status()}</div>
          <div style={{ display: "flex", gap: "6px", "flex-wrap": "wrap", "margin-top": "8px", "max-width": "900px" }}>
            <For each={availableReplays()}>
              {(replay) => (
                <button
                  class={css.v3Btn}
                  onClick={() => void loadReplayPath(replay.path, replay.name)}
                  title={replay.path}
                  style={{ "font-size": "11px", padding: "4px 8px" }}
                >
                  {replay.name}
                </button>
              )}
            </For>
          </div>
        </div>
        <div style={{ display: "flex", gap: "12px", "align-items": "center" }}>
          <label class={css.v3Btn} style={{ display: "inline-flex", "align-items": "center", gap: "8px" }}>
            <span>Load JSONL</span>
            <input type="file" accept=".jsonl,application/json,text/plain" onChange={onFileInput} />
          </label>
        </div>
      </div>

      <Show
        when={initData() && currentFrame() && terrainData()}
        fallback={<div class={css.v3Connecting}>{status()}</div>}
      >
        <ScoreBar
          players={currentPlayers()}
          agentNames={agentNames()}
          gameNumber={0}
        />

        <div class={css.v3Main}>
          <div class={css.v3Canvas}>
            <V3HexCanvas
              width={initData()!.width}
              height={initData()!.height}
              biomes={terrainData()!.biomes}
              heights={terrainData()!.heights}
              rivers={[]}
              frame={currentFrame()}
              layers={layers()}
              tickIntervalMs={tickMs()}
              onEntityClick={(id) => setSelectedEntityId(id)}
              focusRegion={focusRegion()}
            />
          </div>

          <Inspector
            entity={selectedEntity()}
            onClose={() => setSelectedEntityId(null)}
          />
        </div>

        <div class={css.v3Footer}>
          <LayerToggles layers={layers()} onToggle={toggleLayer} />
          <Show when={winner() != null}>
            <span style={{ color: "#ffd700", "font-weight": "bold", "font-size": "12px" }}>
              Winner: P{winner()}
            </span>
          </Show>
        </div>

        <PlaybackControls
          tick={viewIdx()}
          maxTick={Math.max(0, frames().length - 1)}
          playing={playing()}
          following={false}
          tickMs={tickMs()}
          serverPaused
          onTogglePlay={togglePlay}
          onStep={stepForward}
          onSeek={seek}
          onBackToLive={jumpToEnd}
          onSetSpeed={setTickMs}
          onServerPause={() => {}}
          onServerResume={() => {}}
          onReset={() => {}}
          onResync={jumpToEnd}
          showServerControls={false}
        />
      </Show>
    </div>
  );
};

export default V3ReplayApp;
