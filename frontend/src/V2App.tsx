import { Component, createEffect, createMemo, createSignal, For, onCleanup, Show, batch } from "solid-js";
import HexCanvas from "./HexCanvas";
import type { RenderLayer } from "./HexCanvas";
import Nav from "./Nav";
import type {
  BoardFrameData,
  BoardHexHover,
  BoardStaticData,
  V2Frame,
  V2GameInfo,
  V2HexDelta,
  V2ReviewBundle,
  V2ReviewBundleSummary,
  V2ReviewListResponse,
  V2ReviewStatus,
  V2Settlement,
  V2SpectatorPlayer,
  V2UnitSnapshot,
} from "./v2types";
import { normalizeGameInfoStatic, normalizeReplayFrame, normalizeReplayStatic, normalizeWsFrame } from "./v2types";
import * as styles from "./styles/app.css";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

type Phase =
  | { kind: "connecting" }
  | { kind: "playing"; game: V2GameInfo }
  | { kind: "game_over"; game: V2GameInfo; winner: number | null; tick: number; timedOut: boolean };

const SPEED_PRESETS = [
  { label: "0.5x", ms: 500 },
  { label: "1x", ms: 250 },
  { label: "2x", ms: 125 },
  { label: "4x", ms: 60 },
  { label: "10x", ms: 25 },
  { label: "Max", ms: 10 },
];

const ALL_LAYERS: RenderLayer[] = ["territory", "roads", "settlements", "convoys", "destinations"];

function emptyFrame(game: V2GameInfo, tick: number): V2Frame {
  const cellCount = game.width * game.height;
  return {
    tick,
    entities: [],
    units: [],
    convoys: [],
    territory: Array.from({ length: cellCount }, () => null),
    roads: Array.from({ length: cellCount }, () => 0),
    depots: Array.from({ length: cellCount }, () => false),
    population: [],
    engagements: [],
    hex_ownership: Array.from({ length: cellCount }, () => null),
    road_levels: Array.from({ length: cellCount }, () => 0),
    settlements: [],
    players: Array.from({ length: game.num_players }, (_, id) => ({
      id,
      alive: true,
      population: 0,
      territory: 0,
      food_level: 0,
      material_level: 0,
    })),
  };
}

function applyHexChanges(
  frame: V2Frame,
  hexChanges: V2HexDelta[],
  width: number,
): { territory: (number | null)[]; roads: number[]; settlements: V2Settlement[] } {
  const territory = (frame.territory ?? frame.hex_ownership ?? []).slice();
  const roads = (frame.roads ?? frame.road_levels ?? []).slice();
  const settlementMap = new Map<string, V2Settlement>((frame.settlements ?? []).map((s) => [`${s.q},${s.r}`, s] as const));

  for (const hex of hexChanges) {
    territory[hex.index] = hex.owner;
    roads[hex.index] = hex.road_level;
    const row = Math.floor(hex.index / width);
    const col = hex.index % width;
    const q = col - Math.floor((row - (row & 1)) / 2);
    const r = row;
    const key = `${q},${r}`;
    if (hex.has_settlement && hex.settlement_owner !== null) {
      settlementMap.set(key, { q, r, owner: hex.settlement_owner });
    } else {
      settlementMap.delete(key);
    }
  }

  return {
    territory,
    roads,
    settlements: Array.from(settlementMap.values()),
  };
}

/** Max frames to keep in memory. Older frames beyond this are compacted. */
const MAX_FRAMES = 600;
/** When compacting, keep every Nth old frame to allow coarse scrubbing. */
const COMPACT_KEEP_EVERY = 5;

