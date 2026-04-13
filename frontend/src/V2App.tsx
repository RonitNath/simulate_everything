import { Component, createSignal, createEffect, onCleanup, Show, For, batch } from "solid-js";
import HexBoard from "./HexBoard";
import Nav from "./Nav";
import type { V2Frame, V2GameInfo, V2HexDelta, V2SpectatorPlayer, V2Settlement } from "./v2types";
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

function emptyFrame(game: V2GameInfo, tick: number): V2Frame {
  return {
    tick,
    units: [],
    engagements: [],
    convoys: [],
    hex_ownership: Array.from({ length: game.width * game.height }, () => null),
    road_levels: Array.from({ length: game.width * game.height }, () => 0),
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
): { ownership: (number | null)[]; roads: number[]; settlements: V2Settlement[] } {
  const ownership = frame.hex_ownership.slice();
  const roads = frame.road_levels.slice();
  const settlementMap = new Map(frame.settlements.map((s) => [`${s.q},${s.r}`, s] as const));
  for (const hex of hexChanges) {
    ownership[hex.index] = hex.owner;
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
    ownership,
    roads,
    settlements: Array.from(settlementMap.values()),
  };
}

const V2App: Component = () => {
  const [phase, setPhase] = createSignal<Phase>({ kind: "connecting" });
  const [frames, setFrames] = createSignal<V2Frame[]>([]);
  const [viewIdx, setViewIdx] = createSignal(0);
  const [following, setFollowing] = createSignal(true);
  const [tickMs, setTickMs] = createSignal(250);
  const [showNumbers, setShowStrength] = createSignal(false);

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
          case "v2_init": {
            batch(() => {
              setFrames([]);
              setViewIdx(0);
              setFollowing(true);
              setPhase({
                kind: "playing",
                game: {
                  width: msg.width,
                  height: msg.height,
                  terrain: msg.terrain,
                  material_map: msg.material_map ?? [],
                  height_map: msg.height_map ?? [],
                  region_ids: msg.region_ids ?? [],
                  num_players: msg.player_count,
                  agent_names: msg.agent_names,
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
              const { ownership, roads, settlements } = applyHexChanges(base, msg.hex_changes ?? [], game.width);
              const next: V2Frame = {
                tick: msg.tick,
                units: msg.units ?? [],
                engagements: msg.engagements ?? [],
                convoys: msg.convoys ?? [],
                hex_ownership: ownership,
                road_levels: roads,
                settlements: msg.full_state ? (msg.settlements ?? []) : settlements,
                players: (msg.players ?? []) as V2SpectatorPlayer[],
              };
              return [...prev, next];
            });
            break;
          }
          case "v2_game_end": {
            const p = phase();
            const game = (p.kind === "playing" || p.kind === "game_over")
              ? (p as any).game as V2GameInfo
              : { width: 0, height: 0, terrain: [], material_map: [], height_map: [], region_ids: [], num_players: 0, agent_names: [] };
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

  const playerStats = () => currentFrame()?.players ?? [];
  const levelLabel = (level: number) => ["starving", "low", "ok", "surplus"][level] ?? "unknown";

  return (
    <div class={styles.app}>
      <div class={styles.header}>
        <span class={styles.title}>Generals V2</span>
        <Nav />
        <span style={{ "font-size": "12px", color: "#8888a0" }}>
          <Show when={gameInfo()}>
            {(g) => <>{g().width}x{g().height} hex &middot; {g().num_players} players</>}
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

      <Show when={(phase().kind === "playing" || phase().kind === "game_over") && currentFrame() && gameInfo()}>
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
        </div>

        <div class={styles.main}>
          <div class={styles.boardContainer}>
            <HexBoard
              terrain={gameInfo()!.terrain}
              units={currentFrame()!.units}
              hexOwnership={currentFrame()!.hex_ownership}
              roadLevels={currentFrame()!.road_levels}
              settlements={currentFrame()!.settlements}
              width={gameInfo()!.width}
              height={gameInfo()!.height}
              showNumbers={showNumbers()}
            />
          </div>

          <div class={styles.sidebar}>
            <div class={styles.statsPanel}>
              <For each={playerStats()}>
                {(stat) => (
                  <div class={`${styles.playerStat} ${!stat.alive ? styles.eliminated : ""}`}>
                    <div class={styles.playerDot} style={{ background: PLAYER_COLORS[stat.id % PLAYER_COLORS.length] }} />
                    <span>{gameInfo()!.agent_names[stat.id]}</span>
                    <span class={styles.statValue}>
                      pop {stat.population} &middot; land {stat.territory}
                    </span>
                    <span class={styles.statValue}>
                      food {levelLabel(stat.food_level)} / mat {levelLabel(stat.material_level)}
                    </span>
                  </div>
                )}
              </For>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default V2App;
