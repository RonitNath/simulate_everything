import { Component, createSignal, createEffect, onCleanup, Show, For } from "solid-js";
import type { Replay } from "./types";
import Board from "./Board";
import Nav from "./Nav";
import * as styles from "./styles/app.css";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

const App: Component = () => {
  const [replay, setReplay] = createSignal<Replay | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [turn, setTurn] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeed] = createSignal(10);

  // Config
  const [showNumbers, setShowNumbers] = createSignal(false);
  const [seed, setSeed] = createSignal("");
  const [players, setPlayers] = createSignal(2);
  const [maxTurns, setMaxTurns] = createSignal(500);

  const maxTurn = () => {
    const r = replay();
    return r ? r.frames.length - 1 : 0;
  };
  const frame = () => replay()?.frames[turn()];

  const fetchGame = async () => {
    setLoading(true);
    setPlaying(false);
    const params = new URLSearchParams();
    if (seed()) params.set("seed", seed());
    params.set("players", String(players()));
    params.set("turns", String(maxTurns()));

    const res = await fetch(`/api/game?${params}`);
    const data: Replay = await res.json();
    setReplay(data);
    setTurn(0);
    setPlaying(false);
    setLoading(false);
  };

  // Fetch on mount with random seed
  createEffect(() => { fetchGame(); });

  // Playback timer
  createEffect(() => {
    if (!playing()) return;
    const ms = Math.max(16, 1000 / speed());
    const id = setInterval(() => {
      setTurn((t) => {
        if (t >= maxTurn()) {
          setPlaying(false);
          return t;
        }
        return t + 1;
      });
    }, ms);
    onCleanup(() => clearInterval(id));
  });

  // Keyboard controls
  const onKey = (e: KeyboardEvent) => {
    if ((e.target as HTMLElement).tagName === "INPUT") return;
    switch (e.key) {
      case " ":
        e.preventDefault();
        setPlaying((p) => !p);
        break;
      case "ArrowRight":
        setPlaying(false);
        setTurn((t) => Math.min(t + 1, maxTurn()));
        break;
      case "ArrowLeft":
        setPlaying(false);
        setTurn((t) => Math.max(t - 1, 0));
        break;
      case "Home":
        setPlaying(false);
        setTurn(0);
        break;
      case "End":
        setPlaying(false);
        setTurn(maxTurn());
        break;
    }
  };

  createEffect(() => {
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  return (
    <div class={styles.app}>
      <div class={styles.header}>
        <span class={styles.title}>Simulate Everything</span>
        <Nav />
      </div>

      {/* Config bar */}
      <div class={styles.configBar}>
        <label class={styles.configLabel}>
          Seed
          <input
            class={styles.configInput}
            type="text"
            placeholder="random"
            value={seed()}
            onInput={(e) => setSeed(e.currentTarget.value)}
            style={{ width: "80px" }}
          />
        </label>
        <label class={styles.configLabel}>
          Players
          <select
            class={styles.configInput}
            value={players()}
            onChange={(e) => setPlayers(parseInt(e.currentTarget.value))}
          >
            <option value="2">2</option>
            <option value="3">3</option>
            <option value="4">4</option>
          </select>
        </label>
        <label class={styles.configLabel}>
          Turns
          <select
            class={styles.configInput}
            value={maxTurns()}
            onChange={(e) => setMaxTurns(parseInt(e.currentTarget.value))}
          >
            <option value="100">100</option>
            <option value="250">250</option>
            <option value="500">500</option>
            <option value="1000">1000</option>
          </select>
        </label>
        <button class={styles.btnPrimary} onClick={fetchGame} disabled={loading()}>
          {loading() ? "Running..." : "New Game"}
        </button>
        <Show when={replay()}>
          {(r) => (
            <span style={{ "font-size": "12px", color: "#8888a0", "margin-left": "auto" }}>
              {r().width}x{r().height}
              <Show when={r().winner !== null}>
                {" "}&middot; Winner: {r().agent_names[r().winner!]}
              </Show>
            </span>
          )}
        </Show>
      </div>

      <Show when={replay() && frame()} fallback={
        <div style={{ display: "flex", "align-items": "center", "justify-content": "center", flex: 1, color: "#8888a0" }}>
          {loading() ? "Generating game..." : "No game loaded"}
        </div>
      }>
        <div class={styles.controls}>
          <button class={styles.btn} onClick={() => { setPlaying(false); setTurn(0); }}>&#x23EE;</button>
          <button class={styles.btn} onClick={() => { setPlaying(false); setTurn((t) => Math.max(t - 1, 0)); }}>&#x23F4;</button>
          <button class={styles.btn} onClick={() => setPlaying((p) => !p)}>
            {playing() ? "\u23F8" : "\u25B6"}
          </button>
          <button class={styles.btn} onClick={() => { setPlaying(false); setTurn((t) => Math.min(t + 1, maxTurn())); }}>&#x23F5;</button>
          <button class={styles.btn} onClick={() => { setPlaying(false); setTurn(maxTurn()); }}>&#x23ED;</button>
          <span class={styles.turnLabel}>Turn {frame()!.turn} / {replay()!.frames[maxTurn()].turn}</span>
          <input
            type="range"
            class={styles.slider}
            min={0}
            max={maxTurn()}
            value={turn()}
            onInput={(e) => { setPlaying(false); setTurn(parseInt(e.currentTarget.value)); }}
          />
        </div>

        <div class={styles.speedControls}>
          <span>Speed:</span>
          <For each={[1, 5, 10, 25, 50, 100]}>
            {(s) => (
              <button
                class={styles.btn}
                style={{ "font-weight": speed() === s ? "bold" : "normal", "font-size": "10px", padding: "2px 6px" }}
                onClick={() => setSpeed(s)}
              >
                {s}x
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
            <Board frame={frame()!} width={replay()!.width} height={replay()!.height} showNumbers={showNumbers()} />
          </div>

          <div class={styles.sidebar}>
            <div class={styles.statsPanel}>
              <For each={frame()!.stats}>
                {(stat) => (
                  <div class={`${styles.playerStat} ${!stat.alive ? styles.eliminated : ""}`}>
                    <div class={styles.playerDot} style={{ background: PLAYER_COLORS[stat.player % PLAYER_COLORS.length] }} />
                    <span>{replay()!.agent_names[stat.player]}</span>
                    <span class={styles.statValue}>
                      {stat.land} land &middot; {stat.armies} army
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

export default App;
