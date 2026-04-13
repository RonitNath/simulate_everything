import { style, globalStyle } from "@vanilla-extract/css";
import { vars } from "./theme.css";

export const board = style({
  display: "grid",
  gap: "1px",
  background: vars.color.border,
  border: `1px solid ${vars.color.border}`,
  borderRadius: vars.radius.md,
  overflow: "hidden",
  maxWidth: "100%",
  maxHeight: "100%",
});

export const cell = style({
  position: "relative",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  aspectRatio: "1",
  fontSize: vars.font.size.xs,
  fontWeight: 600,
  userSelect: "none",
  transition: "background 0.1s ease",
  minWidth: 0,
  minHeight: 0,
  overflow: "hidden",
});

export const cellMountain = style({
  background: vars.color.mountain,
});

export const cellEmpty = style({
  background: vars.color.empty,
});

export const cellCity = style({
  position: "relative",
  "::after": {
    content: '""',
    position: "absolute",
    top: "2px",
    right: "2px",
    width: "4px",
    height: "4px",
    borderRadius: "1px",
    background: vars.color.city,
    opacity: 0.6,
  },
});

export const cellGeneral = style({
  position: "relative",
  "::after": {
    content: '""',
    position: "absolute",
    top: "2px",
    right: "2px",
    width: "5px",
    height: "5px",
    background: vars.color.general,
    clipPath: "polygon(50% 0%, 100% 100%, 0% 100%)",
  },
});

export const armyCount = style({
  fontSize: vars.font.size.xs,
  lineHeight: 1,
  textShadow: "0 1px 2px rgba(0,0,0,0.8)",
  color: "#fff",
});
