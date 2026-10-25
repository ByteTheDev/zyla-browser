# Contributing to Zyla Browser

Thanks for helping improve Zyla. Keep changes focused and describe the user-visible behavior they change.

## Before you start

- Search existing issues before opening a new one.
- For a substantial change, open an issue first so the scope can be discussed.
- Do not include real browser profiles, cookies, history, saved logins, card data, or other private data in commits, screenshots, or bug reports.

## Development

Use the Windows build steps in [browser/README.md](browser/README.md). Keep `bun.lock` and `browser/host/Cargo.lock` in sync with dependency changes. Do not commit `node_modules`, Cargo `target` output, `dist` output, or local WebView2 profile data.

Before submitting a pull request, build the Windows app with `bun run host` and `bun run build` from `browser/`. Run `bun run test` when changing Rust address-bar behavior. In the pull request, summarize the change and include concise reproduction steps for bug fixes.

## Pull requests

- Keep each pull request focused and explain why the change is needed.
- Include screenshots for visible interface changes, using synthetic data only.
- Update the README when controls, requirements, or supported behavior change.
- Call out changes that affect how browser data is stored or filled.

By submitting a contribution, you agree that it will be distributed under the repository's MIT License.
