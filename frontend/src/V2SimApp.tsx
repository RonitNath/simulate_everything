import { Component, createSignal, createEffect, createMemo, onCleanup, Show, For } from "solid-js";
import type { V2Replay, V2ReplayFrame, BoardStaticData, BoardFrameData, V2ScoreSnapshot } from "./v2types";
import { normalizeReplayStatic, normalizeReplayFrame } from "./v2types";
import HexBoard from "./HexBoard";
import type { RenderLayer } from "./HexBoard";
import Nav from "./Nav";
import * as styles from "./styles/app.css";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

const ALL_LAYERS: RenderLayer[] = ["territory", "roads", "depots", "settlements", "convoys", "destinations"];

const V2SimApp: Component = () => {
  const [replay, setReplay] = createSignal<V2Replay | null>(null);
  const [loading, setLoading] = createSignal(false);
  const [frameIdx, setFrameIdx] = createSignal(0);
  const [playing, setPlaying] = createSignal(false);
  const [speed, setSpeed] = createSignal(10);
  const [showNumbers, setShowStrength] = createSignal(false);
  const [layers, setLayers] = createSignal<Set<RenderLayer>>(
    new Set(["territory", "roads", "depots", "settlements", "convoys"])
  );

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
  const frame = (): V2ReplayFrame | undefined => replay()?.frames[frameIdx()];

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

  const staticData = createMemo((): BoardStaticData | null => {
    const r = replay();
    return r ? normalizeReplayStatic(r) : null;
  });

  const currentFrameData = createMemo((): BoardFrameData | null => {
    const f = frame();
    return f ? normalizeReplayFrame(f) : null;
  });

  // Per-player stats from current frame
  const playerStats = () => {
    const f = frame();
    const r = replay();
    if (!f || !r) return [];
    return Array.from({ length: r.num_players }, (_, i) => {
      const pops = f.population.filter(p => p.owner === i);
      const totalPop = pops.reduce((s, p) => s + p.count, 0);
      const farmers = pops.filter(p => p.role === "Farmer").reduce((s, p) => s + p.count, 0);
      const workers = pops.filter(p => p.role === "Worker").reduce((s, p) => s + p.count, 0);
      const soldiers = pops.filter(p => p.role === "Soldier").reduce((s, p) => s + p.count, 0);
      const territoryCount = (f.cells ?? []).filter(c => c.stockpile_owner === i).length;
      const hexPops = new Map<string, number>();
      for (const p of pops) {
        const key = `${p.q},${p.r}`;
        hexPops.set(key, (hexPops.get(key) ?? 0) + p.count);
      }
      const settlements = [...hexPops.values()].filter(c => c >= 10).length;
      const convoyCount = (f.convoys ?? []).filter(c => c.owner === i).length;
      const score: V2ScoreSnapshot | undefined = (f.scores ?? []).find(s => s.player_id === i);
      return {
        id: i,
        units: f.units.filter((u) => u.owner === i).length,
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

      <Show when={replay() && frame() && staticData() && currentFrameData()} fallback={
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
              numPlayers={replay()!.num_players}
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
                      <span>{replay()!.agent_names[stat.id]}</span>
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
                <svg width="14" height="14"><path d="M3,4 L3,12 L11,12 L11,4 L7,1 Z" fill="rgba(74,158,255,0.9)" stroke="#fff" stroke-width="0.5" /></svg>
                <span>Settlement</span>
                <svg width="14" height="14"><rect x="3" y="3" width="8" height="8" fill="#c0a000" stroke="#8a7200" stroke-width="0.5" /></svg>
                <span>Depot</span>
                <svg width="14" height="14"><line x1="2" y1="7" x2="7" y2="7" stroke="rgba(200,200,180,0.6)" stroke-width="2" stroke-linecap="round" /><line x1="7" y1="7" x2="12" y2="4" stroke="rgba(220,200,140,0.7)" stroke-width="2" stroke-linecap="round" /></svg>
                <span>Road network</span>
                <svg width="14" height="14"><path d="M3,10 L5,6 L7,8 L9,6 L11,10 Z" fill="#fff" stroke="#000" stroke-width="0.5" /></svg>
                <span>General (crown)</span>
                <svg width="14" height="14"><line x1="1" y1="7" x2="13" y2="7" stroke="#ff6644" stroke-width="3" stroke-linecap="round" /></svg>
                <span>Combat edge</span>
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

export default V2SimApp;
