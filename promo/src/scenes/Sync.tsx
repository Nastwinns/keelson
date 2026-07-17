import React from "react";
import { AbsoluteFill, useCurrentFrame, interpolate } from "remotion";
import { theme, fleet } from "../theme";
import { Terminal } from "../components/Terminal";
import { fadeIn, typed } from "../components/anim";

export const Sync: React.FC = () => {
  const frame = useCurrentFrame();
  const cmd = typed("haw sync", frame, 4, 16);
  const lockStart = 96;

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
        2 — <span style={{ color: theme.accent }}>haw sync</span> clones every repo
        &amp; writes the lockfile
      </div>

      <Terminal title="haw — fleet" width={1180}>
        <div style={{ fontSize: 26, lineHeight: 1.5 }}>
          <div style={{ color: theme.text }}>
            <span style={{ color: theme.green }}>$ </span>
            {cmd}
            {frame < 24 && <span style={{ color: theme.accent }}>▋</span>}
          </div>

          {fleet.map((r, i) => {
            const start = 26 + i * 12;
            const done = frame > start + 8;
            return (
              <div
                key={r.name}
                style={{
                  opacity: fadeIn(frame, start),
                  color: theme.dim,
                  marginTop: 6,
                }}
              >
                <span style={{ color: done ? theme.green : theme.amber }}>
                  {done ? "✓" : "⟳"}
                </span>{" "}
                cloning{" "}
                <span style={{ color: theme.text }}>{r.repo}</span>
                {done && (
                  <span style={{ color: theme.dim }}>
                    {"  "}→ pinned{" "}
                    <span style={{ color: theme.blue }}>{r.sha}</span>
                  </span>
                )}
              </div>
            );
          })}

          <div
            style={{
              marginTop: 22,
              padding: "16px 20px",
              background: "rgba(163,113,247,0.08)",
              border: `1px solid ${theme.accent}`,
              borderRadius: 10,
              opacity: fadeIn(frame, lockStart),
            }}
          >
            <span style={{ color: theme.accent }}>haw.lock</span>{" "}
            <span style={{ color: theme.dim }}>
              — 5 repos pinned to exact SHAs. commit it.
            </span>
          </div>
        </div>
      </Terminal>
    </AbsoluteFill>
  );
};
