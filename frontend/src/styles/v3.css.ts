import { style } from "@vanilla-extract/css";
import { vars } from "./theme.css";

export const v3App = style({
  display: "flex",
  flexDirection: "column",
  height: "100vh",
  overflow: "hidden",
});

export const v3Header = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: `${vars.space.xs} ${vars.space.md}`,
  borderBottom: `1px solid ${vars.color.border}`,
  background: vars.color.surface,
  flexShrink: 0,
});

export const v3Title = style({
  fontSize: vars.font.size.md,
  fontWeight: 700,
  letterSpacing: "0.05em",
  textTransform: "uppercase",
});

export const v3Main = style({
  display: "flex",
  flex: 1,
  overflow: "hidden",
});

export const v3Canvas = style({
  flex: 1,
  position: "relative",
  overflow: "hidden",
  minWidth: 0,
});

export const v3Inspector = style({
  width: "320px",
  borderLeft: `1px solid ${vars.color.border}`,
  background: vars.color.surface,
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
  flexShrink: 0,
});

export const v3Footer = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.sm,
  padding: `${vars.space.xs} ${vars.space.md}`,
  borderTop: `1px solid ${vars.color.border}`,
  background: vars.color.surface,
  flexShrink: 0,
});

export const v3ScoreStrip = style({
  display: "flex",
  gap: vars.space.sm,
  padding: `${vars.space.xs} ${vars.space.md}`,
  borderBottom: `1px solid ${vars.color.border}`,
  background: vars.color.surface,
  overflowX: "auto",
  flexShrink: 0,
});

export const v3ScorePlayer = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.xs,
  fontSize: vars.font.size.xs,
  color: vars.color.textMuted,
  whiteSpace: "nowrap",
});

export const v3PlayerDot = style({
  width: "8px",
  height: "8px",
  borderRadius: "50%",
  flexShrink: 0,
});

export const v3Controls = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.sm,
  fontSize: vars.font.size.sm,
});

export const v3Btn = style({
  background: vars.color.surfaceHover,
  border: `1px solid ${vars.color.border}`,
  color: vars.color.text,
  padding: `${vars.space.xs} ${vars.space.sm}`,
  borderRadius: vars.radius.sm,
  cursor: "pointer",
  fontFamily: vars.font.mono,
  fontSize: vars.font.size.sm,
  ":hover": {
    background: vars.color.border,
  },
});

export const v3BtnActive = style({
  background: vars.color.players.p0,
  borderColor: vars.color.players.p0,
  color: "#fff",
});

export const v3Slider = style({
  flex: 1,
  accentColor: vars.color.players.p0,
  minWidth: "80px",
});

export const v3Label = style({
  fontSize: vars.font.size.xs,
  color: vars.color.textMuted,
  minWidth: "60px",
});

export const v3LayerToggles = style({
  display: "flex",
  gap: vars.space.sm,
  alignItems: "center",
  fontSize: vars.font.size.xs,
  color: vars.color.textMuted,
});

export const v3LayerLabel = style({
  display: "flex",
  alignItems: "center",
  gap: "3px",
  cursor: "pointer",
  userSelect: "none",
});

export const v3Connecting = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  height: "100vh",
  fontSize: vars.font.size.lg,
  color: vars.color.textMuted,
});

export const v3Tooltip = style({
  position: "absolute",
  background: "rgba(10, 10, 20, 0.92)",
  color: "#e0e0e0",
  border: "1px solid #444",
  borderRadius: "4px",
  padding: "6px 8px",
  fontSize: "11px",
  lineHeight: "1.4",
  pointerEvents: "none",
  zIndex: "10",
  maxWidth: "200px",
  fontFamily: "monospace",
});
