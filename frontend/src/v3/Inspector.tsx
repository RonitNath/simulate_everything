import { Component, For, Show } from "solid-js";
import type { SpectatorEntityInfo } from "../v3types";
import { playerColorHex } from "./colors";
import * as css from "../styles/v3.css";

interface InspectorProps {
  entity: SpectatorEntityInfo | null;
  onClose: () => void;
}

const Inspector: Component<InspectorProps> = (props) => {
  function siteLabel(e: SpectatorEntityInfo): string | null {
    const tags = e.physical?.tags ?? [];
    if (!e.site) return null;
    if (tags.includes("Farm")) return "Farm";
    if (tags.includes("Workshop")) return "Workshop";
    if (tags.includes("Settlement")) return "Settlement";
    return "Site";
  }

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

              <Show when={siteLabel(e)}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Site</span>
                    <span>{siteLabel(e)}</span>
                  </div>
                  <Show when={e.site}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Build</span>
                      <span>{(e.site!.build_progress * 100).toFixed(0)}%</span>
                    </div>
                  </Show>
                  <Show when={e.contains_count > 0}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Population</span>
                      <span>{e.contains_count}</span>
                    </div>
                  </Show>
                </div>
              </Show>

              <Show when={e.matter}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Matter</span>
                    <span>{e.matter!.commodity}: {e.matter!.amount.toFixed(0)}</span>
                  </div>
                </div>
              </Show>

              <Show when={e.physical}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Material</span>
                    <span>{e.physical!.material}</span>
                  </div>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>State</span>
                    <span>{e.physical!.matter_state}</span>
                  </div>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Tags</span>
                    <span>{e.physical!.tags.join(", ") || "none"}</span>
                  </div>
                </div>
              </Show>

              <Show when={e.tool}>
                <div class={css.v3InspectorSection}>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Tool Force</span>
                    <span>{e.tool!.force_mult.toFixed(1)}x</span>
                  </div>
                  <div class={css.v3InspectorRow}>
                    <span class={css.v3InspectorLabel}>Durability</span>
                    <span>{(e.tool!.durability * 100).toFixed(0)}%</span>
                  </div>
                </div>
              </Show>

              <Show when={e.current_goal || e.current_action || e.needs}>
                <div class={css.v3InspectorSection}>
                  <Show when={e.current_goal}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Goal</span>
                      <span>{e.current_goal}</span>
                    </div>
                  </Show>
                  <Show when={e.current_action}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Action</span>
                      <span>{e.current_action}</span>
                    </div>
                  </Show>
                  <Show when={e.decision_reason}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Why</span>
                      <span>{e.decision_reason}</span>
                    </div>
                  </Show>
                  <Show when={e.action_queue_preview && e.action_queue_preview.length > 0}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Queue</span>
                      <span>{e.action_queue_preview!.join(" → ")}</span>
                    </div>
                  </Show>
                  <Show when={e.needs}>
                    <div class={css.v3InspectorRow}>
                      <span class={css.v3InspectorLabel}>Needs</span>
                      <span>
                        H {e.needs!.hunger.toFixed(2)} | S {e.needs!.safety.toFixed(2)} | D {e.needs!.duty.toFixed(2)}
                      </span>
                    </div>
                  </Show>
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