const V2App: Component = () => {
  const [phase, setPhase] = createSignal<Phase>({ kind: "connecting" });
  const [frames, setFrames] = createSignal<V2Frame[]>([]);
  const [viewIdx, setViewIdx] = createSignal(0);
  const [following, setFollowing] = createSignal(true);
  const [playing, setPlaying] = createSignal(true);
  const [serverPaused, setServerPaused] = createSignal(false);
  const [tickMs, setTickMs] = createSignal(250);
  const [showNumbers, setShowStrength] = createSignal(true);
  const [gameNumber, setGameNumber] = createSignal(0);
  const [liveTick, setLiveTick] = createSignal<number | null>(null);
  const [capturableStartTick, setCapturableStartTick] = createSignal<number | null>(null);
  const [capturableEndTick, setCapturableEndTick] = createSignal<number | null>(null);
  const [activeCapture, setActiveCapture] = createSignal<V2ReviewBundleSummary | null>(null);
  const [pendingReviews, setPendingReviews] = createSignal<V2ReviewBundleSummary[]>([]);
  const [savedReviews, setSavedReviews] = createSignal<V2ReviewBundleSummary[]>([]);
  const [reviewBundle, setReviewBundle] = createSignal<V2ReviewBundle | null>(null);
  const [reviewFrameIdx, setReviewFrameIdx] = createSignal(0);
  const [reviewLoading, setReviewLoading] = createSignal(false);
  const [flagError, setFlagError] = createSignal<string | null>(null);
  const [pendingHoverHex, setPendingHoverHex] = createSignal<BoardHexHover | null>(null);
  const [resolvedHoverHex, setResolvedHoverHex] = createSignal<BoardHexHover | null>(null);
  const [layers, setLayers] = createSignal<Set<RenderLayer>>(
    new Set(["territory", "roads", "settlements", "convoys"]),
  );

  let wsRef: WebSocket | null = null;

  const sendSpeed = (ms: number) => {
    setTickMs(ms);
    fetch("/api/v2/rr/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tick_ms: ms }),
    });
  };

  const toggleServerPause = () => {
    const endpoint = serverPaused() ? "/api/v2/rr/resume" : "/api/v2/rr/pause";
    fetch(endpoint, { method: "POST" });
  };

  const restartGame = () => {
    fetch("/api/v2/rr/reset", { method: "POST" });
  };

  const refreshStatus = async () => {
    const res = await fetch("/api/v2/rr/status");
    const data: V2ReviewStatus = await res.json();
    setServerPaused(!!data.paused);
    if (data.tick_ms != null) setTickMs(data.tick_ms);
    setLiveTick(data.current_tick ?? null);
    setCapturableStartTick(data.capturable_start_tick ?? null);
    setCapturableEndTick(data.capturable_end_tick ?? null);
    setActiveCapture(data.active_capture ?? null);
  };

  const refreshReviews = async () => {
    const res = await fetch("/api/v2/rr/reviews");
    const data: V2ReviewListResponse = await res.json();
    setPendingReviews(data.pending ?? []);
    setSavedReviews(data.saved ?? []);
  };

  const flagViewedTick = async () => {
    const frame = currentLiveFrame();
    if (!frame) return;
    setFlagError(null);
    const res = await fetch("/api/v2/rr/flags", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ game_number: gameNumber(), tick: frame.tick }),
    });
    const data = await res.json();
    if (!res.ok) {
      setFlagError(data.error ?? "Failed to flag tick");
      return;
    }
    await Promise.all([refreshStatus(), refreshReviews()]);
  };

  const openReview = async (id: string) => {
    setReviewLoading(true);
    try {
      const res = await fetch(`/api/v2/rr/reviews/${id}`);
      if (!res.ok) {
        setFlagError("Failed to load review bundle");
        return;
      }
      const data: V2ReviewBundle = await res.json();
      setReviewBundle(data);
      setReviewFrameIdx(0);
      setFollowing(false);
      setPlaying(false);
    } finally {
      setReviewLoading(false);
    }
  };

  const deleteReview = async (id: string) => {
    await fetch(`/api/v2/rr/reviews/${id}`, { method: "DELETE" });
    if (reviewBundle()?.id === id) {
      setReviewBundle(null);
      setReviewFrameIdx(0);
    }
    await refreshReviews();
  };

  const leaveReview = () => {
    setReviewBundle(null);
    setReviewFrameIdx(0);
  };

  const startCapture = async () => {
    const frame = currentLiveFrame();
    if (!frame) return;
    setFlagError(null);
    const res = await fetch("/api/v2/rr/capture/start", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ game_number: gameNumber(), tick: frame.tick }),
    });
    const data = await res.json();
    if (!res.ok) {
      setFlagError(data.error ?? "Failed to start capture");
      return;
    }
    await Promise.all([refreshStatus(), refreshReviews()]);
  };

  const stopCapture = async () => {
    const frame = currentLiveFrame();
    if (!frame) return;
    setFlagError(null);
    const res = await fetch("/api/v2/rr/capture/stop", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ game_number: gameNumber(), tick: frame.tick }),
    });
    const data = await res.json();
    if (!res.ok) {
      setFlagError(data.error ?? "Failed to stop capture");
      return;
    }
    await Promise.all([refreshStatus(), refreshReviews()]);
  };

  createEffect(() => {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsUrl = `${protocol}//${window.location.host}/ws/v2/rr`;

    let retryDelay = 500;
    let dead = false;

    function connect() {
      if (dead) return;
      const ws = new WebSocket(wsUrl);
      wsRef = ws;

      ws.onopen = () => {
        retryDelay = 500;
      };

      ws.onmessage = (ev) => {
        const msg = JSON.parse(ev.data);
        switch (msg.type) {
          case "v2_init": {
            batch(() => {
              setFrames([]);
              setViewIdx(0);
              setFollowing(true);
              setGameNumber(msg.game_number ?? 0);
              setPhase({
                kind: "playing",
                game: {
                  width: msg.width,
                  height: msg.height,
                  terrain: msg.terrain ?? [],
                  material_map: msg.material_map ?? [],
                  heights: msg.height_map ?? [],
                  moistures: [],
                  biomes: [],
                  rivers: [],
                  height_map: msg.height_map ?? [],
                  region_ids: msg.region_ids ?? [],
                  num_players: msg.player_count,
                  agent_names: msg.agent_names ?? [],
                  game_number: msg.game_number ?? 0,
                },
              });
            });
            break;
          }
          case "v2_snapshot": {
            const game = gameInfo();
            if (!game) break;
            setFrames((prev) => {
              const base = msg.full_state || prev.length === 0 ? emptyFrame(game, msg.tick) : prev[prev.length - 1];
              const patched = applyHexChanges(base, msg.hex_changes ?? [], game.width);
              const territory = patched.territory;
              const roads = patched.roads;

              // Compute dead units (were in prev frame, not in this one).
              const prevUnits = prev.length > 0 ? prev[prev.length - 1].units : [];
              const newUnitIds = new Set((msg.units ?? []).map((u: V2UnitSnapshot) => u.id));
              const deadUnits: V2UnitSnapshot[] = prevUnits
                .filter((u: V2UnitSnapshot) => !newUnitIds.has(u.id) && !(u as any)._dead)
                .map((u: V2UnitSnapshot) => ({ ...u, _dead: true, _deadTick: msg.tick } as any));

              // Carry forward existing ghosts, age them out after 8 ticks.
              const prevGhosts = prevUnits
                .filter((u: any) => u._dead && msg.tick - u._deadTick < 8)
                .map((u: any) => ({ ...u }));

              const next: V2Frame = {
                tick: msg.tick,
                entities: msg.entities ?? [],
                units: [...(msg.units ?? []), ...deadUnits, ...prevGhosts],
                convoys: msg.convoys ?? [],
                territory,
                roads,
                depots: Array.from({ length: territory.length }, () => false),
                population: [],
                engagements: msg.engagements ?? [],
                hex_ownership: territory,
                road_levels: roads,
                settlements: msg.full_state ? (msg.settlements ?? []) : patched.settlements,
                players: (msg.players ?? []) as V2SpectatorPlayer[],
              };

              const updated = [...prev, next];
              // Compact old frames to avoid memory leaks on long games.
              if (updated.length > MAX_FRAMES) {
                const excess = updated.length - MAX_FRAMES;
                // Keep every COMPACT_KEEP_EVERY-th frame from the old section.
                const oldSection = updated.slice(0, excess);
                const kept = oldSection.filter((_, i) => i % COMPACT_KEEP_EVERY === 0);
                const recentSection = updated.slice(excess);
                const compacted = [...kept, ...recentSection];
                // Adjust viewIdx so the user stays at roughly the same frame.
                const vi = viewIdx();
                if (vi < excess) {
                  setViewIdx(Math.floor(vi / COMPACT_KEEP_EVERY));
                } else {
                  setViewIdx(vi - excess + kept.length);
                }
                return compacted;
              }
              return updated;
            });
            // Tick-gated hover: promote pending hover on each live tick.
            if (!reviewBundle()) {
              setResolvedHoverHex(pendingHoverHex());
            }
            break;
          }
          case "v2_game_end": {
            const p = phase();
            const game = (p.kind === "playing" || p.kind === "game_over")
              ? p.game
              : {
                  width: 0,
                  height: 0,
                  terrain: [],
                  material_map: [],
                  heights: [],
                  moistures: [],
                  biomes: [],
                  rivers: [],
                  height_map: [],
                  region_ids: [],
                  num_players: 0,
                  agent_names: [],
                  game_number: 0,
                };
            setPhase({
              kind: "game_over",
              game,
              winner: msg.winner,
              tick: msg.tick,
              timedOut: !!msg.timed_out,
            });
            setFollowing(false);
            break;
          }
          case "v2_config": {
            if (msg.tick_ms != null) setTickMs(msg.tick_ms);
            break;
          }
          case "v2_rr_status": {
            batch(() => {
              setServerPaused(!!msg.paused);
              if (msg.tick_ms != null) setTickMs(msg.tick_ms);
              setLiveTick(msg.current_tick ?? null);
              setCapturableStartTick(msg.capturable_start_tick ?? null);
              setCapturableEndTick(msg.capturable_end_tick ?? null);
              setActiveCapture(msg.active_capture ?? null);
            });
            break;
          }
        }
      };

      ws.onclose = () => {
        wsRef = null;
        setPhase({ kind: "connecting" });
        if (!dead) {
          setTimeout(connect, retryDelay);
          retryDelay = Math.min(retryDelay * 2, 2000);
        }
      };

      ws.onerror = () => {};
    }

    connect();
    void refreshReviews();
    const poll = setInterval(() => {
      void refreshReviews();
    }, 2000);
    onCleanup(() => {
      dead = true;
      clearInterval(poll);
      wsRef?.close();
      wsRef = null;
    });
  });

  // Playback: advance one frame per tick interval when playing.
  // "Following" means jump straight to live; "playing" means advance at server speed.
  createEffect(() => {
    if (!playing() && !following()) return;
    const interval = following() ? 33 : Math.max(tickMs(), 16);
    const id = setInterval(() => {
      const max = reviewBundle() ? reviewBundle()!.replay.frames.length - 1 : frames().length - 1;
      if (max < 0) return;
      if (reviewBundle()) {
        if (playing()) {
          setReviewFrameIdx((t) => {
            if (t >= max) {
              setPlaying(false);
              return t;
            }
            return t + 1;
          });
        }
      } else if (following()) {
        // Live mode: catch up quickly.
        if (viewIdx() < max) {
          const behind = max - viewIdx();
          const step = behind > 20 ? Math.ceil(behind / 10) : 1;
          setViewIdx((t) => Math.min(t + step, max));
        }
      } else if (playing()) {
        // Playback mode: advance one frame at server speed.
        if (viewIdx() < max) {
          setViewIdx((t) => t + 1);
        } else {
          // Reached the end of buffered frames, stop playing.
          setPlaying(false);
        }
      }
    }, interval);
    onCleanup(() => clearInterval(id));
  });

  const currentLiveFrame = () => frames()[viewIdx()];
  const currentReviewFrame = () => reviewBundle()?.replay.frames[reviewFrameIdx()];
  const currentFrame = () => reviewBundle() ? currentReviewFrame() : currentLiveFrame();
  const maxIdx = () => {
    const review = reviewBundle();
    if (review) return Math.max(0, review.replay.frames.length - 1);
    return Math.max(0, frames().length - 1);
  };

  const gameInfo = (): V2GameInfo | null => {
    const p = phase();
    if (p.kind === "playing" || p.kind === "game_over") return p.game;
    return null;
  };

  const staticData = createMemo((): BoardStaticData | null => {
    const review = reviewBundle();
    if (review) return normalizeReplayStatic(review.replay);
    const g = gameInfo();
    return g ? normalizeGameInfoStatic(g) : null;
  });

  const currentFrameData = createMemo((): BoardFrameData | null => {
    const review = reviewBundle();
    if (review) {
      const frame = currentReviewFrame();
      return frame ? normalizeReplayFrame(frame) : null;
    }
    const f = currentLiveFrame();
    return f ? normalizeWsFrame(f) : null;
  });

  const playerStats = () => {
    const review = reviewBundle();
    if (review) {
      const frame = currentReviewFrame();
      const replay = review.replay;
      if (!frame) return [];
      return Array.from({ length: replay.num_players }, (_, id) => {
        const pops = frame.population.filter((p) => p.owner === id);
        const totalPop = pops.reduce((sum, p) => sum + p.count, 0);
        const territory = frame.cells.filter((c) => c.stockpile_owner === id).length;
        const convoyCount = frame.convoys.filter((c) => c.owner === id).length;
        return {
          id,
          alive: frame.alive[id] ?? false,
          units: frame.units.filter((u) => u.owner === id).length,
          convoys: convoyCount,
          population: totalPop,
          territory,
          foodLevel: frame.player_food[id] > 20 ? 3 : frame.player_food[id] > 5 ? 2 : frame.player_food[id] > 0 ? 1 : 0,
          materialLevel:
            frame.player_material[id] > 20 ? 3 : frame.player_material[id] > 5 ? 2 : frame.player_material[id] > 0 ? 1 : 0,
        };
      });
    }

    const frame = currentLiveFrame();
    const game = gameInfo();
    if (!frame || !game) return [];
    return Array.from({ length: game.num_players }, (_, id) => {
      const player = frame.players?.find((entry) => entry.id === id);
      return {
        id,
        alive: player?.alive ?? false,
        units: frame.units.filter((u) => u.owner === id).length,
        convoys: frame.convoys.filter((c) => c.owner === id).length,
        population: player?.population ?? 0,
        territory: player?.territory ?? 0,
        foodLevel: player?.food_level ?? 0,
        materialLevel: player?.material_level ?? 0,
      };
    });
  };

  const viewedTickCapturable = createMemo(() => {
    const frame = currentLiveFrame();
    if (reviewBundle() || !frame) return false;
    const start = capturableStartTick();
    const end = capturableEndTick();
    if (start == null || end == null) return false;
    return frame.tick >= start && frame.tick <= end;
  });

  const scrubberOverlayData = createMemo(() => {
    if (reviewBundle()) return null;
    const liveFrames = frames();
    if (liveFrames.length === 0) return null;
    const minTick = liveFrames[0].tick;
    const maxTick = liveFrames[liveFrames.length - 1].tick;
    if (maxTick <= minTick) return null;
    const position = (tick: number) => ((Math.min(maxTick, Math.max(minTick, tick)) - minTick) / (maxTick - minTick)) * 100;
    const toBand = (startTick: number, endTick: number, kind: string, opacity: number) => ({
      left: position(startTick),
      width: Math.max(0.6, position(endTick) - position(startTick)),
      kind,
      opacity,
    });
    const savedBands = savedReviews().map((review) => toBand(review.range_start, review.range_end, review.kind, 0.42));
    const active = activeCapture();
    const pendingBands = pendingReviews()
      .filter((review) => review.id !== active?.id)
      .map((review) => toBand(review.range_start, review.range_end, `pending-${review.kind}`, 0.28));
    const markers = [...savedReviews(), ...pendingReviews()]
      .filter((review) => review.kind === "point")
      .flatMap((review) => review.flagged_ticks.map((tick) => ({ left: position(tick), saved: review.saved })));
    return {
      capturable: capturableStartTick() != null && capturableEndTick() != null
        ? toBand(capturableStartTick()!, capturableEndTick()!, "capturable", 0.2)
        : null,
      savedBands,
      pendingBands,
      activeBand: active ? toBand(active.range_start, active.range_end, "active", 0.55) : null,
      markers,
    };
  });

  const currentReviewAnomalyKeys = createMemo(() => {
    const review = reviewBundle();
    const frame = currentReviewFrame();
    if (!review || !frame) return new Set<string>();
    return new Set(
      review.anomalies
        .filter((anomaly) => anomaly.tick === frame.tick)
        .map((anomaly) => `${anomaly.q},${anomaly.r}`),
    );
  });

  const hoverInspector = createMemo(() => {
    const hover = resolvedHoverHex();
    const frame = currentFrameData();
    const board = staticData();
    if (!hover || !frame || !board) return null;
    const settlement = frame.settlements.find((entry) => entry.q === hover.q && entry.r === hover.r) ?? null;
    const units = currentFrame()!.units.filter((unit) => unit.q === hover.q && unit.r === hover.r);
    const anomaly = reviewBundle() ? currentReviewAnomalyKeys().has(`${hover.q},${hover.r}`) : false;
    return {
      offset: { col: hover.col, row: hover.row },
      axial: { q: hover.q, r: hover.r },
      biome: board.biomes[hover.index] ?? "grassland",
      terrain: board.terrain[hover.index] ?? 0,
      material: board.materialMap[hover.index] ?? 0,
      height: board.heights[hover.index] ?? 0,
      moisture: board.moistures[hover.index] ?? 0,
      river: board.rivers[hover.index] ?? false,
      territoryOwner: frame.territory[hover.index] ?? null,
      stockpileOwner: frame.stockpileOwners[hover.index] ?? null,
      roadLevel: frame.roads[hover.index] ?? 0,
      depot: frame.depots[hover.index] ?? false,
      settlement,
      units,
      anomaly,
    };
  });

  const levelLabel = (level: number) => ["starving", "low", "ok", "surplus"][level] ?? "unknown";

  const toggleLayer = (layer: RenderLayer) => {
    const next = new Set(layers());
    if (next.has(layer)) next.delete(layer);
    else next.add(layer);
    setLayers(next);
  };

  return (
    <div class={styles.app}>
      <div class={styles.header}>
        <span class={styles.title}>Simulate Everything</span>
        <Nav />
        <span style={{ "font-size": "12px", color: "#8888a0" }}>
          <Show when={gameInfo()}>
            {(g) => <>Game #{gameNumber()} &middot; {g().width}x{g().height} hex &middot; {g().num_players} players</>}
          </Show>
          <Show when={phase().kind === "game_over"}>
            {" "}&middot; {((phase() as { timedOut: boolean }).timedOut ? "Timeout" : "Winner")}: {gameInfo()?.agent_names[(phase() as { winner: number | null }).winner ?? -1] ?? "draw"}
          </Show>
        </span>
      </div>

      <Show when={phase().kind === "connecting"}>
        <div style={{ display: "flex", "align-items": "center", "justify-content": "center", flex: 1, color: "#8888a0" }}>
          Connecting to V2 server...
        </div>
      </Show>

      <Show when={(phase().kind === "playing" || phase().kind === "game_over") && currentFrame() && gameInfo() && staticData() && currentFrameData()}>
        <div class={styles.controls}>
          <span class={styles.turnLabel}>Tick {currentFrame()!.tick}</span>
          <button
            class={styles.btn}
            onClick={() => {
              setPlaying(false);
              if (reviewBundle()) setReviewFrameIdx(0);
              else {
                setFollowing(false);
                setViewIdx(0);
              }
            }}
          >
            &#x23EE;
          </button>
          <button
            class={styles.btn}
            onClick={() => {
              setPlaying(false);
              if (reviewBundle()) setReviewFrameIdx((t) => Math.max(t - 1, 0));
              else {
                setFollowing(false);
                setViewIdx((t) => Math.max(t - 1, 0));
              }
            }}
          >
            &#x23F4;
          </button>
          <button class={styles.btn} onClick={() => {
            if (reviewBundle()) {
              setPlaying((p) => !p);
            } else if (following()) {
              // Currently live — pause playback.
              setFollowing(false);
              setPlaying(false);
            } else if (playing()) {
              // Currently playing at speed — pause.
              setPlaying(false);
            } else {
              // Paused — resume playing at server speed.
              setPlaying(true);
            }
          }}>
            {(following() || playing()) ? "\u23F8" : "\u25B6"}
          </button>
          <button
            class={styles.btn}
            onClick={() => {
              setPlaying(false);
              if (reviewBundle()) setReviewFrameIdx((t) => Math.min(t + 1, maxIdx()));
              else {
                setFollowing(false);
                setViewIdx((t) => Math.min(t + 1, maxIdx()));
              }
            }}
          >
            &#x23F5;
          </button>
          <button
            class={styles.btn}
            onClick={() => {
              setPlaying(false);
              if (reviewBundle()) setReviewFrameIdx(maxIdx());
              else {
                setFollowing(true);
                setViewIdx(maxIdx());
              }
            }}
          >
            &#x23ED;
          </button>
          <div class={styles.scrubberWrap}>
            <Show when={scrubberOverlayData()}>
              {(overlay) => (
                <div class={styles.scrubberOverlay}>
                  <Show when={overlay().capturable}>
                    {(band) => (
                      <div
                        class={styles.scrubberBand}
                        style={{
                          left: `${band().left}%`,
                          width: `${band().width}%`,
                          background: "rgba(255,255,255,0.12)",
                        }}
                      />
                    )}
                  </Show>
                  <For each={overlay().savedBands}>
                    {(band) => (
                      <div
                        class={styles.scrubberBand}
                        style={{
                          left: `${band.left}%`,
                          width: `${band.width}%`,
                          background: band.kind === "segment" ? "rgba(74,158,255,0.32)" : "rgba(255,180,90,0.3)",
                          opacity: `${band.opacity}`,
                        }}
                      />
                    )}
                  </For>
                  <For each={overlay().pendingBands}>
                    {(band) => (
                      <div
                        class={styles.scrubberBand}
                        style={{
                          left: `${band.left}%`,
                          width: `${band.width}%`,
                          background: band.kind.includes("segment") ? "rgba(74,158,255,0.2)" : "rgba(255,180,90,0.2)",
                          opacity: `${band.opacity}`,
                          border: "1px dashed rgba(255,255,255,0.3)",
                        }}
                      />
                    )}
                  </For>
                  <Show when={overlay().activeBand}>
                    {(band) => (
                      <div
                        class={styles.scrubberBand}
                        style={{
                          left: `${band().left}%`,
                          width: `${band().width}%`,
                          background: "rgba(80,255,180,0.45)",
                          "box-shadow": "0 0 10px rgba(80,255,180,0.25)",
                        }}
                      />
                    )}
                  </Show>
                  <For each={overlay().markers}>
                    {(marker) => (
                      <div
                        class={styles.scrubberMarker}
                        style={{
                          left: `${marker.left}%`,
                          background: marker.saved ? "rgba(255,220,120,0.9)" : "rgba(255,255,255,0.85)",
                        }}
                      />
                    )}
                  </For>
                </div>
              )}
            </Show>
            <input
              type="range"
              class={styles.slider}
              min={0}
              max={maxIdx()}
              value={reviewBundle() ? reviewFrameIdx() : viewIdx()}
              onInput={(e) => {
                const next = parseInt(e.currentTarget.value);
                setPlaying(false);
                if (reviewBundle()) setReviewFrameIdx(next);
                else {
                  setFollowing(false);
                  setViewIdx(next);
                }
              }}
            />
          </div>
        </div>

        <div class={styles.speedControls}>
          <Show when={!reviewBundle()}>
            <button
              class={styles.btnPrimary}
              style={{ padding: "2px 8px", "font-size": "10px" }}
              disabled={!viewedTickCapturable()}
              onClick={() => void flagViewedTick()}
              title={viewedTickCapturable() ? "Flag the currently viewed tick" : "Viewed tick is outside the server capture window"}
            >
              Flag Tick
            </button>
            <button
              class={styles.btnPrimary}
              style={{ padding: "2px 8px", "font-size": "10px", background: "rgba(80,255,180,0.85)" }}
              disabled={!viewedTickCapturable() || activeCapture() !== null}
              onClick={() => void startCapture()}
              title={activeCapture() ? "A segment capture is already active" : "Start segment capture at the currently viewed tick"}
            >
              Start Capture
            </button>
            <button
              class={styles.btnPrimary}
              style={{ padding: "2px 8px", "font-size": "10px", background: "rgba(255,150,80,0.85)" }}
              disabled={!viewedTickCapturable() || activeCapture() == null}
              onClick={() => void stopCapture()}
              title={activeCapture() ? "Stop the active segment capture at the currently viewed tick" : "No active capture"}
            >
              Stop Capture
            </button>
          </Show>
          <Show when={reviewBundle()}>
            <button
              class={styles.btnPrimary}
              style={{ padding: "2px 8px", "font-size": "10px" }}
              onClick={leaveReview}
            >
              Back To Live
            </button>
          </Show>
          <span>Server speed:</span>
          <For each={SPEED_PRESETS}>
            {(preset) => (
              <button
                class={styles.btn}
                style={{
                  "font-weight": tickMs() === preset.ms ? "bold" : "normal",
                  "font-size": "10px",
                  padding: "2px 6px",
                }}
                onClick={() => sendSpeed(preset.ms)}
              >
                {preset.label}
              </button>
            )}
          </For>
          <Show when={!reviewBundle()}>
            <button
              class={styles.btn}
              style={{ "font-size": "10px", padding: "2px 6px" }}
              onClick={restartGame}
            >
              Restart
            </button>
            <button
              class={styles.btn}
              style={{
                "font-size": "10px",
                padding: "2px 6px",
                "font-weight": serverPaused() ? "bold" : "normal",
              }}
              onClick={toggleServerPause}
            >
              {serverPaused() ? "Resume Server" : "Pause Server"}
            </button>
          </Show>
          <span style={{ "margin-left": "auto" }} />
          <button
            class={styles.btn}
            style={{ "font-size": "10px", padding: "2px 6px", "font-weight": showNumbers() ? "bold" : "normal" }}
            onClick={() => setShowStrength((s) => !s)}
          >
            {showNumbers() ? "#" : "#\u0338"}
          </button>
          <For each={ALL_LAYERS}>
            {(layer) => (
              <button
                class={styles.btn}
                style={{ "font-size": "10px", padding: "2px 6px", "font-weight": layers().has(layer) ? "bold" : "normal" }}
                onClick={() => toggleLayer(layer)}
              >
                {layer[0].toUpperCase()}
              </button>
            )}
          </For>
        </div>
        <div class={styles.configBar} style={{ "font-size": "10px", padding: "4px 12px" }}>
          <Show
            when={!reviewBundle()}
            fallback={
              <>
                <span>Review: {reviewBundle()!.id}</span>
                <span>Range: {reviewBundle()!.range_start}..{reviewBundle()!.range_end}</span>
                <span>Flags: {reviewBundle()!.flagged_ticks.join(", ")}</span>
                <span>{reviewBundle()!.complete ? "complete" : "partial"}</span>
              </>
            }
          >
            <>
              <span>Live tick: {liveTick() ?? "?"}</span>
              <span>Capturable: {capturableStartTick() ?? "?"}..{capturableEndTick() ?? "?"}</span>
              <span>Viewed: {currentLiveFrame()?.tick ?? "?"}</span>
              <span>{viewedTickCapturable() ? "capturable" : "not capturable"}</span>
              <Show when={activeCapture()}>
                {(capture) => <span>Active capture: {capture().start_tick}..{capture().range_end}</span>}
              </Show>
              <Show when={flagError()}>
                {(err) => <span style={{ color: "#ff8080", "margin-left": "auto" }}>{err()}</span>}
              </Show>
            </>
          </Show>
        </div>

        <div class={styles.main}>
          <div class={styles.boardContainer}>
            <HexCanvas
              staticData={staticData()!}
              frameData={currentFrameData()!}
              numPlayers={gameInfo()!.num_players}
              showNumbers={showNumbers()}
              layers={layers()}
            />
          </div>

          <div class={styles.sidebar}>
            <div class={styles.statsPanel}>
              <div class={styles.playerPanel}>
                <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
                  <span>Review Bundles</span>
                  <span class={styles.statValue}>{pendingReviews().length + savedReviews().length}</span>
                </div>
                <Show when={reviewLoading()}>
                  <div class={styles.statRow}>Loading review bundle...</div>
                </Show>
                <For each={pendingReviews()}>
                  {(review) => (
                    <div class={styles.statRow}>
                      Pending {review.kind} {review.range_start}-{review.range_end}
                    </div>
                  )}
                </For>
                <For each={savedReviews().slice(0, 8)}>
                  {(review) => (
                    <div class={styles.statRow} style={{ display: "flex", gap: "6px", "align-items": "center" }}>
                      <span style={{ flex: 1, overflow: "hidden", "text-overflow": "ellipsis" }}>
                        {review.kind} {review.range_start}-{review.range_end}
                      </span>
                      <button class={styles.btn} style={{ padding: "1px 4px", "font-size": "10px" }} onClick={() => void openReview(review.id)}>
                        Open
                      </button>
                      <button class={styles.btn} style={{ padding: "1px 4px", "font-size": "10px" }} onClick={() => void deleteReview(review.id)}>
                        Del
                      </button>
                    </div>
                  )}
                </For>
              </div>
              <For each={playerStats()}>
                {(stat) => (
                  <div class={`${styles.playerPanel} ${!stat.alive ? styles.eliminated : ""}`}>
                    <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
                      <div class={styles.playerDot} style={{ background: PLAYER_COLORS[stat.id % PLAYER_COLORS.length] }} />
                      <span>{reviewBundle()?.agent_names[stat.id] ?? gameInfo()!.agent_names[stat.id] ?? `Player ${stat.id + 1}`}</span>
                    </div>
                    <div class={styles.statRow}>
                      <span class={styles.statValue}>{stat.units} units &middot; {stat.convoys} convoys</span>
                    </div>
                    <div class={styles.statRow}>
                      <span class={styles.statValue}>{stat.population} pop &middot; {stat.territory} hexes</span>
                    </div>
                    <div class={styles.statRow}>
                      <span class={styles.statValue}>Food {levelLabel(stat.foodLevel)} &middot; Material {levelLabel(stat.materialLevel)}</span>
                    </div>
                  </div>
                )}
              </For>
              <div class={styles.playerPanel}>
                <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
                  <span>Hover Inspector</span>
                  <span class={styles.statValue}>{hoverInspector() ? `${hoverInspector()!.offset.col},${hoverInspector()!.offset.row}` : "none"}</span>
                </div>
                <Show
                  when={hoverInspector()}
                  fallback={<div class={styles.statRow}>Hover a hex to inspect it.</div>}
                >
                  {(info) => (
                    <>
                      <div class={styles.statRow}>Offset ({info().offset.col},{info().offset.row}) | Axial ({info().axial.q},{info().axial.r})</div>
                      <div class={styles.statRow}>{info().biome} | h {info().height.toFixed(2)} | m {info().moisture.toFixed(2)} | river {info().river ? "yes" : "no"}</div>
                      <div class={styles.statRow}>Terrain {info().terrain.toFixed(2)} | Material {info().material.toFixed(2)} | Road {info().roadLevel} | Depot {info().depot ? "yes" : "no"}</div>
                      <div class={styles.statRow}>Territory {info().territoryOwner ?? "none"} | Stockpile {info().stockpileOwner ?? "none"} | Settlement {info().settlement ? `${info().settlement?.settlement_type ?? "Village"} p${info().settlement?.owner ?? "?"}` : "none"}</div>
                      <div class={styles.statRow}>Units: {info().units.length === 0 ? "none" : info().units.map((unit) => `#${unit.id}/p${unit.owner}/${unit.strength.toFixed(1)}${unit.engaged ? " engaged" : ""}`).join(" | ")}</div>
                      <Show when={reviewBundle()}>
                        <div class={styles.statRow}>Review anomaly: {info().anomaly ? "yes" : "no"}</div>
                      </Show>
                    </>
                  )}
                </Show>
              </div>
            </div>

            <div class={styles.legend}>
              <div class={styles.legendTitle}>Legend</div>
              <div class={styles.legendGrid}>
                <svg width="14" height="14"><polygon points="7,1 13,7 7,13 1,7" fill="rgba(74,158,255,0.85)" stroke="#fff" stroke-width="0.5" /></svg>
                <span>Convoy (F/M/S)</span>
                <svg width="14" height="14"><circle cx="7" cy="7" r="2.5" fill="rgba(74,158,255,0.8)" /></svg>
                <span>Farm</span>
                <svg width="14" height="14"><path d="M3,5 L3,12 L11,12 L11,5 L7,2 Z" fill="rgba(74,158,255,0.9)" stroke="#fff" stroke-width="0.5" /></svg>
                <span>Village</span>
                <svg width="14" height="14"><path d="M2,12 L2,5 L4,5 L4,3 L6,3 L6,5 L8,5 L8,3 L10,3 L10,5 L12,5 L12,12 Z" fill="rgba(74,158,255,0.95)" stroke="#fff" stroke-width="0.5" /></svg>
                <span>City</span>
                <svg width="14" height="14"><line x1="2" y1="7" x2="7" y2="7" stroke="rgba(200,200,180,0.6)" stroke-width="2" stroke-linecap="round" /><line x1="7" y1="7" x2="12" y2="4" stroke="rgba(220,200,140,0.7)" stroke-width="2" stroke-linecap="round" /></svg>
                <span>Road network</span>
                <svg width="14" height="14"><path d="M3,10 L5,6 L7,8 L9,6 L11,10 Z" fill="#fff" stroke="#000" stroke-width="0.5" /></svg>
                <span>General (crown)</span>
              </div>
              <div class={styles.legendTitle} style={{ "margin-top": "6px" }}>Convoy routes</div>
              <div class={styles.legendGrid}>
                <svg width="14" height="14"><line x1="1" y1="7" x2="13" y2="7" stroke="rgba(74,158,255,0.5)" stroke-width="1.5" stroke-dasharray="3,3" /></svg>
                <span>Food route</span>
                <svg width="14" height="14"><line x1="1" y1="7" x2="13" y2="7" stroke="rgba(74,158,255,0.5)" stroke-width="1.5" stroke-dasharray="6,4" /></svg>
                <span>Material route</span>
                <svg width="14" height="14"><line x1="1" y1="7" x2="13" y2="7" stroke="rgba(74,158,255,0.5)" stroke-width="1.5" stroke-dasharray="2,6" /></svg>
                <span>Settler route</span>
              </div>
              <div class={styles.legendTitle} style={{ "margin-top": "6px" }}>Unit status</div>
              <div class={styles.legendGrid}>
                <span style={{ color: "#ff6644", "font-weight": "bold", "text-align": "center" }}>⚔</span>
                <span>In combat</span>
                <span style={{ color: "#88cc88", "font-weight": "bold", "text-align": "center" }}>→</span>
                <span>Moving</span>
                <span style={{ color: "#aaa", "font-weight": "bold", "text-align": "center" }}>◷</span>
                <span>Cooldown</span>
              </div>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default V2App;
