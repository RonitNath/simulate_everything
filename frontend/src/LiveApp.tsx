import { Component, createSignal, createEffect, onCleanup, Show, For, batch } from "solid-js";
import Board from "./Board";
import Nav from "./Nav";
import type { Frame, PlayerStats } from "./types";
import * as styles from "./styles/app.css";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

interface LobbyPlayer {
  slot: number;
  name: string;
}

interface GameInfo {
  width: number;
  height: number;
  num_players: number;
  agent_names: string[];
}

type Phase =
  | { kind: "connecting" }
  | { kind: "lobby"; players: LobbyPlayer[]; needed: number }
  | { kind: "playing"; game: GameInfo }
  | { kind: "game_over"; game: GameInfo; winner: number | null; turns: number };

const SPEED_PRESETS = [
  { label: "0.5x", ms: 500 },
  { label: "1x", ms: 250 },
  { label: "2x", ms: 125 },
  { label: "4x", ms: 60 },
  { label: "10x", ms: 25 },
  { label: "Max", ms: 10 },
];

const LiveApp: Component = () => {
  const isLive = (window as any).__PAGE__ === "live";
  const [phase, setPhase] = createSignal<Phase>({ kind: "connecting" });
  const [frames, setFrames] = createSignal<Frame[]>([]);
  const [viewTurn, setViewTurn] = createSignal(0);
  const [following, setFollowing] = createSignal(true);
  const [tickMs, setTickMs] = createSignal(250);
  const [showNumbers, setShowNumbers] = createSignal(false);

  let wsRef: WebSocket | null = null;

  const sendSpeed = (ms: number) => {
    setTickMs(ms);
    if (wsRef && wsRef.readyState === WebSocket.OPEN) {
      wsRef.send(JSON.stringify({ type: "set_speed", tick_ms: ms }));
    }
  };

  createEffect(() => {
    const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const wsPath = (window as any).__WS_PATH__ || "/ws/spectate";
    const wsUrl = `${protocol}//${window.location.host}${wsPath}`;

    let t0 = performance.now();
    let retryDelay = 500;
    let dead = false;
    let firstFrameLogged = false;

    const log = (label: string, detail?: string) => {
      const dt = (performance.now() - t0).toFixed(0);
      console.log(`[spectator +${dt}ms] ${label}${detail ? ": " + detail : ""}`);
    };

    function connect() {
      if (dead) return;
      t0 = performance.now();
      firstFrameLogged = false;
      log("ws_create");

      const ws = new WebSocket(wsUrl);
      wsRef = ws;

      ws.onopen = () => {
        log("ws_open");
        retryDelay = 500;
        setPhase({ kind: "lobby", players: [], needed: 2 });
      };

      ws.onmessage = (ev) => {
        const msg = JSON.parse(ev.data);
        switch (msg.type) {
          case "hello": {
            const clientVer = (window as any).__BUILD_VER__;
            if (clientVer && msg.build_ver && clientVer !== msg.build_ver) {
              log("version_mismatch", `client=${clientVer} server=${msg.build_ver}, reloading`);
              window.location.reload();
              return;
            }
            log("hello", `build_ver=${msg.build_ver}`);
            break;
          }
          case "lobby":
            log("rx_lobby", `${msg.players.length}/${msg.players_needed} players`);
            setPhase({ kind: "lobby", players: msg.players, needed: msg.players_needed });
            break;
          case "game_start":
            log("rx_game_start", `${msg.width}x${msg.height} ${msg.agent_names.join(" vs ")}`);
            firstFrameLogged = false;
            batch(() => {
              setFrames([]);
              setViewTurn(0);
              setFollowing(true);
              setPhase({
                kind: "playing",
                game: {
                  width: msg.width,
                  height: msg.height,
                  num_players: msg.num_players,
                  agent_names: msg.agent_names,
                },
              });
            });
            break;
          case "frame":
            if (!firstFrameLogged) {
              log("rx_first_frame", `turn=${msg.turn}`);
              firstFrameLogged = true;
            }
            setFrames((prev) => [...prev, msg as Frame]);
            break;
          case "game_end": {
            log("rx_game_end", `winner=${msg.winner} turns=${msg.turns}`);
            const p = phase();
            const game = (p.kind === "playing" || p.kind === "game_over")
              ? (p as any).game as GameInfo
              : { width: 0, height: 0, num_players: 0, agent_names: [] };
            setPhase({ kind: "game_over", game, winner: msg.winner, turns: msg.turns });
            setFollowing(false);
            break;
          }
          case "config":
            if (msg.show_numbers !== undefined && msg.show_numbers !== null) {
              setShowNumbers(msg.show_numbers);
            }
            if (msg.tick_ms !== undefined && msg.tick_ms !== null) {
              setTickMs(msg.tick_ms);
            }
            break;
        }
      };

      ws.onclose = (ev) => {
        log("ws_close", `code=${ev.code}`);
        wsRef = null;
        setPhase({ kind: "connecting" });
        if (!dead) {
          log("reconnect_scheduled", `${retryDelay}ms`);
          setTimeout(connect, retryDelay);
          retryDelay = Math.min(retryDelay * 2, 2000);
        }
      };

      ws.onerror = () => {
        // onclose fires after onerror, reconnect happens there
      };
    }

    connect();
    onCleanup(() => { dead = true; wsRef?.close(); wsRef = null; });
  });

  // Smooth playback: when following, advance one frame per display tick (~30fps).
  // This decouples the display rate from the server tick rate.
  createEffect(() => {
    if (!following()) return;
    const id = setInterval(() => {
      const max = frames().length - 1;
      if (max >= 0 && viewTurn() < max) {
        // Advance by 1, or catch up faster if we're far behind.
        const behind = max - viewTurn();
        const step = behind > 20 ? Math.ceil(behind / 10) : 1;
        setViewTurn((t) => Math.min(t + step, max));
      }
    }, 33); // ~30fps display rate
    onCleanup(() => clearInterval(id));
  });

  const currentPhase = () => phase();
  const currentFrames = () => frames();
  const currentFrame = () => currentFrames()[viewTurn()];
  const maxIdx = () => Math.max(0, currentFrames().length - 1);

  const gameInfo = (): GameInfo | null => {
    const p = currentPhase();
    if (p.kind === "playing" || p.kind === "game_over") {
      return (p as any).game;
    }
    return null;
  };

  return (
    <div class={styles.app}>
      <div class={styles.header}>
        <span class={styles.title}>Generals</span>
        <Nav />
        <span style={{ "font-size": "12px", color: "#8888a0" }}>
          <Show when={gameInfo()}>
            {(g) => <>{g().width}x{g().height} &middot; {g().num_players} players</>}
          </Show>
          <Show when={currentPhase().kind === "game_over"}>
            {" "}&middot; Winner: {gameInfo()?.agent_names[(currentPhase() as any).winner] ?? "draw"}
          </Show>
        </span>
      </div>

      {/* Lobby / connecting states */}
      <Show when={currentPhase().kind === "connecting"}>
        <div style={{ display: "flex", "align-items": "center", "justify-content": "center", flex: 1, color: "#8888a0" }}>
          Connecting to server...
        </div>
      </Show>

      <Show when={currentPhase().kind === "lobby"}>
        <div style={{ display: "flex", "flex-direction": "column", "align-items": "center", "justify-content": "center", flex: 1, gap: "16px" }}>
          <span style={{ "font-size": "18px" }}>Waiting for players...</span>
          <For each={(currentPhase() as any).players as LobbyPlayer[]}>
            {(p) => (
              <div class={styles.playerStat}>
                <div class={styles.playerDot} style={{ background: PLAYER_COLORS[p.slot % PLAYER_COLORS.length] }} />
                <span>P{p.slot}: {p.name}</span>
              </div>
            )}
          </For>
          <span style={{ color: "#8888a0", "font-size": "12px" }}>
            {(currentPhase() as any).players.length} / {(currentPhase() as any).needed} connected
          </span>
        </div>
      </Show>

      {/* Game view */}
      <Show when={(currentPhase().kind === "playing" || currentPhase().kind === "game_over") && currentFrame()}>
        <div class={styles.controls}>
          <span class={styles.turnLabel}>Turn {currentFrame()!.turn}</span>
          <Show when={isLive}>
            <button class={styles.btn} onClick={() => { setFollowing(false); setViewTurn(0); }}>&#x23EE;</button>
            <button class={styles.btn} onClick={() => { setFollowing(false); setViewTurn((t) => Math.max(t - 1, 0)); }}>&#x23F4;</button>
            <button class={styles.btn} onClick={() => setFollowing((f) => !f)}>
              {following() ? "\u23F8" : "\u25B6"}
            </button>
            <button class={styles.btn} onClick={() => { setFollowing(false); setViewTurn((t) => Math.min(t + 1, maxIdx())); }}>&#x23F5;</button>
            <button class={styles.btn} onClick={() => { setFollowing(true); setViewTurn(maxIdx()); }}>&#x23ED;</button>
            <input
              type="range"
              class={styles.slider}
              min={0}
              max={maxIdx()}
              value={viewTurn()}
              onInput={(e) => { setFollowing(false); setViewTurn(parseInt(e.currentTarget.value)); }}
            />
          </Show>
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
            onClick={() => setShowNumbers((s) => !s)}
          >
            {showNumbers() ? "#" : "#\u0338"}
          </button>
        </div>

        <div class={styles.main}>
          <div class={styles.boardContainer}>
            <Board frame={currentFrame()!} width={gameInfo()!.width} height={gameInfo()!.height} showNumbers={showNumbers()} />
          </div>

          <div class={styles.sidebar}>
            <div class={styles.statsPanel}>
              <For each={currentFrame()!.stats}>
                {(stat: PlayerStats) => {
                  const us = () => currentFrame()!.compute_us?.[stat.player] ?? 0;
                  const label = () => {
                    const v = us();
                    if (v >= 1000) return `${(v / 1000).toFixed(1)}ms`;
                    return `${v}μs`;
                  };
                  return (
                    <div class={`${styles.playerStat} ${!stat.alive ? styles.eliminated : ""}`}>
                      <div class={styles.playerDot} style={{ background: PLAYER_COLORS[stat.player % PLAYER_COLORS.length] }} />
                      <span>{gameInfo()!.agent_names[stat.player]}</span>
                      <span class={styles.statValue}>
                        {stat.land} land &middot; {stat.armies} army
                      </span>
                      <span class={styles.statValue} style={{ color: "#8888a0", "font-size": "10px" }}>
                        {label()}
                      </span>
                    </div>
                  );
                }}
              </For>
            </div>
          </div>
        </div>
      </Show>
    </div>
  );
};

export default LiveApp;
