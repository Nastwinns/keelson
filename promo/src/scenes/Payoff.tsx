import React from "react";
import { AbsoluteFill, useCurrentFrame, useVideoConfig, interpolate } from "remotion";
import { theme } from "../theme";
import { fadeIn, rise } from "../components/anim";

export const Payoff: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const brand = rise(frame, fps, 10, 40);
  const glow = interpolate(frame, [0, 40], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  return (
    <AbsoluteFill
      style={{
        background: `radial-gradient(circle at 50% 40%, rgba(138,43,226,${
          0.16 * glow
        }), ${theme.bg} 60%)`,
        fontFamily: theme.mono,
        justifyContent: "center",
        alignItems: "center",
      }}
    >
      <div style={{ textAlign: "center", ...brand }}>
        <div
          style={{
            fontSize: 96,
            fontWeight: 800,
            letterSpacing: 1,
            background: `linear-gradient(90deg, ${theme.accent}, ${theme.blue})`,
            WebkitBackgroundClip: "text",
            WebkitTextFillColor: "transparent",
          }}
        >
          hawser
        </div>
        <div style={{ fontSize: 42, color: theme.text, marginTop: 10 }}>
          the <span style={{ color: theme.accent }}>identical</span> tree —
          you, your CI, your teammate.
        </div>
      </div>

      <div
        style={{
          marginTop: 60,
          background: theme.bgSoft,
          border: `1px solid ${theme.border}`,
          borderRadius: 12,
          padding: "22px 40px",
          fontSize: 34,
          color: theme.text,
          opacity: fadeIn(frame, 46),
        }}
      >
        <span style={{ color: theme.green }}>$ </span>
        cargo install hawser
      </div>

      <div
        style={{
          marginTop: 34,
          fontSize: 30,
          color: theme.dim,
          opacity: fadeIn(frame, 64),
        }}
      >
        hawser.dev · github.com/Nastwinns/hawser · MIT/Apache-2.0 · one binary, in Rust
      </div>
    </AbsoluteFill>
  );
};
