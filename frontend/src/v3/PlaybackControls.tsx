import type { Component } from "solid-js";
import * as css from "../styles/v3.css";

interface PlaybackControlsProps {
  tick: number;
  maxTick: number;
  playing: boolean;
  following: boolean;
  tickMs: number;
  serverPaused: boolean;
  onTogglePlay: () => void;
  onStep: () => void;
  onSeek: (tick: number) => void;
  onBackToLive: () => void;
  onSetSpeed: (ms: number) => void;
  onServerPause: () => void;
  onServerResume: () => void;
  onReset: () => void;
}

const SPEED_PRESETS = [
  { label: "0.5x", ms: 200 },
  { label: "1x", ms: 100 },
  { label: "2x", ms: 50 },
  { label: "5x", ms: 20 },
  { label: "10x", ms: 10 },
];

const PlaybackControls: Component<PlaybackControlsProps> = (props) => {
  return (
    <div class={css.v3Footer}>
      <div class={css.v3Controls}>
        <button
          class={css.v3Btn}
          onClick={() => props.onTogglePlay()}
          title={props.playing ? "Pause" : "Play"}
        >
          {props.playing ? "\u23F8" : "\u25B6"}
        </button>
        <button
          class={css.v3Btn}
          onClick={() => props.onStep()}
          title="Step forward"
        >
          {"\u25B6|"}
        </button>
        <button
          class={`${css.v3Btn} ${props.following ? css.v3BtnActive : ""}`}
          onClick={() => props.onBackToLive()}
          title="Follow live"
        >
          LIVE
        </button>
      </div>

      <input
        type="range"
        class={css.v3Slider}
        min={0}
        max={props.maxTick}
        value={props.tick}
        onInput={(e) => props.onSeek(parseInt(e.currentTarget.value))}
      />

      <span class={css.v3Label}>
        t={props.tick}
      </span>

      <div class={css.v3Controls}>
        {SPEED_PRESETS.map((preset) => (
          <button
            class={`${css.v3Btn} ${props.tickMs === preset.ms ? css.v3BtnActive : ""}`}
            onClick={() => props.onSetSpeed(preset.ms)}
          >
            {preset.label}
          </button>
        ))}
      </div>

      <div class={css.v3Controls}>
        <button
          class={css.v3Btn}
          onClick={() =>
            props.serverPaused ? props.onServerResume() : props.onServerPause()
          }
          title={props.serverPaused ? "Resume server" : "Pause server"}
        >
          {props.serverPaused ? "SRV \u25B6" : "SRV ||"}
        </button>
        <button
          class={css.v3Btn}
          onClick={() => props.onReset()}
          title="Reset game"
        >
          RST
        </button>
      </div>
    </div>
  );
};

export default PlaybackControls;
