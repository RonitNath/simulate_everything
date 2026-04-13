import { globalStyle, style } from "@vanilla-extract/css";
import { vars } from "./theme.css";

globalStyle("*", {
  margin: 0,
  padding: 0,
  boxSizing: "border-box",
});

globalStyle("body", {
  background: vars.color.bg,
  color: vars.color.text,
  fontFamily: vars.font.mono,
  fontSize: vars.font.size.md,
});

export const app = style({
  display: "flex",
  flexDirection: "column",
  height: "100vh",
  overflow: "hidden",
});

export const header = style({
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  padding: `${vars.space.sm} ${vars.space.md}`,
  borderBottom: `1px solid ${vars.color.border}`,
  background: vars.color.surface,
});

export const title = style({
  fontSize: vars.font.size.lg,
  fontWeight: 700,
  letterSpacing: "0.05em",
  textTransform: "uppercase",
});

export const main = style({
  display: "flex",
  flex: 1,
  overflow: "hidden",
});

export const boardContainer = style({
  flex: 1,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  padding: vars.space.md,
});

export const sidebar = style({
  width: "280px",
  borderLeft: `1px solid ${vars.color.border}`,
  background: vars.color.surface,
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
});

export const controls = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.sm,
  padding: `${vars.space.sm} ${vars.space.md}`,
  borderBottom: `1px solid ${vars.color.border}`,
});

export const turnLabel = style({
  fontSize: vars.font.size.sm,
  color: vars.color.textMuted,
  minWidth: "90px",
});

export const btn = style({
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

export const slider = style({
  flex: 1,
  accentColor: vars.color.players.p0,
});

export const statsPanel = style({
  flex: 1,
  overflow: "auto",
  padding: vars.space.md,
});

export const playerStat = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.sm,
  padding: `${vars.space.xs} 0`,
  fontSize: vars.font.size.sm,
});

export const playerDot = style({
  width: "10px",
  height: "10px",
  borderRadius: "50%",
  flexShrink: 0,
});

export const statValue = style({
  color: vars.color.textMuted,
  marginLeft: "auto",
});

export const eliminated = style({
  opacity: 0.4,
  textDecoration: "line-through",
});

export const speedControls = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.xs,
  padding: `${vars.space.xs} ${vars.space.md}`,
  borderBottom: `1px solid ${vars.color.border}`,
  fontSize: vars.font.size.xs,
  color: vars.color.textMuted,
});

export const nav = style({
  display: "flex",
  gap: vars.space.sm,
  alignItems: "center",
});

export const navLink = style({
  color: vars.color.textMuted,
  textDecoration: "none",
  fontSize: vars.font.size.sm,
  padding: `${vars.space.xs} ${vars.space.sm}`,
  borderRadius: vars.radius.sm,
  transition: "color 0.15s, background 0.15s",
  ":hover": {
    color: vars.color.text,
    background: vars.color.surfaceHover,
  },
});

export const navLinkActive = style({
  color: vars.color.text,
  background: vars.color.surfaceHover,
  fontWeight: 700,
});

export const table = style({
  width: "100%",
  borderCollapse: "collapse",
  fontSize: vars.font.size.sm,
});

globalStyle(`${table} th`, {
  textAlign: "left",
  padding: `${vars.space.sm} ${vars.space.md}`,
  borderBottom: `2px solid ${vars.color.border}`,
  color: vars.color.textMuted,
  fontWeight: 600,
  textTransform: "uppercase",
  fontSize: vars.font.size.xs,
  letterSpacing: "0.05em",
});

globalStyle(`${table} td`, {
  padding: `${vars.space.sm} ${vars.space.md}`,
  borderBottom: `1px solid ${vars.color.border}`,
});

export const configBar = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.md,
  padding: `${vars.space.xs} ${vars.space.md}`,
  borderBottom: `1px solid ${vars.color.border}`,
  background: vars.color.surface,
  fontSize: vars.font.size.sm,
});

export const configLabel = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.xs,
  color: vars.color.textMuted,
  fontSize: vars.font.size.xs,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
});

export const configInput = style({
  background: vars.color.bg,
  border: `1px solid ${vars.color.border}`,
  color: vars.color.text,
  padding: `3px ${vars.space.xs}`,
  borderRadius: vars.radius.sm,
  fontFamily: vars.font.mono,
  fontSize: vars.font.size.sm,
});

export const btnPrimary = style({
  background: vars.color.players.p0,
  border: "none",
  color: "#fff",
  padding: `${vars.space.xs} ${vars.space.md}`,
  borderRadius: vars.radius.sm,
  cursor: "pointer",
  fontFamily: vars.font.mono,
  fontSize: vars.font.size.sm,
  fontWeight: 700,
  ":hover": {
    opacity: 0.85,
  },
  selectors: {
    "&:disabled": {
      opacity: 0.5,
      cursor: "default",
    },
  },
});
