import { Component, For, Show } from "solid-js";
import type { SpectatorEntityInfo } from "../v3types";
import { playerColorHex } from "./colors";
import * as css from "../styles/v3.css";

interface InspectorProps {
  entity: SpectatorEntityInfo | null;
  onClose: () => void;
}

const Inspector: Component<InspectorProps> = (props) => {
  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") props.onClose();
  }

  return (
    <Show when={props.entity}>
      {(entity) => {
        const e = entity();
        return (
          <div
            class={css.v3Inspector}
            tabIndex={0}
            onKeyDown={handleKeyDown}
            ref={(el) => el.focus()}
          >
            <div class={css.v3InspectorHeader}>
              <span style={{ "font-weight": "bold" }}>
                Entity #{e.id}
              </span>
              <button class={css.v3Btn} onClick={() => props.onClose()}>
                {"\u2715"}
              </button>
            </div>

            <div class={css.v3InspectorBody}>
              {/* Identity */}
              <div class={css.v3InspectorSection}>
                <div class={css.v3InspectorRow}>
                  <span class={css.v3InspectorLabel}>Owner</span>
                  <span style={{
                    color: e.owner != null ? playerColorHex(e.owner) : "#888",
                  }}>
                    {e.owner != null ? `P${e.owner}` : "none"}
                  </span>
                </div>
                <div class={css.v3InspectorRow}>
                  <span class={css.v3InspectorLabel}>Kind</span>
                  <span>{e.entity_kind}</span>
                </div>
                <Show when={e.role}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Role</span>
                    <span>{e.role}</span>
                  </div>
                </Show>
              </div>

              {/* Position */}
              <div class={css.v3InspectorSection}>
                <div class={css.v3InspectorRow}>
                  <span class={css.v3InspectorLabel}>Position</span>
                  <span>({e.x.toFixed(1)}, {e.y.toFixed(1)}, {e.z.toFixed(1)})</span>
                </div>
                <div class={css.v3InspectorRow}>
                  <span class={css.v3InspectorLabel}>Hex</span>
                  <span>({e.hex_q}, {e.hex_r})</span>
                </div>
                <Show when={e.facing != null}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Facing</span>
                    <span>{((e.facing! * 180 / Math.PI) % 360).toFixed(0)}&deg;</span>
                  </div>
                </Show>
              </div>

              {/* Vitals */}
              <Show when={e.blood != null || e.stamina != null}>
                <div class={css.v3InspectorSection}>
                  <Show when={e.blood != null}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Blood</span>
                      <div class={css.v3BarContainer}>
                        <div
                          class={css.v3BarFill}
                          style={{
                            width: `${(e.blood! * 100).toFixed(0)}%`,
                            background: e.blood! > 0.5 ? "#44cc44" : e.blood! > 0.25 ? "#cccc44" : "#cc4444",
                          }}
                        />
                      </div>
                      <span>{(e.blood! * 100).toFixed(0)}%</span>
                    </div>
                  </Show>
                  <Show when={e.stamina != null}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Stamina</span>
                      <div class={css.v3BarContainer}>
                        <div
                          class={css.v3BarFill}
                          style={{
                            width: `${(e.stamina! * 100).toFixed(0)}%`,
                            background: "#4488cc",
                          }}
                        />
                      </div>
                      <span>{(e.stamina! * 100).toFixed(0)}%</span>
                    </div>
                  </Show>
                </div>
              </Show>

              {/* Wounds */}
              <Show when={e.wounds && e.wounds.length > 0}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorLabel}>Wounds ({e.wounds!.length})</div>
                  <For each={e.wounds}>
                    {([zone, severity]) => (
                      <div class={css.v3InspectorRow} style={{ "padding-left": "8px" }}>
                        <span style={{ color: "#ff8844" }}>{zone}</span>
                        <span style={{
                          color: severity === "Critical" ? "#ff2222" :
                                 severity === "Serious" ? "#ff6644" : "#ffaa44",
                        }}>
                          {severity}
                        </span>
                      </div>
                    )}
                  </For>
                </div>
              </Show>

              {/* Equipment */}
              <Show when={e.weapon_type || e.armor_type}>
                <div class={css.v3InspectorSection}>
                  <Show when={e.weapon_type}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Weapon</span>
                      <span>{e.weapon_type}</span>
                    </div>
                  </Show>
                  <Show when={e.armor_type}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Armor</span>
                      <span>{e.armor_type}</span>
                    </div>
                  </Show>
                </div>
              </Show>

              {/* Stack */}
              <Show when={e.stack_id != null}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Stack</span>
                    <span>#{e.stack_id}</span>
                  </div>
                </div>
              </Show>

              {/* Structure / Resource */}
              <Show when={e.structure_type}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Structure</span>
                    <span>{e.structure_type}</span>
                  </div>
                  <Show when={e.contains_count > 0}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Population</span>
                      <span>{e.contains_count}</span>
                    </div>
                  </Show>
                </div>
              </Show>

              <Show when={e.resource_type}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Resource</span>
                    <span>{e.resource_type}: {e.resource_amount?.toFixed(0)}</span>
                  </div>
                </div>
              </Show>

              <Show when={e.current_task}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Task</span>
                    <span>{e.current_task}</span>
                  </div>
                </div>
              </Show>
            </div>
          </div>
        );
      }}
    </Show>
  );
};

export default Inspector;
