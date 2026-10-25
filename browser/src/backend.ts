import type { AppDefinition } from "@rs/tsgpui/types";

// The browser keeps its tabs in the host and its bookmarks and history in chrome.js. This backend only
// satisfies the TSGPUI page contract, so the bundle builds like any other TSGPUI app.
const app: AppDefinition<{ ready: boolean }> = {
  state: { ready: false },
  actions: {}
};

export default app;
