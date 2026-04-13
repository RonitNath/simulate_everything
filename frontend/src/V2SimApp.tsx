import { Component, createSignal, createEffect, onCleanup, Show, For } from "solid-js";
import type { V2Replay } from "./v2types";
import HexBoard from "./HexBoard";
import Nav from "./Nav";
import * as styles from "./styles/app.css";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

const V2SimApp: Component = () => {
  const [replay, setReplay] = createSignal<V2Replay | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [frameIdx, setFrameIdx] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeed] = createSignal(10);
  const [showNumbers, setShowStrength] = createSignal(false);

  // Config
  const [seed, setSeed] = createSignal("");
  const [players, setPlayers] = createSignal(2);
  const [width, setWidth] = createSignal(30);
  const [height, setHeight] = createSignal(30);
  const [maxTicks, setMaxTicks] = createSignal(2000);

  const maxIdx = () => {
    const r = replay();
    return r ? r.frames.length - 1 : 0;
  };
  const frame = () => replay()?.frames[frameIdx()];

  const fetchGame = async () => {
    setLoading(true);
    setPlaying(false);
    const params = new URLSearchParams();
    if (seed()) params.set("seed", seed());
    params.set("players", String(players()));
    params.set("width", String(width()));
    params.set("height", String(height()));
    params.set("ticks", String(maxTicks()));

    const res = await fetch(`/api/v2/game?${params}`);
    const data: V2Replay = await res.json();
    setReplay(data);
    setFrameIdx(0);
    setPlaying(false);
    setLoading(false);
  };

  // Fetch on mount
  createEffect(() => { fetchGame(); });

  // Playback timer
  createEffect(() => {
    if (!playing()) return;
    const ms = Math.max(16, 1000 / speed());
    const id = setInterval(() => {
      setFrameIdx((t) => {
        if (t >= maxIdx()) {
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
        setFrameIdx((t) => Math.min(t + 1, maxIdx()));
        break;
      case "ArrowLeft":
        setPlaying(false);
        setFrameIdx((t) => Math.max(t - 1, 0));
        break;
      case "Home":
        setPlaying(false);
        setFrameIdx(0);
        break;
      case "End":
        setPlaying(false);
        setFrameIdx(maxIdx());
        break;
    }
  };

  createEffect(() => {
    window.addEventListener("keydown", onKey);
    onCleanup(() => window.removeEventListener("keydown", onKey));
  });

  // Per-player stats from current frame
  const playerStats = () => {
    const f = frame();
    const r = replay();
    if (!f || !r) return [];
    return Array.from({ length: r.num_players }, (_, i) => ({
      id: i,
      units: f.units.filter((u) => u.owner === i).length,
      food: f.player_food[i] ?? 0,
      material: f.player_material[i] ?? 0,
      alive: f.alive[i] ?? false,
    }));
  };

  return (
    <div class={styles.app}>
      <div class={styles.header}>
        <span class={styles.title}>Generals V2</span>
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
          Size
          <select
            class={styles.configInput}
            value={width()}
            onChange={(e) => { const v = parseInt(e.currentTarget.value); setWidth(v); setHeight(v); }}
          >
            <option value="20">20x20</option>
            <option value="30">30x30</option>
            <option value="40">40x40</option>
            <option value="50">50x50</option>
          </select>
        </label>
        <label class={styles.configLabel}>
          Ticks
          <select
            class={styles.configInput}
            value={maxTicks()}
            onChange={(e) => setMaxTicks(parseInt(e.currentTarget.value))}
          >
            <option value="500">500</option>
            <option value="1000">1000</option>
            <option value="2000">2000</option>
            <option value="5000">5000</option>
          </select>
        </label>
        <button class={styles.btnPrimary} onClick={fetchGame} disabled={loading()}>
          {loading() ? "Running..." : "New Game"}
        </button>
        <Show when={replay()}>
          {(r) => (
            <span style={{ "font-size": "12px", color: "#8888a0", "margin-left": "auto" }}>
              {r().width}x{r().height} hex
              <Show when={r().winner !== null}>
                {" "}&middot; {r().timed_out ? "Timeout" : "Winner"}: {r().agent_names[r().winner!]}
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
          <button class={styles.btn} onClick={() => { setPlaying(false); setFrameIdx(0); }}>&#x23EE;</button>
          <button class={styles.btn} onClick={() => { setPlaying(false); setFrameIdx((t) => Math.max(t - 1, 0)); }}>&#x23F4;</button>
          <button class={styles.btn} onClick={() => setPlaying((p) => !p)}>
            {playing() ? "\u23F8" : "\u25B6"}
          </button>
          <button class={styles.btn} onClick={() => { setPlaying(false); setFrameIdx((t) => Math.min(t + 1, maxIdx())); }}>&#x23F5;</button>
          <button class={styles.btn} onClick={() => { setPlaying(false); setFrameIdx(maxIdx()); }}>&#x23ED;</button>
          <span class={styles.turnLabel}>Tick {frame()!.tick} / {replay()!.frames[maxIdx()].tick}</span>
          <input
            type="range"
            class={styles.slider}
            min={0}
            max={maxIdx()}
            value={frameIdx()}
            onInput={(e) => { setPlaying(false); setFrameIdx(parseInt(e.currentTarget.value)); }}
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
            onClick={() => setShowStrength((s) => !s)}
          >
            {showNumbers() ? "#" : "#\u0338"}
          </button>
        </div>

        <div class={styles.main}>
          <div class={styles.boardContainer}>
            <HexBoard
              terrain={replay()!.terrain}
              units={frame()!.units}
              width={replay()!.width}
              height={replay()!.height}
              showNumbers={showNumbers()}
            />
          </div>

          <div class={styles.sidebar}>
            <div class={styles.statsPanel}>
              <For each={playerStats()}>
                {(stat) => (
                  <div class={`${styles.playerStat} ${!stat.alive ? styles.eliminated : ""}`}>
                    <div class={styles.playerDot} style={{ background: PLAYER_COLORS[stat.id % PLAYER_COLORS.length] }} />
                    <span>{replay()!.agent_names[stat.id]}</span>
                    <span class={styles.statValue}>
                      {stat.units} units &middot; {stat.food.toFixed(1)} food / {stat.material.toFixed(1)} mat
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

export default V2SimApp;
