import { Component, createSignal, createEffect, createMemo, onCleanup, Show, For, batch } from "solid-js";
import HexBoard from "./HexBoard";
import type { RenderLayer } from "./HexBoard";
import Nav from "./Nav";
import type { V2Frame, V2GameInfo, BoardStaticData, BoardFrameData, V2ScoreSnapshot } from "./v2types";
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

const ALL_LAYERS: RenderLayer[] = ["territory", "roads", "depots", "settlements", "convoys", "destinations"];

const V2App: Component = () => {
  const [phase, setPhase] = createSignal<Phase>({ kind: "connecting" });
  const [frames, setFrames] = createSignal<V2Frame[]>([]);
  const [viewIdx, setViewIdx] = createSignal(0);
  const [following, setFollowing] = createSignal(true);
  const [tickMs, setTickMs] = createSignal(250);
  const [showNumbers, setShowStrength] = createSignal(false);
  const [gameNumber, setGameNumber] = createSignal(0);
  const [layers, setLayers] = createSignal<Set<RenderLayer>>(
    new Set(["territory", "roads", "depots", "settlements", "convoys"])
  );

  let wsRef: WebSocket | null = null;

  const sendSpeed = (ms: number) => {
    setTickMs(ms);
    if (wsRef && wsRef.readyState === WebSocket.OPEN) {
      wsRef.send(JSON.stringify({ type: "set_speed", tick_ms: ms }));
    }
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
          case "v2_game_start": {
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
                  heights: msg.heights ?? [],
                  moistures: msg.moistures ?? [],
                  biomes: msg.biomes ?? [],
                  rivers: msg.rivers ?? [],
                  num_players: msg.num_players,
                  agent_names: msg.agent_names,
                  game_number: msg.game_number ?? 0,
                },
              });
            });
            break;
          }
          case "v2_frame": {
            const frame: V2Frame = {
              tick: msg.tick,
              units: msg.units ?? [],
              player_food: msg.player_food ?? [],
              player_material: msg.player_material ?? [],
              alive: msg.alive ?? [],
              territory: msg.territory ?? [],
              roads: msg.roads ?? [],
              depots: msg.depots ?? [],
              population: msg.population ?? [],
              convoys: msg.convoys ?? [],
              scores: msg.scores ?? [],
            };
            setFrames((prev) => [...prev, frame]);
            break;
          }
          case "v2_game_end": {
            const p = phase();
            const game = (p.kind === "playing" || p.kind === "game_over")
              ? (p as any).game as V2GameInfo
              : {
                  width: 0, height: 0, terrain: [], material_map: [],
                  heights: [], moistures: [], biomes: [], rivers: [],
                  num_players: 0, agent_names: [], game_number: 0,
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
    onCleanup(() => { dead = true; wsRef?.close(); wsRef = null; });
  });

  // Smooth playback: advance one frame per ~30fps tick when following
  createEffect(() => {
    if (!following()) return;
    const id = setInterval(() => {
      const max = frames().length - 1;
      if (max >= 0 && viewIdx() < max) {
        const behind = max - viewIdx();
        const step = behind > 20 ? Math.ceil(behind / 10) : 1;
        setViewIdx((t) => Math.min(t + step, max));
      }
    }, 33);
    onCleanup(() => clearInterval(id));
  });

  const currentFrame = () => frames()[viewIdx()];
  const maxIdx = () => Math.max(0, frames().length - 1);

  const gameInfo = (): V2GameInfo | null => {
    const p = phase();
    if (p.kind === "playing" || p.kind === "game_over") {
      return (p as any).game;
    }
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

  // Per-player stats derived from current frame
  const playerStats = () => {
    const f = currentFrame();
    const g = gameInfo();
    if (!f || !g) return [];
    return Array.from({ length: g.num_players }, (_, i) => {
      const pops = f.population.filter(p => p.owner === i);
      const totalPop = pops.reduce((s, p) => s + p.count, 0);
      const farmers = pops.filter(p => p.role === "Farmer").reduce((s, p) => s + p.count, 0);
      const workers = pops.filter(p => p.role === "Worker").reduce((s, p) => s + p.count, 0);
      const soldiers = pops.filter(p => p.role === "Soldier").reduce((s, p) => s + p.count, 0);
      const territoryCount = f.territory.filter(t => t === i).length;
      const hexPops = new Map<string, number>();
      for (const p of pops) {
        const key = `${p.q},${p.r}`;
        hexPops.set(key, (hexPops.get(key) ?? 0) + p.count);
      }
      const settlements = [...hexPops.values()].filter(c => c >= 10).length;
      const convoyCount = f.convoys.filter(c => c.owner === i).length;
      const score: V2ScoreSnapshot | undefined = f.scores.find(s => s.player_id === i);
      return {
        id: i,
        units: f.units.filter(u => u.owner === i).length,
        food: f.player_food[i] ?? 0,
        material: f.player_material[i] ?? 0,
        alive: f.alive[i] ?? false,
        totalPop, farmers, workers, soldiers,
        territoryCount, settlements, convoyCount,
        score,
      };
    });
  };

  const toggleLayer = (l: RenderLayer) => {
    const s = new Set(layers());
    if (s.has(l)) s.delete(l); else s.add(l);
    setLayers(s);
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
            {" "}&middot; {((phase() as any).timedOut ? "Timeout" : "Winner")}: {gameInfo()?.agent_names[(phase() as any).winner] ?? "draw"}
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
          <button class={styles.btn} onClick={() => { setFollowing(false); setViewIdx(0); }}>&#x23EE;</button>
          <button class={styles.btn} onClick={() => { setFollowing(false); setViewIdx((t) => Math.max(t - 1, 0)); }}>&#x23F4;</button>
          <button class={styles.btn} onClick={() => setFollowing((f) => !f)}>
            {following() ? "\u23F8" : "\u25B6"}
          </button>
          <button class={styles.btn} onClick={() => { setFollowing(false); setViewIdx((t) => Math.min(t + 1, maxIdx())); }}>&#x23F5;</button>
          <button class={styles.btn} onClick={() => { setFollowing(true); setViewIdx(maxIdx()); }}>&#x23ED;</button>
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
          <span style={{ "margin-left": "auto" }} />
          <button
            class={styles.btn}
            style={{ "font-size": "10px", padding: "2px 6px", "font-weight": showNumbers() ? "bold" : "normal" }}
            onClick={() => setShowStrength((s) => !s)}
          >
            {showNumbers() ? "#" : "#\u0338"}
          </button>
          <For each={ALL_LAYERS}>
            {(l) => (
              <button
                class={styles.btn}
                style={{ "font-size": "10px", padding: "2px 6px", "font-weight": layers().has(l) ? "bold" : "normal" }}
                onClick={() => toggleLayer(l)}
              >
                {l[0].toUpperCase()}
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
                      <span>{gameInfo()!.agent_names[stat.id]}</span>
                    </div>
                    <Show when={stat.score}>
                      {(sc) => (
                        <div class={styles.scoreBar}>
                          <div style={{ flex: sc().population * 4, background: "#4ac0c0" }} />
                          <div style={{ flex: sc().territory * 3, background: "#4a80ff" }} />
                          <div style={{ flex: sc().military * 2, background: "#ff4a6a" }} />
                          <div style={{ flex: sc().stockpiles * 1, background: "#ffa04a" }} />
                        </div>
                      )}
                    </Show>
                    <div class={styles.statRow}>
                      <span class={styles.statValue}>{stat.units} units &middot; {stat.food.toFixed(0)} food / {stat.material.toFixed(0)} mat</span>
                    </div>
                    <div class={styles.statRow}>
                      <span class={styles.statValue}>{stat.totalPop} pop &middot; {stat.farmers}F {stat.workers}W {stat.soldiers}S</span>
                    </div>
                    <div class={styles.statRow}>
                      <span class={styles.statValue}>{stat.territoryCount} hexes &middot; {stat.settlements} settlements &middot; {stat.convoyCount} convoys</span>
                    </div>
                  </div>
                )}
              </For>
            </div>
            {/* Legend */}
            <div class={styles.legend}>
              <div class={styles.legendTitle}>Legend</div>
              <div class={styles.legendGrid}>
                <svg width="14" height="14"><polygon points="7,1 13,7 7,13 1,7" fill="rgba(74,158,255,0.85)" stroke="#fff" stroke-width="0.5" /></svg>
                <span>Convoy (F/M/S)</span>
                <svg width="14" height="14"><polygon points="7,2 3,12 11,12" fill="rgba(74,158,255,0.9)" stroke="#fff" stroke-width="0.5" /></svg>
                <span>Settlement</span>
                <svg width="14" height="14"><rect x="3" y="3" width="8" height="8" fill="#c0a000" stroke="#8a7200" stroke-width="0.5" /></svg>
                <span>Depot</span>
                <svg width="14" height="14"><line x1="3" y1="7" x2="11" y2="7" stroke="rgba(200,200,180,0.6)" stroke-width="2" /><line x1="7" y1="3" x2="7" y2="11" stroke="rgba(200,200,180,0.6)" stroke-width="2" /></svg>
                <span>Road</span>
                <svg width="14" height="14"><text x="7" y="11" text-anchor="middle" font-size="12" fill="#ffd700">★</text></svg>
                <span>General</span>
                <svg width="14" height="14"><line x1="1" y1="7" x2="13" y2="7" stroke="#ff6644" stroke-width="3" stroke-linecap="round" /></svg>
                <span>Combat edge</span>
              </div>
              <div class={styles.legendTitle} style={{ "margin-top": "6px" }}>Score bar</div>
              <div class={styles.legendGrid}>
                <div style={{ width: "14px", height: "10px", background: "#4ac0c0", "border-radius": "2px" }} />
                <span>Population</span>
                <div style={{ width: "14px", height: "10px", background: "#4a80ff", "border-radius": "2px" }} />
                <span>Territory</span>
                <div style={{ width: "14px", height: "10px", background: "#ff4a6a", "border-radius": "2px" }} />
                <span>Military</span>
                <div style={{ width: "14px", height: "10px", background: "#ffa04a", "border-radius": "2px" }} />
                <span>Stockpiles</span>
              </div>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default V2App;
