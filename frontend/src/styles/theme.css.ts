import { createGlobalTheme } from "@vanilla-extract/css";

export const vars = createGlobalTheme(":root", {
  color: {
    bg: "#0a0a0f",
    surface: "#141420",
    surfaceHover: "#1a1a2e",
    border: "#2a2a3e",
    text: "#e0e0e8",
    textMuted: "#8888a0",
    mountain: "#3a3a4a",
    empty: "#1e1e2e",
    city: "#c0c0d0",
    general: "#ffd700",
    players: {
      p0: "#4a9eff",
      p1: "#ff4a6a",
      p2: "#4aff8a",
      p3: "#ffa04a",
      p4: "#c04aff",
      p5: "#4affd0",
      p6: "#ff4aff",
      p7: "#d0ff4a",
    },
  },
  font: {
    mono: "'JetBrains Mono', 'Fira Code', monospace",
    size: {
      xs: "10px",
      sm: "12px",
      md: "14px",
      lg: "18px",
      xl: "24px",
    },
  },
  space: {
    xs: "4px",
    sm: "8px",
    md: "16px",
    lg: "24px",
    xl: "32px",
  },
  radius: {
    sm: "4px",
    md: "8px",
  },
});
