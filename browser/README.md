# Browser development

This folder contains the Zyla Browser app and its Windows host. The repository root has the user guide; this file covers the local development workflow.

## Requirements

- Windows 10 or 11, x64
- [Bun](https://bun.sh/) and the stable Rust toolchain
- Visual Studio C++ Build Tools with the MSVC toolchain
- Microsoft Edge WebView2 Runtime to run the browser

## Install and build

```powershell
bun install --frozen-lockfile
bun run build
```

`bun run build` recompiles the Rust host before packaging, so embedded pages such as Settings always match the source. The host build produces `host/target/release/tsgpui-browser-host.exe`; the application build writes `dist/zyla-browser.exe`.

Run the app in development mode with `bun run dev`; it also recompiles the host first. Run the Rust address-bar unit tests with `bun run test`.

## Project layout

- `src/` — browser chrome, new-tab and history pages, and the frontend bridge
- `host/src/` — the Windows WebView2 host, tab lifecycle, navigation, storage, and password vault
- `assets/` — application icon and bundled font
- `vendor/rs-tsgpui-0.3.0.tgz` — the MIT-licensed TSGPUI package used by the build scripts
- `../website/` — screenshots and the static project landing page

The WebView2 user profile lives in `%LOCALAPPDATA%\TSGPUI Browser`. Local profile folders and browser test data must never be committed.
