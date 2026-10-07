# Design system

Kelvo uses opendata's visual language ([`DESIGN.md`](https://github.com/tryopendata/opendata/blob/main/DESIGN.md)), adapted for a desktop app that redraws itself once a second. This doc says what carries over from opendata, what is new, and how each screen maps to a route, a window, a set of components and the version that ships it. This doc and the app as built are the visual reference (D-095).

Token values live in `src/app/styles/theme.css`. This doc refers to tokens by name and only repeats a value when the rule depends on it.

## Sources

| Source | Path | What it gives Kelvo |
|---|---|---|
| opendata design spec | [`DESIGN.md`](https://github.com/tryopendata/opendata/blob/main/DESIGN.md) | Surfaces, type rules, radius scale, motion (§10) |
| opendata tokens | [`shared/styles/theme.css`](https://github.com/tryopendata/opendata/blob/main/shared/styles/theme.css) | `@theme` structure, shadcn token names, dark custom variant |
| opendata card glow | [`shared/styles/tool-cards.css`](https://github.com/tryopendata/opendata/blob/main/shared/styles/tool-cards.css) (`.ask-accent`, `.ask-accent--quiet`, `.ask-accent-tile`) | The corner-glow card |
| opendata figures | [`shared/styles/effects.css`](https://github.com/tryopendata/opendata/blob/main/shared/styles/effects.css) (`.data-mono`) | Mono tabular figures |
| opendata anti-slop rules | [`.claude/skills/frontend-design-slop/SKILL.md`](https://github.com/tryopendata/opendata/blob/main/.claude/skills/frontend-design-slop/SKILL.md) | The review checklist at the end of this doc |

## Ported from opendata

| Item | opendata file | Kelvo file | Change on the way in |
|---|---|---|---|
| `@custom-variant dark (&:where(.dark, .dark *))` and the `@theme static` block | `shared/styles/theme.css` | `src/app/styles/theme.css` | Provider colors, `--od-*` aliases, Satoshi and Source Serif dropped. Kelvo tokens added |
| shadcn token names (`background`, `card`, `popover`, `primary`, `border`, `input`, `ring`, `muted-foreground`, `destructive`) | `shared/styles/theme.css` | same | Dark `muted-foreground` changes to `#8a8f98`, light `surface-raised` becomes `--color-raised: #e4e4e7` |
| `.ask-accent` corner glow, `--quiet` variant, accent tile | `shared/styles/tool-cards.css` | `src/app/styles/cards.css` as `.vt-card`, `.vt-card--chart` | Origin becomes a variable (`--o`), hover raises the tint to 22%, `@property --g` registered so the tint can transition |
| `.data-mono` | `shared/styles/effects.css` | `src/app/styles/effects.css` | Unchanged |
| Motion tokens (150 ms, 300 ms, `cubic-bezier(0.16,1,0.3,1)`) and the reduced-motion wrapper rule | `DESIGN.md` §10, `tool-cards.css` | `src/app/styles/motion.css` as `--motion-fast`, `--motion-entry`, `--ease-out` | The 500 ms word fade is not ported; Kelvo has no streaming prose |
| Radius scale (2, 4, 6, 8, 12, 22, full) | `DESIGN.md` §5 | `theme.css` `--radius-*` | 1 px added for dense heatmap cells |
| Type rules: Inter with `cv01` and `ss03`, weights 400/510/590, never 700 | `DESIGN.md` §3 | `theme.css`, `html` rule | Sizes follow Kelvo's app scale, not opendata's marketing scale |
| Luminance stepping for depth, no drop shadows on dark | `DESIGN.md` §6 | applies everywhere | Unchanged |
| Stable keys, one flat keyed array, `inert` on collapsed regions | `DESIGN.md` §10 | React rendering rules | Unchanged |

## Themes

Light and dark follow the system by default. Settings offers Match system, Light and Dark (Settings, General). Rust owns the setting; on start and on `settings-changed`, each window sets or removes the `.dark` class on `<html>`. With Match system, the window follows `prefers-color-scheme`. Light versions of every screen use the dashboard light token set with no layout changes.

## Vibrant surfaces

The popover and the desktop widgets (v2.1) sit over the wallpaper and use translucent surfaces. The original design mocks drew this with CSS `backdrop-filter: blur(40px) saturate(180%)`. That cannot work as written in the app: a webview's backdrop filter only sees the webview's own content, not the desktop behind the window.

The implementation is two layers:

1. A native material behind a transparent webview. The popover panel uses an `NSVisualEffectView` with the popover or HUD material, set through Tauri's `windowEffects` config or the `window-vibrancy` crate (unverified which works with `tauri-nspanel`; check in v1.0 phase 3). Transparent windows require `macOSPrivateApi: true` (unverified for current Tauri 2.x).
2. A CSS tint on top: `--color-vibrant` on the panel root and `--color-card-vibrant` on each card, inside a `.surface-vibrant` scope that also swaps `--color-border`, `--color-border-strong` and `--color-track` for the translucent values in `theme.css`.

Desktop widgets follow the same split with `--color-widget`, and with `--color-widget-editing` for the widget being edited.

When Reduce Transparency is on, the native material is removed and the tokens switch to the opaque fallbacks (`#161619` and `#111113` in dark, `#f6f6f8` and `#ffffff` in light). Rust reads `NSWorkspace.accessibilityDisplayShouldReduceTransparency`, listens for the accessibility display options notification, and sets `data-reduce-transparency` on the root of every vibrant window. WebKit may also support the `prefers-reduced-transparency` media query (unverified), but the Rust-provided attribute is the source of truth because it also controls the native material.

## Module accents

Each module has one accent, used for its sidebar dot, card glow, legend swatches and chart marks. Cyan is both the CPU accent and the CTA color, which is a confirmed decision (see `decisions.md`).

| Module | Token | Dark and light fill | Light ink |
|---|---|---|---|
| CPU | `--color-cpu` | `#06b6d4` | `#0891b2` |
| CPU second series (E-cluster) | `--color-cpu-2` | `#67e8f9` | `#0e7490` |
| GPU | `--color-gpu` | `#f472b6` | `#db2777` |
| Memory | `--color-mem` | `#a78bfa` | `#7c3aed` |
| Power & Sensors | `--color-power` | `#fbbf24` | `#d97706` |
| Temperature (within Power & Sensors) | `--color-temp` | `#f59e0b` | `#b45309` |
| Network | `--color-net` | `#34d399` | `#059669` |
| Disk | `--color-disk` | `#60a5fa` | `#2563eb` |
| Battery | `--color-battery` | `#a3e635` | `#4d7c0f` |

### Ramp rule

Series inside one card are a ramp of that card's accent, never a second hue. The four steps are `--ramp-1` 100%, `--ramp-2` 60%, `--ramp-3` 35% and `--ramp-4` 20%, applied as `color-mix(in srgb, var(--accent) N%, transparent)`. Order follows importance: the series the card's headline number describes gets step 1. A series whose value is often zero (ANE power, compressed memory) gets a diagonal hatch in the accent so a zero-width slice still has a legend swatch that matches. The two exceptions in the mocks are kept: the CPU E-cluster uses `--color-cpu-2` because the residency and frequency rings sit side by side and need to be told apart at a glance, and temperature uses `--color-temp` inside the amber Power card.

### Light-mode ink

Every accent fails 3:1 against a white card (1.5:1 for battery lime, 2.4:1 for cyan). In light mode, lines, text, 1 to 2 px strokes, tick marks and ring arcs under 4 px use the ink column. Area fills, bars at least 4 px tall and heatmap cells keep the fill hex, because they read as shapes and each carries a text value or a tooltip. The original light mocks used the fill hex everywhere; this is a deliberate deviation for accessibility.

### Cyan as both CPU and CTA

On the CPU page almost everything is cyan, so the CTA must be recognizable by shape, not hue. Primary CTAs are always filled buttons with dark text (`--color-primary-foreground`, 8.2:1). CPU marks are strokes, fills and cells, never filled rounded rectangles with text. Cyan text links appear only as `--color-link` and only where a sentence needs an action ("Share sensor dump" in the unknown-chip notice).

## Status colors

| State | Token | Shape that must accompany it | Text that must accompany it |
|---|---|---|---|
| Live, sampling, OK | `--color-live` `#10b981` | 6 px dot | "Live", "1s", "Sampling every 1s" |
| Paused | `--color-fg-faint` | Pause glyph | "Paused" |
| Warning (thermal state Serious, memory pressure Warn, sustained hot process) | `--color-warning` `#f59e0b` | Triangle icon | State name, for example "Thermal: serious" |
| Critical (thermal state Critical, memory pressure Critical) | `--color-destructive` | Octagon icon | State name |
| Not available, unsupported | `--color-muted-foreground` | Info circle | Explanation sentence |

### The amber conflict

The Power accent is `#fbbf24`, the temperature series is `#f59e0b`, and opendata's warning color is also `#f59e0b`. On the Power & Sensors page a warning state would be indistinguishable from ordinary data. The rules:

- Warning and critical are never conveyed by color alone. Each carries its icon and its state word.
- Temperatures are never colored by value. There is no green-to-red ramp on zones or rings; a hot zone is the top row of a table sorted hottest first (Power & Sensors), with its number.
- Warnings render as a chip in the page header or card header, not by recoloring the data marks.
- Critical uses red, which does not collide with any module accent.

## Typography

Inter Variable with `cv01` and `ss03` on every element. Root weight 510. Figures use `.data-mono` (JetBrains Mono, `tabular-nums`) so digits do not shift as values update each second. Nothing outside this scale should appear.

| Role | Family | Size | Weight | Tracking | Example in mocks |
|---|---|---|---|---|---|
| Onboarding title | Inter | 28 px | 590 | -0.022em | "Set up Kelvo" (11) |
| Page title | Inter | 22 px | 590 | -0.022em | "Overview", "CPU", "Timeline" (04, 06, 07, 08, 13) |
| Machine name | Inter | 20 px | 590 | -0.022em | "MacBook Pro 14-inch (M4 Pro, 2024)" (04) |
| Hero figure | Mono | 32 px | 400 | -0.022em | CPU total "18%" (07) |
| Large figure | Mono | 16 to 18 px | 400 | -0.01em to -0.02em | Lane "now" values (06), cluster GHz rings (07), network rates in popover (02) |
| Card figure | Mono | 15 px | 400 | normal | Popover card headline values (14), CPU stat strip (07) |
| Ring center value | Mono | 14 px (Overview), 17 px (Power rings) | 400 | -0.02em | "18%", "14.8" (04, 08) |
| Composer title, popover app title | Inter | 16 px, 13 px | 590 | -0.01em | "Widgets" (09), "Kelvo" (14) |
| Card title | Inter | 13 px (dashboard), 12 px (popover) | 590 | normal | "CPU", "Per-core load, last 10 minutes" |
| Body and row text | Inter | 12 to 13 px | 400 or 510 | normal | Process names, settings rows |
| Secondary text | Inter | 11 to 12 px | 400 | normal | Card subtitles ("10P + 4E"), page subtitles, `--color-muted-foreground` |
| Field label | Mono | 10 px | 400 | 0.08em, uppercase | "P-CLUSTER", "FREQ", table headers, "MODULE" |
| Axis tick, chart corner label | Mono | 9 to 10 px | 400 | normal | "-60s", "now", "40%", heatmap hour labels |
| Initials chip | Mono | 9 px | 400 | normal | Process initial in a 16 px square, 4 px radius, `--color-raised` |

Uppercase appears only on field labels (column headers, KPI labels). It never labels a section; section titles are sentence case.

## Layout and spacing

| Element | Value from mocks |
|---|---|
| Dashboard window | 1280 px wide by default; sidebar 220 px; main padding 20 px top, 24 px sides |
| Grid gap between cards | 16 px |
| Card padding | 14 to 16 px; popover cards 12 px |
| Gap inside a card | 12 px (dashboard), 8 px (popover) |
| Popover panel | 360 × 680 px, 22 px radius, cards inset 10 px, 8 px apart |
| Sidebar row | 30 px tall, 8 px horizontal padding, 6 px radius, 10 px icon gap, three groups separated by `--color-border-subtle` |
| Controls | Icon buttons 28 px square; pills 22 to 24 px tall; buttons 28 px tall |
| Overview grid | Three columns of module cards under a full-width machine header |

## Card anatomy

Every card is a `<section>` with an accessible name, `--radius-card`, a 1 px `--color-border`, and the corner glow on `--color-card` (or `--color-card-vibrant` in vibrant scopes).

```css
.vt-card {
  --a: var(--color-cpu);  /* module accent */
  --g: 9%;                /* tint */
  --o: 0% 0%;             /* glow origin */
  background:
    radial-gradient(ellipse 80% 100% at var(--o),
      color-mix(in srgb, var(--a) var(--g), transparent) 0%, transparent 60%),
    var(--color-card);
}
.vt-card--chart { --g: 6%; }   /* chart cards and all popover cards */
```

Hover raises `--g` to 22% and the border to `--color-border-strong` over 300 ms, inside `prefers-reduced-motion: no-preference`. Hover applies only to cards that are a link or open something (Overview module cards open their module page). Non-interactive cards do not react to hover.

### Rotating origins

Each Overview card has an `origin`: CPU `0% 0%`, GPU `100% 0%`, Memory `0% 100%`, Power `100% 100%`, Network `0% 0%`, Disk `100% 0%`. The CPU and Power & Sensors pages set `--o` per section the same way. The rule is that adjacent cards light different corners so the page does not get a uniform wash in one direction. The `CardGrid` component assigns origins by grid index, cycling top-left, top-right, bottom-left, bottom-right, and a card can override it. The machine header uses `100% 0%` at 6%. Popover cards all use `0% 0%` at 6%, because they are stacked in one column and the glow reads as a consistent light source.

### Card header

Left: an 8 px square swatch in the module accent (2 px radius), then the title at card-title size. Right: either the headline value in mono or a muted subtitle (the Overview uses "10P + 4E", "20-core"). One of the two, not both. Popover cards put the headline value right.

## Motion

Motion tells the reader what changed, and lets a surface settle into place when it appears. On a surface that rewrites itself every second, most changes are obvious without animation, so per-tick updates stay quiet; one-shot moments (a page opening, a control pressed, a state flipping) get the craft (D-086). These rules adapt opendata `DESIGN.md` §10 and its landing-page lift roles to 1 Hz streaming. Tokens and primitives are in `.claude/rules/frontend/motion.md`.

| Change | Treatment | Duration |
|---|---|---|
| A new sample on a streaming chart | Append the point and scroll the whole path with `transform: translateX()` on a wrapping `<g>`. The path `d` is rewritten once per tick, never tweened | Scroll eases over 150 ms (`--ease-tick`), or jumps when motion is reduced |
| A headline number changes (ring centre, stat-strip figures) | Counts to the new value when it moved 10% or more or changed unit: "10 GB" counts down through the megabytes to "100 MB". Smaller moves, other numbers and a change to or from "—" are replaced in place. No per-digit roll, never a count up from zero on mount. Mono tabular figures keep width stable | 450 ms, ease-out cubic; instant in Performance mode (D-087, D-088) |
| A value changes state (paused to live, pressure turning critical) | The glyph and word remount and fade in | 150 ms |
| A bar, ring or core tile changes value | `transform: scaleX()` on the bar fill, `stroke-dashoffset` on the ring, background alpha on tiles | 150 ms, `--ease-tick` |
| A dashboard page opens (navigation, first open) | The header, then each row, fades in and rises 3 px; Overview's cards one by one. 30 ms apart, capped at 8 steps | 280 ms each, once per mount |
| An onboarding step opens | Title, body, footer lift in the same way | 280 ms |
| Sidebar selection | One pill slides to the new row (it used to be a fill on each row) | 150 ms |
| Pressing a control | Buttons darken one step; segmented options and the switch thumb squeeze slightly | 150 ms |
| Dialog, menu, tooltip | Fade plus a 0.97 zoom; dialogs also rise 4 px. Exits mirror at 150 ms | 300 ms dialog, 150 ms others |
| Stale stream | The page dims to 50% with a fade | 150 ms |
| Card hover | `--g` and border color | 300 ms |
| Segmented control | Thumb slides | 150 ms |
| Tray icon | No tween; see the tray section | n/a |

The sidebar and title bar never animate in; route changes and window hides have no exit animation.

Path tweening is banned because interpolating between two `d` strings of different length produces wrong intermediate shapes and costs a layout pass per frame. The 150 ms scroll is a translate on a composited layer, which is cheap.

Animation runs only on visible surfaces. Rust stops a window's live channel when the window is hidden, minimized, occluded or the display sleeps, so nothing renders off screen. In Performance mode, which the user turns on in Settings or macOS Low Power Mode engages, all motion is off: Rust sets a `data-performance` attribute on each window root and every motion token drops to 0 ms (D-088). Being on battery alone changes nothing about motion. When Low Power Mode engaged the mode, the sidebar footer and the popover's interval pill say so, with a hovercard explaining why.

Every transition and keyframe lives inside `@media (prefers-reduced-motion: no-preference)`, as opendata requires. No `opacity: 0` in base rules. There is no JS branch on reduced motion.

## Chart rules

These apply to every live and history chart, in every surface.

| Rule | Detail |
|---|---|
| Gridlines | 1 px `--color-grid`, `vector-effect: non-scaling-stroke`. Horizontal only, at the tick values. No vertical gridlines on streaming charts |
| Baseline | 1 px `--color-axis` at y = 0 |
| Axis labels | Mono 9 to 10 px, `--color-fg-faint` for repeat labels (ticks) and `--color-muted-foreground` when the label is the only statement of scale. Right edge labelled "now" on any chart that ends at the present |
| Y scale | Fixed domain for bounded metrics (0 to 100% for load, 20 to 110 °C for temperature). Autoscaled metrics (network, power, popover CPU at 40%) snap the ceiling to a nice step and only shrink after 60 s below the next step down, so the scale does not jitter each tick. The current ceiling is always labelled (the "40%" corner label on the popover CPU chart) |
| Lines and areas | 1.5 px line in the accent (ink in light), area fill a vertical gradient of the accent from 40% to 0 |
| Figures | Every number drawn in or next to a chart uses `.data-mono` |
| Gaps | A missing sample, a sleep period or a series that disappeared is a gap. Gaps are drawn as a hatched band (`repeating-linear-gradient(135deg, …)` in `--color-grid`, 1 px stripes at 7 px) with a label ("Asleep 11:02 to 11:31, not interpolated"). The line breaks at the gap: no segment, no area, no interpolation across it. A hollow dot marks each edge |
| Min/max envelopes | History charts at a zoom where one pixel covers several buckets draw the avg line plus a 20% min/max band, so a 1 s spike is not hidden by averaging |
| Color is never the only encoding | Every series has a text label next to it or in a legend with its value. Mirrored network bars label up and down with arrows and words. Stacked power uses position plus a hatch for ANE. Heatmap cells carry a tooltip and an accessible name with the value |
| Crosshair | One vertical line synced across all lanes, with a dot on each series and a single tooltip listing every lane's value plus the top processes at that moment |
| Annotations | Pills at the top of the plot area. They must not overlap: when two collide, later ones move to a second row or merge into a "+N" pill. The original Timeline mock showed the overlap this rule prevents (the "14:02 Fans ramped up" pill covers the "06:55 Wake" and "10:12 Docker" markers) |

## Accessibility

- Every chart is `role="img"` with an `aria-label` that states what it shows and the current value, for example ("CPU, last 60 seconds", "Upload above the line, download below, last 48 seconds"). The label is updated at most every 10 s so screen readers are not flooded.
- Each Timeline lane has a "Show as table" alternative: a `<table>` of bucket time, min, avg, max for the visible range.
- Up and down, P and E, and segment names are always text, not only color or position.
- The status item carries an accessibility label with all values it shows, for example ("Kelvo: CPU 18%, GPU 36%, memory 42%, 61 degrees").
- The 30-day heatmap is a `role="grid"` with arrow-key navigation, and each cell's name is its date, hour and value or "no samples".
- Focus rings use `--color-ring` at 2 px with 2 px offset, on every interactive element including heatmap cells and table headers.
- Text contrast: `--color-muted-foreground` passes 4.5:1 in both themes; `--color-fg-faint` does not and is only used for non-essential repeat labels and disabled controls.
- Reduce Motion, Reduce Transparency and Increase Contrast are honored. Increase Contrast swaps `--color-border` for `--color-border-strong` and `--color-fg-faint` for `--color-muted-foreground`.
- Windows support full keyboard navigation of the sidebar (arrow keys, Enter) and ⌘1 to ⌘9 for the first nine sidebar entries (proposed, not in mocks).

## Tray icon spec

The tray image is rendered in Rust (`tiny-skia` for shapes, `ab_glyph` for text) as an `NSImage` template, so macOS tints it for light, dark and selected menu bars. All glyphs are monochrome; values are encoded by height and text.

| Style | Ships | Content | Geometry |
|---|---|---|---|
| Combined | v1.0 (default) | Three bars (CPU, GPU, memory pressure) plus the hottest SoC zone temperature in text | 17 × 14 pt icon; bars 3 pt wide with a 4 pt gap; unfilled track at 30% opacity; 1 pt corner radius; temperature in mono after the bars ("61°") |
| Values | v1.0 | Stacked three-letter vertical labels (CPU, GPU, MEM, SOC, PWR) beside mono values ("18%", "36%", "42%", "61°", "14.8W") | Labels at 6 to 6.5 px in a column 1 character wide; values at the menu bar text size |
| Graphs | v1.1 | CPU line sparkline, memory fill gauge with value, network up and down rates stacked | Sparkline 20 samples wide; rates in two lines at 8 px |
| Cores + histogram | v1.1 | Per-core strip (P cores, a 3 pt gap, then E cores; 2 pt bars, 1 pt gap, 16 pt tall) plus a GPU usage histogram of the last 9 samples | As in the content column |

Redraw runs at most every 2 s, or 4 s when backed off on battery, doubled again in Performance mode (D-077, D-088), and stops during display sleep. Values are quantized (bars to the pixel, temperature to 1 °C, percentages to 1%) and the rendered frame is hashed; an unchanged frame is not sent to the status item. The original tray mock specified a "150 ms tween". Kelvo does not tween the tray icon: a tween means 5 to 10 extra image sets per second, each of which costs WindowServer compositing, and the WindowServer budget is under 0.2% CPU. The bars jump to the new value. This deviation is recorded in the risks section of `v1-local-monitor.md`.

The status item's highlighted state (a 22 pt tall pill with 5 pt radius) is native and needs no drawing.

## Anti-slop rules

From opendata's `frontend-design-slop` skill, applied to Kelvo.

- No section kickers. Field labels in mono uppercase are fine (column headers, KPI labels, the "MODULES" and "MENU BAR STYLE" control-group labels in onboarding). A label above a heading that restates it is not.
- No subtitle that restates the title. Page subtitles carry facts: "Apple M4 Pro · 10 performance + 4 efficiency cores", "On battery · 14.8 W system draw · 6:12 remaining".
- The 25% zoom test applies to every page. Each page has one dominant region: the stacked lanes on Timeline, the total chart on CPU, the module grid on Overview.
- The Overview's six identical module cards are a real grid of comparable modules, which is the legitimate case. Do not add a seventh card of a different kind to fill a row.
- The corner glow is house style. No other glow: no `box-shadow` halos, no glowing dots. The live dot is a flat 6 px circle, and it does not pulse.
- No accent dot before labels except the module swatch in a card header or legend, where it is the legend key.
- No icons in tinted circles above titles. Sidebar icons stay because they disambiguate entries at a glance.
- No count-up from zero, no hover scale, no springs. Headline numbers count only between two real readings (D-087).
- No marketing words in UI copy. Empty states say what is happening and how long it will take ("First 4 min recorded. History is kept for 30 days on this Mac only.").
- One accent job per view. On a module page the module accent is for data; cyan CTAs appear only in the popover footer, onboarding and Settings actions.

## Screen map

| Screen | Route or window | Components it needs | Ships in |
|---|---|---|---|
| Menu bar strip | Native status item (Rust-rendered); `TrayPreview` in onboarding and Settings | Rust `tray::render`, `TrayPreview` | v1.0 combined and values; v1.1 graphs, cores + histogram, per-module items |
| Popover, dark | `popover` window, route `/popover` | `PopoverShell`, `PopoverHeader`, `StatusPill`, `IconButton`, `ModuleCard` (popover variant), `StreamArea`, `InlineBar`, `CoreTiles`, `StackBar`, `Legend`, `StatGrid`, `MirrorBars`, `PopoverFooter`, `SelfCpuReadout` | v1.0 |
| Popover, light | same, light | same | v1.0 |
| Overview, dark | `dashboard` window, route `/dashboard/overview` | `DashboardShell`, `Sidebar`, `PageHeader`, `StatusPill`, `MachineHeader`, `CardGrid`, `MetricCard`, `RingGauge`, `InlineBar`, `Legend`, `ProcessList`, `InitialChip` | v1.0. GPU and Network process lists v1.2. "Customize" button v2.3 |
| Overview, light | same, light | same | v1.0 |
| Timeline | `/dashboard/timeline` | `PageHeader`, `SegmentedControl`, `RangeNav`, `TimelineLanes`, `TimelineLane`, `Crosshair`, `CrosshairTooltip`, `GapBand`, `AnnotationRow`, `CalendarHeatmap`, `ExportButton` | v1.0 1h and 24h with sleep bands; v1.1 7d, 30d, heatmap, Export CSV; v1.2 event annotations |
| CPU page | `/dashboard/cpu` | `PageHeader`, `SegmentedControl`, `StatStrip`, `StreamArea` (large), `ClusterFreqRing`, `CoreHeatmap`, `ResidencyBar`, `ProcessTable` | v1.0 |
| Power & Sensors page | `/dashboard/power`; battery section reused at `/dashboard/battery` | `RingStatCard`, `ZoneTable`, `PowerStack`, `ChartAnnotation`, `BatteryHistoryBars`, `StatStrip` | v1.0 |
| Widget composer | `/dashboard/widgets` | `ComposerTabs`, `WidgetInspector`, `WidgetLibrary`, `LibraryTile`, `ComposerPreview`, `DropIndicator`, `DragGhost`, `SegmentedControl`, `Switch` | v2.0 popover tab; v2.1 desktop widget tabs |
| Desktop widgets | `board-<displayId>` window, route `/board/:display` | `BoardCanvas`, `WidgetFrame`, `SnapGuides`, `ResizeHandles`, `SizeReadout`, `EditPill`, widget components (`CoreHeatmap`, `MirrorBars`, `RingGauge`, `StackBar`) | v2.1 |
| Onboarding | `onboarding` window, route `/onboarding` | `OnboardingShell`, `ModuleToggleList`, `TrayStyleOption`, `TrayPreview`, `ChipStatus`, `Checkbox` | v1.0 (combined and values options); v1.1 adds the graph-per-module option |
| Empty, sleep gap and unsupported states | States inside `/dashboard/timeline`, module pages and Overview | `CollectingOverlay`, `GapBand`, `UnsupportedNotice`, `MiniModuleCard` | v1.0 |
| Settings | `/dashboard/settings` | `SettingsSection`, `SettingsRow`, `ModuleSettingsTable`, `Select`, `Switch`, `SegmentedControl`, `HistorySizeRow` | v1.0 (menu bar modes Combined, Value + label, Hidden); v1.1 per-module item modes |
| Popover panel | Component used by `/popover` | as the popover | v1.0 |
| Dashboard sidebar | Component used by every `/dashboard/*` route | `Sidebar`, `SidebarItem`, `SidebarFooter` | v1.0 (Widgets entry hidden until v2.0) |
| GPU page | `/dashboard/gpu` | `StatStrip`, `StreamArea`, `RingStatCard`, `InlineBar`, `ProcessTable` (v1.2) | v1.0 |
| Memory page | `/dashboard/memory` | `StatStrip`, `StackBar`, `StreamArea`, `Legend`, `ProcessTable` | v1.0 |
| Network page | `/dashboard/network` | `StatStrip`, `MirrorBars`, `InterfaceTable`, `ProcessTable` (v1.2) | v1.0 |
| Disk page | `/dashboard/disk` | `StatStrip`, `MirrorBars`, `VolumeTable`, `ProcessTable` | v1.0 |
| Battery page | `/dashboard/battery` | `RingStatCard`, `BatteryHistoryBars`, `StatStrip` | v1.0 |
| Processes page | `/dashboard/processes` | `ProcessTable` (full), `SearchField`, `SegmentedControl`, `ProcessActions` (row action and context menu with Quit and Force Quit), `Dialog`, toast | v1.0 |
| WidgetKit widgets | SwiftUI extension `native/KelvoWidgets` | SwiftUI views reading the widget feed | v2.2 dev build; v3 signed public build |
| Alerts editor | `/dashboard/alerts` | `RuleList`, `RuleEditor` | v2.3 (first built-in rule in v1.2 has no editor) |
| Hosts fleet view | `/dashboard/hosts` | `HostCard`, `Sparkline`, `HostSwitcher` | v4.2 |
| Add host flow | `add-host` sheet | `HostPicker`, `InstallProgress` | v4.0 |

## Component inventory

Components under `src/app/widgets/` are render-only. They take plain props and callbacks, and Biome forbids them from importing transport, stores, router or Tauri APIs. Their data props are JSON-serializable: numbers, strings, booleans, `(number | null)[]` series with `null` for gaps, and millisecond epoch timestamps. No `Date`, `Map`, typed arrays or class instances. That keeps their data contract identical to the widget feed JSON that v2.2's SwiftUI widgets read. The "Widget-safe" column marks those components.

| Component | Description | Widget-safe | Props sketch |
|---|---|---|---|
| `Card` | Glow card shell | yes | `{ accent: ModuleId; origin?: Corner; variant?: 'default' \| 'chart'; labelledBy: string; children }` |
| `CardGrid` | Grid that assigns rotating glow origins | no | `{ columns: number; children }` |
| `MetricCard` | Overview module card: header, ring, two bars, legend, top-5 list | yes | `{ module; title; subtitle; ring: RingGaugeProps; bars: InlineBarProps[2]; legend: LegendItem[]; list: ProcessRow[] \| InterfaceRow[]; href }` |
| `ModuleCard` | Popover module card: title, headline value, body slot | yes | `{ module; title; value: string; unit?: string; children }` |
| `RingGauge` | Arc gauge with one or two segments and a centered value | yes | `{ fractions: [number, number?]; value: string; label: string; accent; size: 64 \| 80 \| 104 }` |
| `ClusterFreqRing` | RingGauge variant: GHz value, max, active % below | yes | `{ cluster: 'P' \| 'E'; ghz: number; maxGhz: number; activePct: number; accent }` |
| `RingStatCard` | Ring plus title, description and one key/value | yes | `{ title; description; ring: RingGaugeProps; kv: { label; value } }` |
| `InlineBar` | Label, value, horizontal bar | yes | `{ label; value: string; fraction: number; accent; rampStep?: 1..4 }` |
| `StackBar` | Composition bar with legend | yes | `{ segments: { key; label; value: string; fraction; step: 1..4 \| 'hatch' }[]; accent; showLegend }` |
| `Legend` | Swatch, label, value rows | yes | `{ items: { label; value: string; step: 1..4 \| 'hatch' }[]; accent; columns: 1 \| 2 }` |
| `StatGrid` | Two or three mono KPI cells with field labels | yes | `{ items: { label; value: string; unit?: string }[] }` |
| `StatStrip` | Page-level KPI strip above a chart | yes | `{ hero?: { value; unit }; items: { label; value; muted?: boolean; swatch?: step }[] }` |
| `StreamArea` | Live area or line chart, scrolls with translateX | yes | `{ series: { values: (number \| null)[]; step: 1..4 }[]; tEndMs: number; intervalMs: number; yMax: number \| 'auto'; accent; height; ariaLabel }` |
| `MirrorBars` | Up bars above a baseline, down below | yes | `{ up: (number \| null)[]; down: (number \| null)[]; intervalMs; accent; ariaLabel }` |
| `CoreTiles` | Per-core load tiles grouped by cluster | yes | `{ clusters: { name: string; freqGhz: number; cores: { id: string; load: number }[] }[] }` |
| `CoreHeatmap` | Per-core load over time, one row per core | yes | `{ cores: { id; cluster: 'P' \| 'E'; now: number; buckets: (number \| null)[] }[]; bucketMs: number; windowMs: number }` |
| `ResidencyBar` | Cluster frequency residency as a stacked bar plus table | yes | `{ cluster; activePct; states: { label: string; pct: number }[] }` |
| `ZoneTable` | Sensor table, hottest first, with bar and 10-minute range | no | `{ rows: { name; key; now: number; min10: number; max10: number }[]; extras: { label; value: number }[]; unit: 'C' \| 'F' }` |
| `PowerStack` | Stacked power area by component, last 10 minutes | yes | `{ series: { key: 'cpu' \| 'gpu' \| 'ane' \| 'dram'; values: (number \| null)[] }[]; intervalMs; annotations: ChartAnnotation[] }` |
| `BatteryHistoryBars` | Hourly charge bars with charging marks | yes | `{ hours: { tsMs: number; charge: number \| null; charging: boolean }[]; annotations: ChartAnnotation[] }` |
| `ChartAnnotation` | Labelled marker line inside a chart | yes | `{ tsMs; label }` |
| `ProcessList` | Compact top-5: initial chip, name, value | yes | `{ rows: { initial; name; value: string }[] }` |
| `ProcessTable` | Sortable, virtualized process table | no | `{ rows: ProcessRow[]; columns: ColumnId[]; sort: { by; dir }; onSort; onSelect? }` |
| `InterfaceTable`, `VolumeTable` | Network interfaces and disk volumes | no | `{ rows }` |
| `InitialChip` | 16 px mono initial for a process | yes | `{ text: string }` |
| `TimelineLanes` | Lane stack with shared x axis, crosshair and gap bands | no | `{ range: { fromMs; toMs }; tier; lanes: TimelineLaneProps[]; gaps: { fromMs; toMs; kind }[]; annotations; onCursor }` |
| `TimelineLane` | One metric's line and area with label column | no | `{ metric; label; nowValue; sub; color; points: { tMs; min; avg; max }[]; domain; mirrored?: boolean }` |
| `CrosshairTooltip` | Values at cursor plus top processes then | no | `{ tMs; resolutionLabel; rows: { label; value; color }[]; processes: { initial; name; value }[] }` |
| `GapBand` | Hatched band with label | yes | `{ fromMs; toMs; label }` |
| `AnnotationRow` | Non-overlapping annotation pills | no | `{ items: { tMs; kind: 'sleep' \| 'wake' \| 'event'; label }[] }` |
| `CalendarHeatmap` | 30 days × 24 hours grid | no | `{ days: { dateIso; hours: (number \| null)[] }[]; metric: 'cpu' \| 'temp'; vmax; selected?: { dateIso; hour }; onSelect }` |
| `SegmentedControl` | Radix ToggleGroup styled as pill track | no | `{ options: { value; label }[]; value; onChange; size: 'sm' \| 'md'; ariaLabel }` |
| `StatusPill` | Dot plus short label | yes | `{ state: 'live' \| 'paused' \| 'stale'; label }` |
| `Sidebar`, `SidebarItem` | Dashboard navigation with live values | no | `{ groups: { items: { route; label; icon; value?: string }[] }[]; active }` |
| `MachineHeader` | Illustration plus spec grid | no | `{ hostInfo: HostInfo; uptimeMs; lastWakeMs }` |
| `PopoverHeader`, `PopoverFooter` | App title, interval pill, pause, settings; CTA row and self-CPU readout | no | `{ hostName; uptimeMs; intervalMs; paused; onPause; onSettings }`, `{ selfCpuPct; onOpenDashboard; onActivity }` |
| `TrayPreview` | Simulated menu bar showing a tray style | no | `{ style: TrayStyle; values: TrayValues; theme }` |
| `TrayStyleOption` | Selectable card with TrayPreview | no | `{ style; title; description; recommended?: boolean; selected; onSelect }` |
| `ModuleToggleList` | Module rows with swatch, description, switch | no | `{ modules: { id; label; description; enabled; available }[]; onToggle }` |
| `SettingsRow` | Label, optional sub, control slot | no | `{ label; sub?: string; children }` |
| `CollectingOverlay` | Empty-history notice over lanes | no | `{ startedMs; recordedMs; retentionDays }` |
| `UnsupportedNotice` | Unknown-chip explanation with dump action | no | `{ modelId; hiddenModules: ModuleId[]; onShareDump }` |
| `WidgetInspector`, `WidgetLibrary`, `ComposerPreview` | Composer panes | no | Defined in v2.0 against the widget manifest |
| `WidgetFrame`, `SnapGuides`, `ResizeHandles`, `SizeReadout`, `EditPill` | Desktop widget edit mode | no | Defined in v2.1 |
