import { Composition } from "remotion";
import { ZylaTeaser, TEASER_FPS, TEASER_FRAMES, TEASER_HEIGHT, TEASER_WIDTH } from "./ZylaTeaser";

export const RemotionRoot = () => (
  <Composition
    id="ZylaTeaser"
    component={ZylaTeaser}
    durationInFrames={TEASER_FRAMES}
    fps={TEASER_FPS}
    width={TEASER_WIDTH}
    height={TEASER_HEIGHT}
  />
);
