---
description: Start the development environment
disable-model-invocation: true
---

## Full app (Tauri window + Rust)

```bash
bun run tauri dev
```

Vite serves on :1420 (fixed; Tauri expects it). Rust changes rebuild the app.

## Browser only, mock transport

For UI work that doesn't need real metrics: the Vite dev server in a browser, fed by the
mock transport. This is also what Playwright runs against.

```bash
bun run dev
```

Use `wait-for 1420` rather than sleeping before hitting it. Compare what you build against
`plan/design-system.md`, the screens already built, and the dev gallery (`/?route=/dev/gallery`).
