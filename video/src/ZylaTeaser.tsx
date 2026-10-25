import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import {
  AbsoluteFill,
  Img,
  Sequence,
  continueRender,
  delayRender,
  interpolate,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";

export const TEASER_WIDTH = 1920;
export const TEASER_HEIGHT = 1080;
export const TEASER_FPS = 30;
// Thirty seconds at 30 fps. Scene lengths below add up to exactly this.
export const TEASER_FRAMES = 900;

const COLOR = {
  base: "#121317",
  surface: "#1b1d24",
  line: "#2a2d37",
  ink: "#f4f5f9",
  ink2: "#a7abb8",
  accent: "#5b8cff",
  accentSoft: "rgba(91, 140, 255, 0.16)",
};

const FONT = '"Public Sans", system-ui, sans-serif';

// Scene windows in frames, relative to the start of the composition.
const SCENES = {
  intro: { from: 0, duration: 90 },
  hook: { from: 90, duration: 150 },
  product: { from: 240, duration: 210 },
  features: { from: 450, duration: 180 },
  build: { from: 630, duration: 180 },
  outro: { from: 810, duration: 90 },
} as const;

// Loads the bundled Public Sans so the first rendered frames already use it.
const useBrandFont = () => {
  const [handle] = useState(() => delayRender("Loading Public Sans"));
  useEffect(() => {
    const face = new FontFace("Public Sans", `url(${staticFile("public-sans.woff2")})`, { weight: "100 900" });
    face
      .load()
      .then((loaded) => {
        document.fonts.add(loaded);
        continueRender(handle);
      })
      .catch((error) => {
        console.error(error);
        continueRender(handle);
      });
  }, [handle]);
};

const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;

// Fades in over `inFrames` and out over the last `outFrames` of a scene of `duration` frames.
const sceneOpacity = (frame: number, duration: number, inFrames = 12, outFrames = 12) => {
  if (outFrames === 0) return interpolate(frame, [0, inFrames], [0, 1], clamp);
  return interpolate(frame, [0, inFrames, duration - outFrames, duration], [0, 1, 1, 0], clamp);
};

const Background = () => {
  const frame = useCurrentFrame();
  const drift = interpolate(frame, [0, TEASER_FRAMES], [0, 90], clamp);
  return (
    <AbsoluteFill
      style={{
        background: `radial-gradient(ellipse 60% 55% at ${50 + drift / 30}% ${40 + drift / 60}%, rgba(91,140,255,0.14), transparent 70%), ${COLOR.base}`,
      }}
    />
  );
};

const Eyebrow = ({ children, style }: { children: ReactNode; style?: CSSProperties }) => (
  <div
    style={{
      fontFamily: FONT,
      fontSize: 26,
      fontWeight: 600,
      letterSpacing: 4,
      textTransform: "uppercase",
      color: COLOR.accent,
      ...style,
    }}
  >
    {children}
  </div>
);

const Intro = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const mark = spring({ frame, fps, config: { damping: 14, stiffness: 120 } });
  const wordmark = interpolate(frame, [18, 36], [0, 1], clamp);
  const tagline = interpolate(frame, [40, 58], [0, 1], clamp);
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", opacity: sceneOpacity(frame, SCENES.intro.duration) }}>
      <Img
        src={staticFile("logo.png")}
        style={{ width: 180, height: 180, transform: `scale(${interpolate(mark, [0, 1], [0.6, 1])})`, opacity: mark }}
      />
      <div
        style={{
          marginTop: 36,
          fontFamily: FONT,
          fontSize: 120,
          fontWeight: 700,
          color: COLOR.ink,
          letterSpacing: -2,
          opacity: wordmark,
          transform: `translateY(${interpolate(wordmark, [0, 1], [24, 0])}px)`,
        }}
      >
        Zyla
      </div>
      <div style={{ marginTop: 14, fontFamily: FONT, fontSize: 34, color: COLOR.ink2, opacity: tagline }}>
        Two pages, one window.
      </div>
    </AbsoluteFill>
  );
};

const HOOK_LINES = [
  ["The", "next", "fastest"],
  ["browser", "is", "coming."],
];

