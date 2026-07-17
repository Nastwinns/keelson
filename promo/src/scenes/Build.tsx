import React from "react";
import { AbsoluteFill, useCurrentFrame, interpolate } from "remotion";
import { theme, fleet } from "../theme";
import { Terminal } from "../components/Terminal";
import { fadeIn, typed } from "../components/anim";

export const Build: React.FC = () => {
  const frame = useCurrentFrame();
  const cmd = typed("haw build -j4 && haw test", frame, 4, 22);

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
        3 — build &amp; test the whole fleet in parallel
      </div>

      <Terminal title="haw build -j4" width={1220}>
        <div style={{ fontSize: 24, lineHeight: 1.5 }}>
          <div style={{ color: theme.text, marginBottom: 16 }}>
            <span style={{ color: theme.green }}>$ </span>
            {cmd}
            {frame < 30 && <span style={{ color: theme.accent }}>▋</span>}
          </div>

          {fleet.map((r, i) => {
            const start = 30 + i * 6;
            const prog = interpolate(frame, [start, start + 40], [0, 1], {
              extrapolateLeft: "clamp",
              extrapolateRight: "clamp",
            });
            const done = prog >= 1;
            return (
              <div
                key={r.name}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 16,
                  marginBottom: 14,
                  opacity: fadeIn(frame, start),
                }}
              >
                <span
                  style={{
                    width: 150,
                    color: theme.text,
                    fontWeight: 600,
                  }}
                >
                  {r.name}
                </span>
                <div
                  style={{
                    flex: 1,
                    height: 22,
                    background: theme.panel,
                    borderRadius: 6,
                    overflow: "hidden",
                    border: `1px solid ${theme.border}`,
                  }}
                >
                  <div
                    style={{
                      width: `${prog * 100}%`,
                      height: "100%",
                      background: done ? theme.green : r.color,
                      transition: "none",
                    }}
                  />
                </div>
                <span
                  style={{
                    width: 300,
                    fontSize: 20,
                    color: done ? theme.green : theme.dim,
                  }}
                >
                  {done ? `✓ ${r.ok}` : `${Math.round(prog * 100)}%`}
                </span>
              </div>
            );
          })}

          <div
            style={{
              marginTop: 18,
              fontSize: 22,
              color: theme.blue,
              opacity: fadeIn(frame, 118),
            }}
          >
            coremark → <span style={{ color: theme.text }}>CoreMark 1.0 : 26021.34</span>
            <span style={{ color: theme.green }}>  ·  cJSON 100% tests passed (19)</span>
          </div>
        </div>
      </Terminal>
    </AbsoluteFill>
  );
};
