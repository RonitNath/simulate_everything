import {
  Component,
  Show,
  batch,
  createSignal,
  onCleanup,
  onMount,
} from "solid-js";
import type {
  V3Init,
  V3Snapshot,
  SpectatorEntityInfo,
} from "./v3types";
import type { BiomeName } from "./v2types";
import V3HexCanvas from "./v3/HexCanvas";
import type { V3RenderLayer } from "./v3/LayerToggles";
import * as css from "./styles/v3.css";

const V3DrillApp: Component = () => {
  const [initData, setInitData] = createSignal<V3Init | null>(null);
  const [frame, setFrame] = createSignal<V3Snapshot | null>(null);
  const [status, setStatus] = createSignal("Connecting...");
  const [settled, setSettled] = createSignal(true);
  const [layers] = createSignal<Set<V3RenderLayer>>(new Set());

  // Minimal biome mapping — flat drill pad.
  const biomes = () => {
    const init = initData();
    if (!init) return [];
    const count = init.width * init.height;
    return Array.from({ length: count }, () => "grassland" as BiomeName);
  };

  const heights = () => {
    const init = initData();
    return init?.height_map ?? [];
  };

  const rivers = () => {
    const init = initData();
    if (!init) return [];
    return Array.from({ length: init.width * init.height }, () => false);
  };

  onMount(() => {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(`${proto}//${location.host}/ws/v3/drill`);

    ws.onopen = () => setStatus("Connected");

    ws.onmessage = (ev) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const msg: any = JSON.parse(ev.data);

      switch (msg.type) {
        case "v3_init": {
          batch(() => {
            setInitData(msg as V3Init);
            setFrame(null);
            setStatus(`Init: ${msg.width}x${msg.height}`);
          });
          break;
        }
        case "v3_snapshot": {
          setFrame(msg as V3Snapshot);
          setSettled(true);
          break;
        }
        case "v3_snapshot_delta": {
          setFrame((prev) => {
            if (!prev) return null;
            const entities: SpectatorEntityInfo[] = [...prev.entities];

            // Remove.
            const removedSet = new Set(msg.entities_removed);
            const kept = entities.filter((e) => !removedSet.has(e.id));

            // Update.
            for (const u of msg.entities_updated) {
              const e = kept.find((e) => e.id === u.id);
              if (!e) continue;
              if (u.x !== undefined) e.x = u.x;
              if (u.y !== undefined) e.y = u.y;
              if (u.z !== undefined) e.z = u.z;
              if (u.hex_q !== undefined) e.hex_q = u.hex_q;
              if (u.hex_r !== undefined) e.hex_r = u.hex_r;
              if (u.facing !== undefined) e.facing = u.facing;
              if (u.blood !== undefined) e.blood = u.blood;
              if (u.stamina !== undefined) e.stamina = u.stamina;
              if (u.wounds !== undefined) e.wounds = u.wounds;
              if (u.attack_phase !== undefined) e.attack_phase = u.attack_phase ?? undefined;
              if (u.attack_motion !== undefined) e.attack_motion = u.attack_motion ?? undefined;
              if (u.weapon_angle !== undefined) e.weapon_angle = u.weapon_angle ?? undefined;
              if (u.attack_progress !== undefined) e.attack_progress = u.attack_progress ?? undefined;
            }

            // Appeared.
            kept.push(...msg.entities_appeared);

            return {
              ...prev,
              tick: msg.tick,
              dt: msg.dt,
              entities: kept,
              projectiles: prev.projectiles,
              stacks: prev.stacks,
            };
          });
          setSettled(true);
          break;
        }
      }
    };

    ws.onclose = () => setStatus("Disconnected");
    ws.onerror = () => setStatus("WS error");

    onCleanup(() => ws.close());
  });

  return (
    <div class={css.v3App}>
      <div style="padding: 8px; font-family: monospace; font-size: 13px;">
        <span style="color: #888;">Drill Pad</span>
        {" | "}
        <span>{status()}</span>
        {" | "}
        <span style={`color: ${settled() ? "#4c4" : "#cc4"}`}>
          {settled() ? "READY" : "SETTLING"}
        </span>
      </div>
      <Show when={initData() != null && frame() != null}>
        <V3HexCanvas
          width={initData()!.width}
          height={initData()!.height}
          biomes={biomes()}
          heights={heights()}
          rivers={rivers()}
          frame={frame()}
          layers={layers()}
          tickIntervalMs={50}
          focusRegion={{
            minRow: 1,
            minCol: 1,
            maxRow: 1,
            maxCol: initData()!.width - 2,
          }}
        />
      </Show>
    </div>
  );
};

export default V3DrillApp;
