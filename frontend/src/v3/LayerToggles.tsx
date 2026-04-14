import type { Component } from "solid-js";
import * as css from "../styles/v3.css";

export type V3RenderLayer = "territory" | "roads" | "settlements" | "depots";

interface LayerTogglesProps {
  layers: Set<V3RenderLayer>;
  onToggle: (layer: V3RenderLayer) => void;
}

const ALL_LAYERS: { key: V3RenderLayer; label: string }[] = [
  { key: "territory", label: "Territory" },
  { key: "roads", label: "Roads" },
  { key: "settlements", label: "Settlements" },
  { key: "depots", label: "Depots" },
];

const LayerToggles: Component<LayerTogglesProps> = (props) => {
  return (
    <div class={css.v3LayerToggles}>
      {ALL_LAYERS.map((l) => (
        <label class={css.v3LayerLabel}>
          <input
            type="checkbox"
            checked={props.layers.has(l.key)}
            onChange={() => props.onToggle(l.key)}
          />
          {l.label}
        </label>
      ))}
    </div>
  );
};

export default LayerToggles;
