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

export const gallery = style({
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
  flexShrink: 0,
});

export const headerLeft = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.md,
});

export const title = style({
  fontSize: vars.font.size.lg,
  fontWeight: 700,
  letterSpacing: "0.05em",
  textTransform: "uppercase",
});

export const backLink = style({
  color: vars.color.textMuted,
  textDecoration: "none",
  fontSize: vars.font.size.sm,
  ":hover": {
    color: vars.color.text,
  },
});

export const refreshBtn = style({
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

export const content = style({
  flex: 1,
  overflow: "auto",
  padding: vars.space.md,
});

export const groupHeader = style({
  fontSize: vars.font.size.md,
  fontWeight: 600,
  color: vars.color.textMuted,
  textTransform: "uppercase",
  letterSpacing: "0.08em",
  padding: `${vars.space.md} 0 ${vars.space.sm}`,
  borderBottom: `1px solid ${vars.color.border}`,
  marginBottom: vars.space.sm,
});

export const emptyState = style({
  textAlign: "center",
  padding: vars.space.xl,
  color: vars.color.textMuted,
  fontSize: vars.font.size.sm,
});

export const card = style({
  border: `1px solid ${vars.color.border}`,
  borderRadius: vars.radius.md,
  marginBottom: vars.space.sm,
  overflow: "hidden",
  background: vars.color.surface,
});

export const cardHeader = style({
  display: "flex",
  alignItems: "center",
  gap: vars.space.sm,
  padding: `${vars.space.sm} ${vars.space.md}`,
  cursor: "pointer",
  userSelect: "none",
  ":hover": {
    background: vars.color.surfaceHover,
  },
});

export const cardChevron = style({
  fontSize: vars.font.size.sm,
  color: vars.color.textMuted,
  width: "16px",
  flexShrink: 0,
});

export const cardInfo = style({
  flex: 1,
  display: "flex",
  flexDirection: "column",
  gap: "2px",
  minWidth: 0,
});

export const cardTitle = style({
  fontWeight: 600,
  fontSize: vars.font.size.md,
});

export const cardMeta = style({
  fontSize: vars.font.size.xs,
  color: vars.color.textMuted,
  display: "flex",
  gap: vars.space.sm,
  flexWrap: "wrap",
});

export const badge = style({
  padding: `1px ${vars.space.xs}`,
  borderRadius: vars.radius.sm,
  fontSize: vars.font.size.xs,
  fontWeight: 600,
});

export const badgePass = style({
  background: "#1a3a2a",
  color: "#4aff8a",
});

export const badgeFail = style({
  background: "#3a1a1a",
  color: "#ff4a6a",
});

export const annotation = style({
  fontSize: vars.font.size.xs,
  color: vars.color.textMuted,
  fontStyle: "italic",
  marginTop: "2px",
});

export const filmstrip = style({
  maxWidth: "100%",
  maxHeight: "64px",
  borderRadius: vars.radius.sm,
  marginTop: vars.space.xs,
  opacity: 0.8,
});

export const iframeContainer = style({
  borderTop: `1px solid ${vars.color.border}`,
  height: "80vh",
});

export const iframe = style({
  width: "100%",
  height: "100%",
  border: "none",
});

export const noReviewHtml = style({
  padding: vars.space.md,
  color: vars.color.textMuted,
  fontSize: vars.font.size.sm,
  textAlign: "center",
  borderTop: `1px solid ${vars.color.border}`,
});
