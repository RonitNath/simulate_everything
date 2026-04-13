import { Component, createSignal, createEffect, onCleanup, For, Show } from "solid-js";
import Nav from "./Nav";
import * as styles from "./styles/app.css";

interface AgentRow {
  id: string;
  wins: number;
  losses: number;
  draws: number;
  games: number;
  win_rate: string;
}

interface ScoreboardData {
  total_games: number;
  agents: AgentRow[];
}

const ScoreboardApp: Component = () => {
  const [data, setData] = createSignal<ScoreboardData | null>(null);

  const fetchData = async () => {
    const res = await fetch("/api/scoreboard");
    const json = await res.json();
    setData(json);
  };

  createEffect(() => {
    fetchData();
    const id = setInterval(fetchData, 3000);
    onCleanup(() => clearInterval(id));
  });

  return (
    <div class={styles.app}>
      <div class={styles.header}>
        <span class={styles.title}>Scoreboard</span>
        <Nav />
      </div>
      <div style={{ padding: "24px", overflow: "auto", flex: 1 }}>
        <Show when={data()} fallback={<span style={{ color: "#8888a0" }}>Loading...</span>}>
          {(d) => (
            <>
              <div style={{ "margin-bottom": "16px", color: "#8888a0", "font-size": "12px" }}>
                {d().total_games} games played
              </div>
              <table class={styles.table}>
                <thead>
                  <tr>
                    <th>#</th>
                    <th>Agent</th>
                    <th>Win%</th>
                    <th>W</th>
                    <th>L</th>
                    <th>D</th>
                    <th>Games</th>
                  </tr>
                </thead>
                <tbody>
                  <For each={d().agents}>
                    {(agent, i) => (
                      <tr>
                        <td style={{ color: "#8888a0" }}>{i() + 1}</td>
                        <td style={{ "font-weight": "bold" }}>{agent.id}</td>
                        <td>{agent.win_rate}%</td>
                        <td style={{ color: "#4aff8a" }}>{agent.wins}</td>
                        <td style={{ color: "#ff4a6a" }}>{agent.losses}</td>
                        <td style={{ color: "#8888a0" }}>{agent.draws}</td>
                        <td>{agent.games}</td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
            </>
          )}
        </Show>
      </div>
    </div>
  );
};

export default ScoreboardApp;
