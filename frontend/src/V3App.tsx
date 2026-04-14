import {
  Component, Show, batch, createMemo, createSignal, onCleanup,
} from "solid-js";
import type {
  PlayerInfo,
  V3Init,
  V3ServerToSpectator,
  V3Snapshot,
  V3SnapshotDelta,
} from "./v3types";
import ScoreBar from "./v3/ScoreBar";
import Inspector from "./v3/Inspector";
import ViewerCanvas from "./v3/ViewerCanvas";
import { applySnapshotDelta } from "./v3/applySnapshotDelta";
import * as css from "./styles/v3.css";

const V3App: Component = () => {
  const [phase, setPhase] = createSignal<"connecting" | "playing" | "game_over">("connecting");
  const [initData, setInitData] = createSignal<V3Init | null>(null);
  const [currentFrame, setCurrentFrame] = createSignal<V3Snapshot | null>(null);
  const [selectedEntityId, setSelectedEntityId] = createSignal<number | null>(null);
  const [tickMs, setTickMs] = createSignal(100);
  const [serverPaused, setServerPaused] = createSignal(false);
  const [gameNumber, setGameNumber] = createSignal(0);
  const [winner, setWinner] = createSignal<number | null>(null);

  let wsRef: WebSocket | null = null;

  const selectedEntity = createMemo(() => {
    const id = selectedEntityId();
    const frame = currentFrame();
    if (id == null || !frame) return null;
    return frame.entities.find((entity) => entity.id === id) ?? null;
  });

  const players = createMemo<PlayerInfo[]>(() => currentFrame()?.players ?? []);
  const agentNames = createMemo<string[]>(() => initData()?.agent_names ?? []);

  function resync() {
    wsRef?.close();
  }

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
    batch(() => {
      setCurrentFrame(null);
      setSelectedEntityId(null);
      setWinner(null);
    });
  }

  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const wsUrl = `${protocol}//${window.location.host}/ws/v3/rr?format=json`;
  let retryDelay = 500;
  let dead = false;

  function connect() {
    const ws = new WebSocket(wsUrl);
    wsRef = ws;

    ws.onopen = () => {
      retryDelay = 500;
    };

    ws.onmessage = (ev: MessageEvent) => {
      const msg: V3ServerToSpectator = JSON.parse(ev.data);
      switch (msg.type) {
        case "v3_init":
          batch(() => {
            setInitData(msg);
            setGameNumber(msg.game_number);
            setCurrentFrame(null);
            setSelectedEntityId(null);
            setWinner(null);
            setPhase("playing");
          });
          break;
        case "v3_snapshot":
          setCurrentFrame(msg as V3Snapshot);
          break;
        case "v3_snapshot_delta":
          setCurrentFrame((prev) => (prev ? applySnapshotDelta(prev, msg as V3SnapshotDelta) : prev));
          break;
        case "v3_game_end":
          batch(() => {
            setPhase("game_over");
            setWinner(msg.winner);
          });
          break;
        case "v3_config":
          if (msg.tick_ms != null) setTickMs(msg.tick_ms);
          break;
        case "v3_rr_status":
          batch(() => {
            setServerPaused(msg.paused);
            setTickMs(msg.tick_ms);
            setGameNumber(msg.game_number);
          });
          break;
      }
    };

    ws.onclose = () => {
      if (dead) return;
      setTimeout(connect, retryDelay);
      retryDelay = Math.min(retryDelay * 2, 2000);
    };
  }

  connect();
  onCleanup(() => {
    dead = true;
    wsRef?.close();
  });

  return (
    <Show
      when={phase() !== "connecting"}
      fallback={<div class={css.v3Connecting}>Connecting to V3 viewer...</div>}
    >
      <div class={css.v3App}>
        <ScoreBar
          players={players()}
          agentNames={agentNames()}
          gameNumber={gameNumber()}
        />

        <div class={css.v3Main}>
          <div class={css.v3Canvas}>
            <ViewerCanvas onEntityClick={setSelectedEntityId} />
          </div>
          <Inspector
            entity={selectedEntity()}
            onClose={() => setSelectedEntityId(null)}
          />
        </div>

        <div class={css.v3Footer}>
          <div class={css.v3Controls}>
            <span class={css.v3Label}>Tick {currentFrame()?.tick ?? 0}</span>
            <span class={css.v3Label}>{serverPaused() ? "Paused" : "Live"}</span>
            <button class={css.v3Btn} type="button" onClick={() => setSpeed(25)}>10x</button>
            <button class={css.v3Btn} type="button" onClick={() => setSpeed(50)}>2x</button>
            <button class={css.v3Btn} type="button" onClick={() => setSpeed(100)}>1x</button>
            <button class={css.v3Btn} type="button" onClick={serverPaused() ? serverResume : serverPause}>
              {serverPaused() ? "Resume" : "Pause"}
            </button>
            <button class={css.v3Btn} type="button" onClick={resetGame}>Reset</button>
            <button class={css.v3Btn} type="button" onClick={resync}>Resync</button>
          </div>
          <Show when={winner() != null}>
            <span style={{ color: "#ffd700", "font-weight": "bold", "font-size": "12px" }}>
              Winner: P{winner()}
            </span>
          </Show>
        </div>
      </div>
    </Show>
  );
};

export default V3App;
