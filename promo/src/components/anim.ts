import { interpolate, spring } from "remotion";

// Reveal characters of a string over `frames`, starting at `start`.
export const typed = (
  text: string,
  frame: number,
  start: number,
  frames: number
): string => {
  const n = Math.round(
    interpolate(frame, [start, start + frames], [0, text.length], {
      extrapolateLeft: "clamp",
      extrapolateRight: "clamp",
    })
  );
  return text.slice(0, n);
};

export const fadeIn = (frame: number, start: number, dur = 12) =>
  interpolate(frame, [start, start + dur], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });

export const rise = (frame: number, fps: number, start: number, dist = 40) => {
  const s = spring({ frame: frame - start, fps, config: { damping: 200 } });
  return { opacity: s, transform: `translateY(${(1 - s) * dist}px)` };
};

export const pop = (frame: number, fps: number, start: number) =>
  spring({ frame: frame - start, fps, config: { damping: 12, mass: 0.6 } });