const Hook = () => {
  const frame = useCurrentFrame();
  let wordIndex = 0;
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", opacity: sceneOpacity(frame, SCENES.hook.duration) }}>
      <div style={{ fontFamily: FONT, fontSize: 118, fontWeight: 700, color: COLOR.ink, letterSpacing: -2, lineHeight: 1.12, textAlign: "center" }}>
        {HOOK_LINES.map((line, lineIndex) => (
          <div key={lineIndex}>
            {line.map((word) => {
              const start = 8 + wordIndex * 7;
              wordIndex += 1;
              const reveal = interpolate(frame, [start, start + 14], [0, 1], clamp);
              const isAccent = word === "fastest";
              return (
                <span
                  key={word}
                  style={{
                    display: "inline-block",
                    marginRight: 26,
                    opacity: reveal,
                    transform: `translateY(${interpolate(reveal, [0, 1], [40, 0])}px)`,
                    color: isAccent ? COLOR.accent : COLOR.ink,
                  }}
                >
                  {word}
                </span>
              );
            })}
          </div>
        ))}
      </div>
    </AbsoluteFill>
  );
};

const Product = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const lift = spring({ frame: frame - 6, fps, config: { damping: 18, stiffness: 90 } });
  const chips = ["Sidebar or top strip", "Spaces with their own tint", "Split view"];
  return (
    <AbsoluteFill style={{ opacity: sceneOpacity(frame, SCENES.product.duration), alignItems: "center" }}>
      <Eyebrow style={{ position: "absolute", top: 80, left: 0, right: 0, textAlign: "center" }}>Tabs, your way</Eyebrow>
      <div
        style={{
          position: "absolute",
          top: 180,
          width: 1380,
          height: 620,
          borderRadius: 22,
          overflow: "hidden",
          border: `1px solid ${COLOR.line}`,
          boxShadow: "0 40px 120px rgba(0,0,0,0.6), 0 0 0 1px rgba(91,140,255,0.08)",
          transform: `translateY(${interpolate(lift, [0, 1], [140, 0])}px) scale(${interpolate(lift, [0, 1], [0.94, 1])})`,
          opacity: lift,
        }}
      >
        <Img src={staticFile("sidebar-screen.png")} style={{ width: "100%", height: "100%", objectFit: "cover", objectPosition: "top left" }} />
      </div>
      <div style={{ position: "absolute", top: 840, display: "flex", gap: 22 }}>
        {chips.map((chip, index) => {
          const appear = interpolate(frame, [70 + index * 14, 88 + index * 14], [0, 1], clamp);
          return (
            <div
              key={chip}
              style={{
                fontFamily: FONT,
                fontSize: 30,
                fontWeight: 500,
                color: COLOR.ink,
                padding: "14px 28px",
                borderRadius: 999,
                background: COLOR.accentSoft,
                border: `1px solid rgba(91,140,255,0.35)`,
                opacity: appear,
                transform: `translateY(${interpolate(appear, [0, 1], [18, 0])}px)`,
              }}
            >
              {chip}
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
};

const FEATURES = [
  { title: "Keyboard-first", body: "Ctrl+T, Ctrl+Tab, Ctrl+L and Alt+arrows cover everyday browsing." },
  { title: "Split view", body: "Show two tabs side by side with Ctrl+\\." },
  { title: "Local vault", body: "Passwords and cards are encrypted with Windows DPAPI for your account." },
  { title: "Stays on this PC", body: "Session state, bookmarks and history are saved locally." },
];

const Features = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", opacity: sceneOpacity(frame, SCENES.features.duration) }}>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 36, width: 1480 }}>
        {FEATURES.map((feature, index) => {
          const card = spring({ frame: frame - 10 - index * 12, fps, config: { damping: 18, stiffness: 110 } });
          return (
            <div
              key={feature.title}
              style={{
                padding: "48px 52px",
                borderRadius: 24,
                background: COLOR.surface,
                border: `1px solid ${COLOR.line}`,
                opacity: card,
                transform: `translateY(${interpolate(card, [0, 1], [50, 0])}px)`,
              }}
            >
              <div style={{ width: 14, height: 14, borderRadius: 999, background: COLOR.accent, marginBottom: 26 }} />
              <div style={{ fontFamily: FONT, fontSize: 52, fontWeight: 700, color: COLOR.ink, marginBottom: 14 }}>{feature.title}</div>
              <div style={{ fontFamily: FONT, fontSize: 32, lineHeight: 1.4, color: COLOR.ink2 }}>{feature.body}</div>
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
};

const BUILD_STEPS = [
  { label: "Chrome UI", body: "HTML, CSS and JavaScript, packaged with TSGPUI and Bun" },
  { label: "Rust host", body: "A tao event loop and wry, compiled with Rust 2024" },
  { label: "Tabs", body: "One WebView2 instance per tab, placed beside the sidebar" },
  { label: "Storage", body: "DPAPI-protected vault and local session files" },
];

const Build = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const lineGrow = interpolate(frame, [20, 70], [0, 1], clamp);
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", opacity: sceneOpacity(frame, SCENES.build.duration) }}>
      <Eyebrow style={{ position: "absolute", top: 120, left: 0, right: 0, textAlign: "center" }}>How it is built</Eyebrow>
      <div style={{ position: "relative", display: "flex", gap: 28, width: 1580 }}>
        <div
          style={{
            position: "absolute",
            top: 58,
            left: 150,
            right: 150,
            height: 3,
            background: COLOR.line,
            transformOrigin: "left center",
            transform: `scaleX(${lineGrow})`,
          }}
        />
        {BUILD_STEPS.map((step, index) => {
          const appear = spring({ frame: frame - 24 - index * 14, fps, config: { damping: 16, stiffness: 120 } });
          return (
            <div key={step.label} style={{ flex: 1, textAlign: "center", opacity: appear, transform: `translateY(${interpolate(appear, [0, 1], [30, 0])}px)` }}>
              <div
                style={{
                  margin: "0 auto 28px",
                  width: 116,
                  height: 116,
                  borderRadius: 28,
                  background: COLOR.surface,
                  border: `2px solid ${COLOR.accent}`,
                  fontFamily: FONT,
                  fontSize: 44,
                  fontWeight: 700,
                  color: COLOR.accent,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                }}
              >
                {index + 1}
              </div>
              <div style={{ fontFamily: FONT, fontSize: 40, fontWeight: 700, color: COLOR.ink, marginBottom: 12 }}>{step.label}</div>
              <div style={{ fontFamily: FONT, fontSize: 26, lineHeight: 1.4, color: COLOR.ink2, padding: "0 12px" }}>{step.body}</div>
            </div>
          );
        })}
      </div>
      <div style={{ position: "absolute", bottom: 110, fontFamily: FONT, fontSize: 28, color: COLOR.ink2, opacity: interpolate(frame, [110, 130], [0, 1], clamp) }}>
        Windows x64 · WebView2 engine · Open source under MIT
      </div>
    </AbsoluteFill>
  );
};

const Outro = () => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const mark = spring({ frame, fps, config: { damping: 14, stiffness: 120 } });
  const title = interpolate(frame, [14, 32], [0, 1], clamp);
  const sub = interpolate(frame, [30, 48], [0, 1], clamp);
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", opacity: sceneOpacity(frame, SCENES.outro.duration, 12, 0) }}>
      <Img src={staticFile("logo.png")} style={{ width: 130, height: 130, opacity: mark, transform: `scale(${interpolate(mark, [0, 1], [0.7, 1])})` }} />
      <div style={{ marginTop: 28, fontFamily: FONT, fontSize: 96, fontWeight: 700, color: COLOR.ink, opacity: title, letterSpacing: -1 }}>
        Zyla Browser
      </div>
      <div style={{ marginTop: 12, fontFamily: FONT, fontSize: 34, color: COLOR.ink2, opacity: sub }}>
        Two pages, one window.
      </div>
    </AbsoluteFill>
  );
};

export const ZylaTeaser = () => {
  useBrandFont();
  return (
    <AbsoluteFill style={{ backgroundColor: COLOR.base, fontFamily: FONT }}>
      <Background />
      <Sequence from={SCENES.intro.from} durationInFrames={SCENES.intro.duration}>
        <Intro />
      </Sequence>
      <Sequence from={SCENES.hook.from} durationInFrames={SCENES.hook.duration}>
        <Hook />
      </Sequence>
      <Sequence from={SCENES.product.from} durationInFrames={SCENES.product.duration}>
        <Product />
      </Sequence>
      <Sequence from={SCENES.features.from} durationInFrames={SCENES.features.duration}>
        <Features />
      </Sequence>
      <Sequence from={SCENES.build.from} durationInFrames={SCENES.build.duration}>
        <Build />
      </Sequence>
      <Sequence from={SCENES.outro.from} durationInFrames={SCENES.outro.duration}>
        <Outro />
      </Sequence>
    </AbsoluteFill>
  );
};
