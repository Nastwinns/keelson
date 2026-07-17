import React from "react";
import { AbsoluteFill, useCurrentFrame, interpolate } from "remotion";
import { theme, fleet } from "../theme";
import { Terminal } from "../components/Terminal";
import { fadeIn } from "../components/anim";

const header = [
  { t: "[remote.gh]", c: theme.accent },
  { t: 'url = "https://github.com"', c: theme.dim },
  { t: "", c: theme.dim },
];

export const Manifest: React.FC = () => {
  const frame = useCurrentFrame();

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
        1 — declare the fleet in{" "}
        <span style={{ color: theme.accent }}>haw.toml</span>
      </div>

      <Terminal title="haw.toml" width={1120}>
        <div style={{ fontSize: 25, lineHeight: 1.55 }}>
          {header.map((l, i) => (
            <div
              key={i}
              style={{ color: l.c, opacity: fadeIn(frame, 8 + i * 3), height: 38 }}
            >
              {l.t || " "}
            </div>
          ))}
          {fleet.map((r, i) => {
            const start = 20 + i * 12;
            const op = fadeIn(frame, start);
            return (
              <div key={r.name} style={{ opacity: op, marginBottom: 10 }}>
                <span style={{ color: theme.accent }}>[repo.{r.name}]</span>
                <span style={{ color: theme.dim }}>
                  {"  "}
                  # {r.group}
                </span>
                <div style={{ color: theme.text, paddingLeft: 4 }}>
                  repo = <span style={{ color: theme.green }}>"{r.repo}.git"</span>
                </div>
              </div>
            );
          })}
          <div
            style={{
              marginTop: 14,
              opacity: fadeIn(frame, 92),
              color: theme.accent,
            }}
          >
            [stack.fleet]
            <span style={{ color: theme.dim }}>
              {"  "}
              repos = [ coremark, cjson, monocypher, libcanard, mbedtls ]
            </span>
          </div>
        </div>
      </Terminal>
    </AbsoluteFill>
  );
};
