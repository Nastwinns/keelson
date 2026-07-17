import React from "react";
import { AbsoluteFill, useCurrentFrame, useVideoConfig, interpolate } from "remotion";
import { theme, fleet } from "../theme";
import { pop, fadeIn } from "../components/anim";

// scattered card positions (x%, y%, rotation)
const spots = [
  { x: 16, y: 30, r: -6 },
  { x: 50, y: 20, r: 3 },
  { x: 82, y: 33, r: 7 },
  { x: 30, y: 66, r: 5 },
  { x: 70, y: 68, r: -5 },
];

export const Problem: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();

  return (
    <AbsoluteFill
      style={{ background: theme.bg, fontFamily: theme.mono, overflow: "hidden" }}
    >
      <div
        style={{
          position: "absolute",
          top: 70,
          width: "100%",
          textAlign: "center",
          fontSize: 40,
          color: theme.dim,
          opacity: fadeIn(frame, 4),
        }}
      >
        five real embedded upstreams —{" "}
        <span style={{ color: theme.red }}>none pinned yet</span>
      </div>

      {fleet.map((r, i) => {
        const s = pop(frame, fps, 18 + i * 12);
        const p = spots[i];
        const jitter = Math.sin((frame + i * 20) / 22) * 4;
        return (
          <div
            key={r.name}
            style={{
              position: "absolute",
              left: `${p.x}%`,
              top: `${p.y}%`,
              transform: `translate(-50%,-50%) rotate(${p.r}deg) translateY(${jitter}px) scale(${s})`,
              opacity: s,
              width: 340,
              background: theme.panel,
              border: `1px solid ${theme.border}`,
              borderLeft: `4px solid ${r.color}`,
              borderRadius: 12,
              padding: "18px 22px",
              boxShadow: "0 20px 60px rgba(0,0,0,0.5)",
            }}
          >
            <div style={{ fontSize: 30, color: theme.text, fontWeight: 700 }}>
              {r.name}
            </div>
            <div style={{ fontSize: 20, color: theme.dim, marginTop: 4 }}>
              {r.repo}
            </div>
            <div
              style={{
                marginTop: 14,
                fontSize: 22,
                color: theme.red,
                display: "flex",
                gap: 8,
                alignItems: "center",
              }}
            >
              <span style={{ color: theme.dim }}>🔓</span>
              <span
                style={{
                  background: "rgba(248,81,73,0.12)",
                  padding: "2px 12px",
                  borderRadius: 6,
                  letterSpacing: 1,
                }}
              >
                unpinned
              </span>
              <span style={{ color: theme.dim, fontSize: 18 }}>{r.group}</span>
            </div>
          </div>
        );
      })}

      <div
        style={{
          position: "absolute",
          bottom: 70,
          width: "100%",
          textAlign: "center",
          fontSize: 32,
          color: theme.text,
          opacity: interpolate(frame, [95, 115], [0, 1], {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
          }),
        }}
      >
        <span style={{ color: theme.red }}>"works on my machine."</span>{" "}
        nobody wrote down which set was live.
      </div>
    </AbsoluteFill>
  );
};
