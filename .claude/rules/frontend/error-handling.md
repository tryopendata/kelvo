---
paths:
  - "src/app/routes/**/*.tsx"
  - "src/app/components/**/*.tsx"
  - "src/app/lib/**/*.ts"
  - "src/core/**/*.ts"
---

# Frontend Error Handling

## Expected errors

tauri-specta commands return `Result`-shaped values (`{ status: "ok", data } | { status: "error", error }`).
Branch on `status`; don't wrap them in try/catch and don't throw the error away. Map
known error variants to UI states in `src/core/`, not in components.

## Missing data is not an error

- A capability the host never had renders the "not available" state.
- A series that was present and then disappears is drawn as a gap, never interpolated.
- A collector that returned no value for a tick is a gap, not zero.

Don't coerce `undefined`/`null` to `0` to make a chart draw. A zero reads as a real
measurement.

## Channel and transport failures

A dropped Tauri Channel or a WebContent process reload must surface as a visible
"reconnecting" state and resubscribe with a ring-buffer backfill. Never fail silently
with a frozen last value: a monitor that stops updating without saying so is the worst
failure mode this app has.

## Unexpected errors

Route-level error boundary catches render errors and shows a reload action. Log with
enough context (hostId, module, window label) to reproduce. There is no remote error
reporting; don't add one without asking.
