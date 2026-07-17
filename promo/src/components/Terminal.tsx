import React from "react";
import { theme } from "../theme";

export const Terminal: React.FC<{
  title?: string;
  width?: number | string;
  children: React.ReactNode;
  style?: React.CSSProperties;
}> = ({ title = "haw — fleet", width = 1180, children, style }) => {
  return (
    <div
      style={{
        width,
        background: theme.bgSoft,
        border: `1px solid ${theme.border}`,
        borderRadius: 14,
        boxShadow: "0 40px 120px rgba(0,0,0,0.55)",
        overflow: "hidden",
        fontFamily: theme.mono,
        ...style,
      }}
    >
      <div
        style={{
          height: 46,
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: "0 18px",
          background: theme.panel,
          borderBottom: `1px solid ${theme.border}`,
        }}
      >
        <Dot c="#ff5f56" />
        <Dot c="#ffbd2e" />
        <Dot c="#27c93f" />
        <span
          style={{
            marginLeft: 14,
            color: theme.dim,
            fontSize: 20,
            letterSpacing: 0.4,
          }}
        >
          {title}
        </span>
      </div>
      <div style={{ padding: "26px 30px" }}>{children}</div>
    </div>
  );
};

const Dot: React.FC<{ c: string }> = ({ c }) => (
  <span
    style={{
      width: 14,
      height: 14,
      borderRadius: "50%",
      background: c,
      display: "inline-block",
    }}
  />
);
