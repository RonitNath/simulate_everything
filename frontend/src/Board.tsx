import { Component, For, createMemo } from "solid-js";
import type { Frame } from "./types";
import * as styles from "./styles/board.css";

const PLAYER_COLORS = [
  "#4a9eff", "#ff4a6a", "#4aff8a", "#ffa04a",
  "#c04aff", "#4affd0", "#ff4aff", "#d0ff4a",
];

function parseHex(hex: string): [number, number, number] {
  return [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ];
}

function playerRgbDim(owner: number, t: number): string {
  const [r, g, b] = parseHex(PLAYER_COLORS[owner % PLAYER_COLORS.length]);
  return `rgb(${Math.round(r * t)},${Math.round(g * t)},${Math.round(b * t)})`;
}

function armyBrightness(count: number, maxArmy: number): number {
  if (count <= 0) return 0.35;
  return 0.35 + 0.65 * Math.log1p(count) / Math.log1p(Math.max(maxArmy, 1));
}

interface BoardProps {
  frame: Frame;
  width: number;
  height: number;
  showNumbers?: boolean;
}

const Board: Component<BoardProps> = (props) => {
  const cellSize = createMemo(() => {
    const maxW = (window.innerWidth - 320) * 0.9;
    const maxH = (window.innerHeight - 100) * 0.9;
    // Account for 1px gap between cells.
    const availW = maxW - (props.width - 1);
    const availH = maxH - (props.height - 1);
    return Math.max(2, Math.min(
      Math.floor(availW / props.width),
      Math.floor(availH / props.height),
    ));
  });

  const maxArmy = createMemo(() => {
    let max = 1;
    for (const cell of props.frame.grid) {
      if (cell.armies > max) max = cell.armies;
    }
    return max;
  });

  // Max garrison among neutral cities — used for city shading independent of player armies.
  const maxCityGarrison = createMemo(() => {
    let max = 1;
    for (const cell of props.frame.grid) {
      if (cell.tile === "City" && cell.owner === null && cell.armies > max) {
        max = cell.armies;
      }
    }
    return max;
  });

  return (
    <div
      class={styles.board}
      style={{
        "grid-template-columns": `repeat(${props.width}, ${cellSize()}px)`,
        "grid-template-rows": `repeat(${props.height}, ${cellSize()}px)`,
      }}
    >
      <For each={props.frame.grid}>
        {(cell) => {
          const isMountain = cell.tile === "Mountain";
          const isCity = cell.tile === "City";
          const isGeneral = cell.tile === "General";
          const hasOwner = cell.owner !== null;

          let bg: string;
          let boxShadow: string | undefined;
          if (isMountain) {
            bg = "#3a3a4a";
          } else if (hasOwner) {
            const t = armyBrightness(cell.armies, maxArmy());
            bg = playerRgbDim(cell.owner!, isGeneral ? Math.max(t, 0.9) : t);
            if (isGeneral) {
              boxShadow = "inset 0 0 0 2px rgba(255,215,0,0.9), 0 0 8px rgba(255,215,0,0.5)";
            } else if (isCity) {
              boxShadow = "inset 0 0 0 1px rgba(255,255,255,0.7)";
            }
          } else if (isCity) {
            // Shade neutral cities by garrison strength — bigger garrisons appear darker/heavier.
            const cityT = Math.min(cell.armies / Math.max(maxCityGarrison(), 1), 1);
            const lum = 26 + 20 * (1 - cityT); // 26-46 range: strong=dark, weak=lighter
            bg = `rgb(${lum},${lum},${lum + 12})`;
            boxShadow = `inset 0 0 0 1px rgba(255,255,255,${(0.25 + 0.35 * (1 - cityT)).toFixed(2)})`;
          } else {
            bg = "#1e1e2e";
          }

          const classes = [
            styles.cell,
            isMountain && styles.cellMountain,
            !isMountain && !hasOwner && !isCity && styles.cellEmpty,
            isCity && styles.cellCity,
            isGeneral && styles.cellGeneral,
          ].filter(Boolean).join(" ");

          return (
            <div class={classes} style={{ background: bg, "box-shadow": boxShadow }}>
              {props.showNumbers && !isMountain && cell.armies > 0 && (
                <span class={styles.armyCount}>{cell.armies}</span>
              )}
            </div>
          );
        }}
      </For>
    </div>
  );
};

export default Board;
