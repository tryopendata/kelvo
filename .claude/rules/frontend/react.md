---
paths:
  - "src/app/**/*.ts"
  - "src/app/**/*.tsx"
  - "src/core/**/*.ts"
  - "src/main.tsx"
---

# Frontend Rules

React 19 + Vite frontend for Kelvo, rendered inside Tauri 2 webviews (popover panel,
dashboard window, onboarding, desktop board windows). There is no server and no SSR.

## Architecture

### Two-Layer Structure

```
src/app/                # React layer - uses ~/ alias
  routes/<route>/       # Window entry routes: popover, dashboard/*, onboarding, board/:display
    _components/        # Components local to one route
    _hooks/             # Hooks local to one route
    _lib/               # Pure helpers local to one route
  components/
    ui/                 # shadcn "new-york" primitives only
    charts/             # Chart primitives (SVG live charts, uPlot history)
  widgets/              # Widget registry + render-only widget components
  hooks/                # Shared hooks
  lib/                  # React utilities (cn(), providers)
    motion/             # Motion primitives (enter, pageEnter, Swap, NumberTicker); widgets may use them

src/core/               # Framework-agnostic TypeScript - uses @core/ alias
  generated/            # tauri-specta bindings (generated, never hand-edit)
  transport.ts          # The seam: Tauri Channel in the app, mock in browser/tests
  ...                   # Query keys, formatters, unit conversion, pure chart math
```

`src/core/` has zero React dependencies. Testable without rendering.

### Path Aliases

```typescript
import { commands } from "@core/generated/bindings"; // src/core/*
import { Button } from "~/components/ui/button"; // src/app/*
```

### Typed IPC

Rust types are the single source of truth. tauri-specta generates TS bindings for every
command, event and `kelvo-schema` type into `src/core/generated/`. Never hand-write a type
that mirrors a Rust type; regenerate instead (see `.claude/rules/generated-bindings.md`).

`seq` and timestamps cross the bridge as `i64` exported as `number`. They stay below 2^53;
don't add BigInt handling.

### Live state (1 Hz streaming)

- One scoped zustand store per host (`createStore` + context provider), keyed
  `hosts[hostId]` even though v1 has one host. No global singleton store.
- `core/transport.ts` feeds the store. In the app it wraps a Tauri Channel; in the browser
  dev server, Vitest and Playwright it is the mock transport. Components never call
  `@tauri-apps/api` directly.
- Consumers subscribe with narrow selectors so a 1 Hz tick re-renders only the cards whose
  slice changed. Selecting the whole store, or returning a fresh object/array from a selector
  without `useShallow`, re-renders every card every second.
- Rust decides when a window's channel stops. Don't rely on `visibilitychange` or timers in
  hidden webviews; WKWebView throttles them.
- Settings and layouts are owned by Rust. The frontend holds read-only mirrors updated by
  the `settings-changed` event and writes through commands.

### History

History goes through TanStack Query with centralised keys `(hostId, module, range, tier)`.
Never build a query key inline.

### Widgets are render-only

`src/app/widgets/**` components receive data and callbacks as props. Biome (`noRestrictedImports`
override in `biome.json`) forbids them from importing `@core/transport`, stores, `react-router` or `@tauri-apps/*`. The same
component renders in the popover, overview cards, board windows and the composer preview,
and its prop contract is what the v3 WidgetKit feed exposes. Fetch/subscribe in the route
or a hook, pass values down.

### Routes

React Router in data/library mode with memory history. Windows map to entry routes by
window label. No loaders that fetch, no `.server` files, no SSR.

## Rendering stability

- Stable keys from data identity (series key, pid), never array index for live lists.
- One flat keyed array for lists that reorder (process tables), not nested maps.
- `inert` on collapsed regions so hidden controls leave the tab order.

## Constraints

- Use semantic color tokens (`bg-background`) not hardcoded colors
- Files are kebab-case; named exports everywhere except route modules
- Co-locate unit tests with source files (`*.test.ts`)
- Keep `src/core/` free of React imports
- Check every UI change against `plan/design-system.md` and the existing screens, by screenshot
