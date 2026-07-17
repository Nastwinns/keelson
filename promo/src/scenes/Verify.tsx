import React from "react";
import { AbsoluteFill, useCurrentFrame, useVideoConfig } from "remotion";
import { theme } from "../theme";
import { Terminal } from "../components/Terminal";
import { fadeIn, typed, pop } from "../components/anim";

export const Verify: React.FC = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const cmd = typed("haw verify", frame, 4, 16);
  const check = pop(frame, fps, 52);

  return (
    <AbsoluteFill
      style={{
        background: theme.bg,
        fontFamily: theme.mono,
        justifyContent: "center",
        alignItems: "center",
      }}
    >
      <div
        style={{
          fontSize: 34,
          color: theme.dim,
          marginBottom: 26,
          opacity: fadeIn(frame, 2),
        }}
      >
        4 — drop <span style={{ color: theme.accent }}>haw verify</span> into CI
      </div>

      <Terminal title="ci · haw verify" width={1020}>
        <div style={{ fontSize: 28, lineHeight: 1.6 }}>
          <div style={{ color: theme.text }}>
            <span style={{ color: theme.green }}>$ </span>
            {cmd}
            {frame < 22 && <span style={{ color: theme.accent }}>▋</span>}
          </div>
          <div
            style={{
              marginTop: 20,
              color: theme.dim,
              opacity: fadeIn(frame, 30),
            }}
          >
            checking on-disk tree against haw.lock…
          </div>
          <div
            style={{
              marginTop: 22,
              display: "flex",
              alignItems: "center",
              gap: 18,
              opacity: check,
              transform: `scale(${0.9 + check * 0.1})`,
            }}
          >
            <span
              style={{
                width: 56,
                height: 56,
                borderRadius: "50%",
                background: theme.green,
                color: theme.bg,
                fontSize: 36,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontWeight: 800,
              }}
            >
              ✓
            </span>
            <span style={{ color: theme.green, fontSize: 34, fontWeight: 700 }}>
              tree matches lock · exit 0
            </span>
          </div>
          <div
            style={{
              marginTop: 14,
              color: theme.dim,
              fontSize: 22,
              opacity: fadeIn(frame, 74),
            }}
          >
            drift would exit <span style={{ color: theme.red }}>3</span> and fail the pipeline.
          </div>
        </div>
      </Terminal>
    </AbsoluteFill>
  );
};
