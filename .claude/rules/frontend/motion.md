---
paths:
  - "src/**/*.css"
  - "src/app/components/charts/**"
  - "src/app/lib/motion/**"
  - "src/app/widgets/**"
  - "src/app/hooks/use-*motion*"
  - "src/app/hooks/use-prefers-reduced-motion*"
---

# Motion Rules

Kelvo is a monitor, not a landing page, but it should feel alive (D-086). Motion does three
jobs: continuity (a value or chart moving to its next state), feedback (a control
responding), and choreography (a surface settling into place when it appears). There are
no scroll reveals and no decorative loops, and per-tick motion stays quiet.

## Tokens

Durations and easing come from the tokens in `src/app/styles/motion.css`, never literal
numbers in components. They are split by cost: per-tick tweens run every second a window
is visible, everything else runs once per interaction.

| Token | Value | Use for | Power saver |
|-------|-------|---------|-------------|
| `--motion-tick` | 150ms | Per-tick tweens: chart scroll, bar `scaleX`, ring dashoffset, core-tile alpha | off |
| `--motion-count` | 450ms | A headline number counting to its next value (`NumberTicker`, read from JS) | off |
| `--motion-fast` | 150ms | Hover/press feedback, toggles, segmented thumb, menus, sidebar pill | on |
| `--motion-entry` | 300ms | Hover glow, dialogs | on |
| `--motion-enter` | 280ms | One-shot entrance (lift) | on |
| `--motion-stagger` | 30ms | Delay step between entering items, index capped at 8 | on |
| `--motion-crossfade` | 150ms | Keyed state swap | on |
| `--ease-out` | `cubic-bezier(0.16,1,0.3,1)` | Everything except per-tick tweens | |
| `--ease-tick` | `cubic-bezier(0.33,1,0.68,1)` | Per-tick tweens | |

Every token is 0ms outside `prefers-reduced-motion: no-preference`, so reduced motion needs
no JS branch: a transition or animation with a 0ms duration is instant. Write transitions
and keyframe rules inside that media block anyway.

## Choreography (`src/app/lib/motion/`)

- `pageEnter` on the element hosting a route's `<Outlet />`: each child of the route's root
  (header, then each row) lifts in reading order. A route change mounts a fresh root, so
  navigation replays it. The dashboard shell applies it; pages get it for free.
- `data-stagger` on a container plus `stagger(i)` on its children (CardGrid) staggers them
  one by one, continuing the page's count.
- `enter(i, "lift" | "fade")` spread onto any element that should enter on mount
  (onboarding frame).
- `<Swap k={state}>` remounts small leaf content when a state changes and fades it in
  (status pill, pressure badge). Never wrap a canvas, a uPlot chart, a store subscriber or
  a `role="status"` region: remounting destroys them.

Rules for anything new:

- **Never `opacity: 0` in a base rule.** It lives only in a keyframe `from`, with
  `backwards` fill, so an animation that doesn't run leaves the element visible.
- Entrances run once per mount. 1 Hz re-renders must not remount an entering element (use
  stable keys), or it replays every second. A DOM move restarts a CSS animation too, so
  entering children keep a stable order (CardGrid items do).
- The app chrome (sidebar, title bar) doesn't animate in. Exits stay instant except overlay
  primitives (dialogs, menus), which mirror their entrance at `--motion-fast`.
- Animate layout (`grid-template-rows`, `width`) only with a reason written down; nothing
  in the library does today.

## Streaming charts

- Live charts scroll with a `translateX` on a group, then re-anchor; they never animate
  path `d` or re-layout per tick.
- All motion is off in Performance mode, which the user turns on or macOS Low Power Mode
  engages (D-088): Rust sets `data-performance` on the window root and every motion token
  drops to 0ms. Being on battery alone leaves motion on. Check the flag, don't guess from
  frame timing.
- A tween must finish before the next tick (under 1s). An animation still running when the
  next value lands produces visible lag behind the real number.
- Gaps (sleep, a missing series) are drawn as breaks. Never animate across a gap.
- Headline numbers (ring centres, stat strips) render through `<NumberTicker text unit?>`
  (D-087). It counts only when a value moved 10% or more or changed unit, in log space
  on the byte and rate ladders, and writes the span's text from rAF so a count never
  re-renders React. Every other number is replaced in place: no count-up from zero on
  mount, no digit rolls. Don't put `NumberTicker` in a table, list row or anything with more
  than a handful of instances per view; each count is main-thread text layout per frame.

## Reduced Motion

Handled by token zeroing in CSS; components don't branch on it. A JS-driven tween reads
its duration token from the root when it starts (`NumberTicker` reads `--motion-count`), so a
0ms token covers reduced motion and Performance mode alike. Only if JS needs the preference
itself, port opendata's `shared/hooks/use-prefers-reduced-motion.ts` to `src/app/hooks/`
and consume it there. Don't call `window.matchMedia` in components; you'll
miss the live toggle.

## Performance

The idle CPU budget (under 0.5% across the app and WebKit helpers) covers the backgrounded
app, menu bar only (D-088), and nothing above runs while a window is hidden. Visible motion
has no product budget but is regression-guarded. Foreground cost is measured before and after any change that adds
motion (`make bench` popover/overview scenarios, `perf-gate.spec.ts`).

- Animate `transform` and `opacity` only (plus `background-color` on core tiles).
- Don't add `will-change` by hand; it pins layers in memory in a long-lived panel.
- No infinite animations (`animate-pulse`, spinners) unless something is actually loading,
  and stop them when it isn't. A hidden-but-warm popover keeps running CSS animations.
- No springs or overshoot.
