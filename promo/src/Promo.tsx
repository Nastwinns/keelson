import React from "react";
import { AbsoluteFill, Series } from "remotion";
import { theme } from "./theme";
import { Hook } from "./scenes/Hook";
import { Problem } from "./scenes/Problem";
import { Manifest } from "./scenes/Manifest";
import { Sync } from "./scenes/Sync";
import { Build } from "./scenes/Build";
import { Verify } from "./scenes/Verify";
import { Payoff } from "./scenes/Payoff";

export const DURATIONS = {
  hook: 120,
  problem: 150,
  manifest: 150,
  sync: 150,
  build: 180,
  verify: 120,
  payoff: 150,
};

export const TOTAL = Object.values(DURATIONS).reduce((a, b) => a + b, 0);

export const Promo: React.FC = () => {
  return (
    <AbsoluteFill style={{ background: theme.bg }}>
      <Series>
        <Series.Sequence durationInFrames={DURATIONS.hook}>
          <Hook />
        </Series.Sequence>
        <Series.Sequence durationInFrames={DURATIONS.problem}>
          <Problem />
        </Series.Sequence>
        <Series.Sequence durationInFrames={DURATIONS.manifest}>
          <Manifest />
        </Series.Sequence>
        <Series.Sequence durationInFrames={DURATIONS.sync}>
          <Sync />
        </Series.Sequence>
        <Series.Sequence durationInFrames={DURATIONS.build}>
          <Build />
        </Series.Sequence>
        <Series.Sequence durationInFrames={DURATIONS.verify}>
          <Verify />
        </Series.Sequence>
        <Series.Sequence durationInFrames={DURATIONS.payoff}>
          <Payoff />
        </Series.Sequence>
      </Series>
    </AbsoluteFill>
  );
};
