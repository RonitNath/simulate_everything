import { Component, For, Show } from "solid-js";
import type { PlayerInfo } from "../v3types";
import { playerColorHex } from "./render/grid";
import * as css from "../styles/v3.css";

interface ScoreBarProps {
  players: PlayerInfo[];
  agentNames: string[];
  gameNumber: number;
}

const ScoreBar: Component<ScoreBarProps> = (props) => {
  return (
    <div class={css.v3ScoreStrip}>
      <span class={css.v3ScorePlayer}>G#{props.gameNumber}</span>
      <For each={props.players}>
        {(player) => (
          <span
            class={css.v3ScorePlayer}
            style={{ opacity: player.alive ? 1 : 0.4 }}
          >
            <span
              class={css.v3PlayerDot}
              style={{ background: playerColorHex(player.id) }}
            />
            <span>{props.agentNames[player.id] ?? `P${player.id}`}</span>
            <Show when={player.alive}>
              <span>
                pop:{player.population} ter:{player.territory} sc:{player.score}
              </span>
            </Show>
            <Show when={!player.alive}>
              <span style={{ "text-decoration": "line-through" }}>eliminated</span>
            </Show>
          </span>
        )}
      </For>
    </div>
  );
};

export default ScoreBar;
