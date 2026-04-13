import { Component, createEffect, createMemo, createSignal, For, onCleanup, Show, batch } from "solid-js";
import HexBoard from "./HexBoard";
import type { RenderLayer } from "./HexBoard";
import Nav from "./Nav";
import type {
  BoardFrameData,
  BoardStaticData,
  V2Frame,
  V2GameInfo,
  V2HexDelta,
  V2Settlement,
  V2SpectatorPlayer,
} from "./v2types";
import { normalizeGameInfoStatic, normalizeWsFrame } from "./v2types";
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
  const settlementMap = new Map((frame.settlements ?? []).map((s) => [`${s.q},${s.r}`, s] as const));

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
  const [showNumbers, setShowStrength] = createSignal(false);
  const [gameNumber, setGameNumber] = createSignal(0);
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
    fetch(endpoint, { method: "POST" }).then(() => setServerPaused((p) => !p));
  };

  const restartGame = () => {
    fetch("/api/v2/rr/reset", { method: "POST" });
    setServerPaused(false);
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
    onCleanup(() => {
      dead = true;
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
      const max = frames().length - 1;
      if (max < 0) return;
      if (following()) {
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

  const currentFrame = () => frames()[viewIdx()];
  const maxIdx = () => Math.max(0, frames().length - 1);

  const gameInfo = (): V2GameInfo | null => {
    const p = phase();
    if (p.kind === "playing" || p.kind === "game_over") return p.game;
    return null;
  };

  const staticData = createMemo((): BoardStaticData | null => {
    const g = gameInfo();
    return g ? normalizeGameInfoStatic(g) : null;
  });

  const currentFrameData = createMemo((): BoardFrameData | null => {
    const f = currentFrame();
    return f ? normalizeWsFrame(f) : null;
  });

  const playerStats = () => {
    const frame = currentFrame();
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
        <span class={styles.title}>Generals V2</span>
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
          <button class={styles.btn} onClick={() => { setFollowing(false); setPlaying(false); setViewIdx(0); }}>&#x23EE;</button>
          <button class={styles.btn} onClick={() => { setFollowing(false); setPlaying(false); setViewIdx((t) => Math.max(t - 1, 0)); }}>&#x23F4;</button>
          <button class={styles.btn} onClick={() => {
            if (following()) {
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
          <button class={styles.btn} onClick={() => { setFollowing(false); setPlaying(false); setViewIdx((t) => Math.min(t + 1, maxIdx())); }}>&#x23F5;</button>
          <button class={styles.btn} onClick={() => { setFollowing(true); setPlaying(false); setViewIdx(maxIdx()); }}>&#x23ED;</button>
          <input
            type="range"
            class={styles.slider}
            min={0}
            max={maxIdx()}
            value={viewIdx()}
            onInput={(e) => { setFollowing(false); setViewIdx(parseInt(e.currentTarget.value)); }}
          />
        </div>

        <div class={styles.speedControls}>
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

        <div class={styles.main}>
          <div class={styles.boardContainer}>
            <HexBoard
              staticData={staticData()!}
              frameData={currentFrameData()!}
              numPlayers={gameInfo()!.num_players}
              showNumbers={showNumbers()}
              layers={layers()}
            />
          </div>

          <div class={styles.sidebar}>
            <div class={styles.statsPanel}>
              <For each={playerStats()}>
                {(stat) => (
                  <div class={`${styles.playerPanel} ${!stat.alive ? styles.eliminated : ""}`}>
                    <div style={{ display: "flex", "align-items": "center", gap: "8px" }}>
                      <div class={styles.playerDot} style={{ background: PLAYER_COLORS[stat.id % PLAYER_COLORS.length] }} />
                      <span>{gameInfo()!.agent_names[stat.id] ?? `Player ${stat.id + 1}`}</span>
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
