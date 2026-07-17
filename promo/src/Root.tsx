import React from "react";
import { Composition } from "remotion";
import { Promo, TOTAL } from "./Promo";

export const RemotionRoot: React.FC = () => {
  return (
    <Composition
      id="HawserPromo"
      component={Promo}
      durationInFrames={TOTAL}
      fps={30}
      width={1920}
      height={1080}
    />
  );
};
