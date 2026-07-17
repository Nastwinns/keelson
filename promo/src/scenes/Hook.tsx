import React from "react";
import { AbsoluteFill, useCurrentFrame, useVideoConfig, interpolate } from "remotion";
import { theme } from "../theme";
import { typed, rise } from "../components/anim";

export const Hook: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const cursor = Math.floor(frame / 15) % 2 === 0 ? "▋" : " ";
  const line1 = typed("Your product lives in 10 repos.", frame, 6, 34);
  const line2 = typed("Which commits go together?", frame, 46, 30);
  const showLine2 = frame > 46;
  const brand = rise(frame, fps, 92, 30);
  const dim = interpolate(frame, [92, 108], [1, 0.28], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

  return (
    <AbsoluteFill
      style={{
        background: theme.bg,
        justifyContent: "center",
        alignItems: "center",
        fontFamily: theme.mono,
      }}
    >
      <div style={{ textAlign: "center", opacity: dim }}>
        <div style={{ fontSize: 60, color: theme.text, fontWeight: 600 }}>
          {line1}
          {!showLine2 && <span style={{ color: theme.accent }}>{cursor}</span>}
        </div>
        <div style={{ height: 24 }} />
        <div style={{ fontSize: 60, color: theme.red, fontWeight: 700 }}>
          {line2}
          {showLine2 && <span style={{ color: theme.accent }}>{cursor}</span>}
        </div>
      </div>

      <div
        style={{
          position: "absolute",
          bottom: 250,
          textAlign: "center",
          ...brand,
        }}
      >
        <div
          style={{
            fontSize: 92,
            fontWeight: 800,
            letterSpacing: 2,
            background: `linear-gradient(90deg, ${theme.accent}, ${theme.blue})`,
            WebkitBackgroundClip: "text",
            WebkitTextFillColor: "transparent",
          }}
        >
          hawser
        </div>
        <div style={{ fontSize: 30, color: theme.dim, marginTop: 6 }}>
          one lockfile for your whole fleet
        </div>
      </div>
    </AbsoluteFill>
  );
};
