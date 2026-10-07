# v2: Customization and desktop widgets

This doc covers v2.0 through v2.3. It turns widgets from hard-coded components into data, lets people compose the popover and desktop boards themselves, adds a WidgetKit build for personal use, and finishes with an alert rules editor and a configurable Overview.

Read [architecture.md](architecture.md) first for the series model, bus, store tiers and the widget boundary rule. [design-system.md](design-system.md) holds the tokens and component inventory the widgets reuse.

## Goal and user value

v1 answers "what is my Mac doing and what did it do". v2 lets people decide where that information lives. Someone who only cares about memory pressure and the hottest SoC zone can make the popover show exactly that. Someone with a second display can park a CPU heatmap and a network graph on the desktop and leave them there, without paying for animation nobody is looking at. Someone who prefers native macOS widgets can build Kelvo from source with a WidgetKit extension and add Kelvo to the widget gallery on their own Mac.

The engineering value is that widget definitions become data. One manifest drives the composer library, the popover, desktop boards, the Overview, and the WidgetKit kinds and their Swift types.

## Problem and JTBD

v1's popover and Overview are fixed. Every user gets the same seven cards in the same order, and the only lever is turning modules off in Settings, which also stops sampling them. The target is a composer with a library, an inspector and a live preview, plus floating widgets on the wallpaper.

| Job | When | What they need from v2 |
|---|---|---|
| Glance | "I want the two numbers I care about one click away" | A popover built from the widgets they picked, in their order, with their titles |
| Ambient display | "I want to see load without opening anything" | Desktop widgets that stay put per display, cost nothing when covered, and respect Reduce Transparency |
| Ambient display, native | "I want Kelvo in the macOS widget gallery next to the clock" | A WidgetKit extension they can build and sign locally with a free Apple account |
| Look back | "Show me the last 24 hours in this widget, not just the last minute" | Per-widget history windows backed by the persisted tiers |
| Be told | "Tell me when memory pressure stays high, not only about hot processes" | An alert rules editor with more rule kinds than v1.2's single rule |
| Glance, dashboard | "The Overview shows Disk but I never look at it" | A Customize mode for the Overview cards |

## Scope

### Must

- v2.0: widget manifest as JSON in `kelvo-schema`, validated at build time, with TS generated from it.
- v2.0: the composer's "Menu bar popover" tab with library, drag into preview, reorder, inspector (show title, custom title, chart style, window), Cancel and Save.
- v2.0: the popover renders from the saved widget layout, and the default layout reproduces the v1 popover.
- v2.1: one transparent board window per display at desktop-icon level, rendering that display's widgets.
- v2.1: edit mode with 8 pt grid snapping, neighbouring-edge snapping, guides, resize handles, size readout and a Done pill.
- v2.1: per-widget appearance and opacity, with forced opaque rendering when Reduce Transparency is on.
- v2.1: streaming pauses on occlusion, display sleep, lock and fullscreen Spaces.
- v2.1: a per-display performance budget, measured.
- v2.2: a WidgetKit extension built from source, signed with a stable local identity, reading a file feed written through a `WidgetFeedSink` trait.
- v2.3: alert rules editor, configurable Overview cards, per-widget history windows up to 24 hours.

### Should

- v2.0: live channel interest narrowing, so a window only receives the series its widgets use.
- v2.1: a Tinted appearance (CSS only) as the fallback if native blur behind a transparent webview fails its spike.
- v2.2: AppIntent configuration for the WidgetKit widgets (custom title, chart style).
- v2.3: alert rule templates for the common cases (memory pressure, battery low, disk free, fans).

### Won't

- Free-form widget authoring or third-party widget plugins. The manifest is internal data, not a public API.
- Click-through interaction with desktop widgets at rest. At rest the board ignores the mouse; interaction happens in edit mode.
- Dragging a widget from one display to another in edit mode. The inspector's Display field moves it.
- Shipping the WidgetKit extension in the public v2 release DMG. It needs a signing identity that exists only on the builder's Mac. Public WidgetKit is v3.1.
- App Group data sharing. App Groups need a paid team, so v2.2 uses a temporary-exception file feed.
- Remote-host widgets. The layout schema carries an optional host field for a later version, but no v2 UI sets it.

## Experience

### Widget composer, Menu bar popover tab (v2.0)

Regions: the header (title "Widgets", the "Menu bar popover" tab, "Unsaved changes", Cancel, "Save changes"), the left column inspector ("CPU usage · Selected": Show title, Custom title, Title text, Chart Area/Bars/Line, Window 60s), the Library grouped by module with "Drag into the preview", and the preview column with the tray icon readout above a 360 pt popover stack.

The composer is the Widgets route in the dashboard sidebar. The preview is a real render: it uses the same widget host as the popover with live local data, so what you see is what the popover will show. Interactions, all visible in the mock:

| Interaction | Mock evidence | Behaviour |
|---|---|---|
| Add | Library tile "Network" shown dimmed with a dashed border while a "Network" ghost floats over the preview, plus the cyan insertion line between RAM usage and GPU | Drag a tile into the preview; an insertion line shows the drop slot. The tile dims while dragging. Click-to-add appends to the end for keyboard users |
| Select | "Build machine" card has a cyan ring and a close button | Click a widget to select it; the inspector shows its options |
| Reorder | Drag handle (six dots) left of the selected card | Drag the handle, or Option+Up/Down with the widget selected |
| Remove | The × on the selected card | Click ×, or Delete with the widget selected |
| Hide title | Bottom card reads "Untitled" with "Title hidden" | With Show title off, the preview keeps a muted placeholder title so the card stays identifiable while editing |
| Save | "Unsaved changes" plus Cancel and "Save changes" | Save sends the whole layout to Rust. Cancel discards the draft. Leaving the route with unsaved changes asks to save or discard |

Empty state: removing every widget leaves a dashed drop target reading "Drag a widget here". Saving an empty popover is allowed; the popover then shows only its header and footer.

Error state: if Rust rejects a layout (for example an option value outside the manifest's schema after a downgrade), the composer keeps the draft, shows the error inline under the header, and does not clear "Unsaved changes".

### Popover rendered from layout (v2.0)

The popover header and footer stay fixed as in v1. The card stack between them becomes a list of widget instances from the saved popover layout. A widget whose module is disabled in Settings, or whose capability the host lacks, renders the v1 "not available" state rather than disappearing, so the user can see why it is blank. A saved instance whose widget id this build doesn't know (after a downgrade) renders as "Unavailable widget" and is preserved on save.

### Composer, Desktop widget tabs (v2.1)

The composer header gets a segmented control: "Desktop widget 1", "Desktop widget 2", "+".

Each "Desktop widget N" tab edits one board: a named group of widgets assigned to a display. Several boards can target the same display, and all boards for a display render in that display's single window. "+" creates a board on the main display. The left column keeps the Library and adds board fields to the inspector (see the next section). The preview shows a scaled outline of the target display with the board's widgets at their frames. Exact positioning happens on the desktop in edit mode, launched from an "Edit on desktop" button in the header.

Empty state: a new board has no widgets and the scaled display shows "Drag a widget here".

### Desktop board inspector fields (v2.1)

No mock. Compose from `09-widget-composer` inspector rows (switch row, segmented pill control, the "Window 60s" value row).

Board-level fields: Name, Display (a select listing connected displays by name, plus remembered disconnected ones marked "not connected"). Widget-level fields added for board widgets: Appearance (Vibrant, Tinted, Solid) as a segmented control, and Opacity as a slider from 40% to 100%. When Reduce Transparency is on, the Appearance control shows Solid selected and disabled, with the note "Reduce Transparency is on in System Settings".

### Desktop board at rest (v2.1)

Reference arrangement, the three widgets not being edited: "CPU cores" (per-core heatmap, 2 minutes, P/E rows), "Network" (up/down rates, mirrored bars, 64 seconds), "Power" (CPU, GPU, System rings).

Widgets sit on the wallpaper below desktop icons' interaction but above the wallpaper, on every Space. The default Vibrant appearance is a native blur material behind each card with the `--color-widget` tint on top, so the wallpaper shows through softened. Tinted drops the blur and keeps only the translucent tint; Solid is opaque. At rest the board window ignores the mouse, so clicks go to the desktop.

When a widget's data is paused (window covered, display asleep), it keeps its last frame. When streaming resumes, the ring buffer backfills the gap, because the engine kept sampling while the board was paused. If the engine itself was asleep, the sleep gap renders as a gap, never interpolated.

### Desktop board edit mode (v2.1)

Reference arrangement, the "Memory" widget being edited: four corner resize handles, cyan outline, size readout "344×148", the dashed alignment guides running the full width and height of the display, the dashed outline of the widget's previous position, and the bottom pill "Editing widgets · snaps to 8 pt and neighbouring edges" with the Done button.

Entry points: the composer's "Edit on desktop" button, and "Edit desktop widgets" in the tray menu. Edit mode applies to every display at once. While editing, board windows accept the mouse and rise above normal windows so widgets are reachable even if the desktop is covered.

| Rule | Value |
|---|---|
| Grid | 8 pt, origin at the display's visible frame (below the menu bar) |
| Edge snapping | Within 6 pt of a neighbour's left, right, top, bottom or centre line, or the display margin (16 pt) |
| Priority | Neighbour edges win over the grid |
| Guides | Dashed 1 px lines across the whole display for each active snap line |
| Resize | Four corner handles; sizes clamp to the manifest's min and max for that widget |
| Size readout | Width × height in points, below the bottom-right corner, updates during drag and resize |
| Overlap | Not allowed. A drop that overlaps animates back to the last valid frame (150 ms) |
| Keyboard | Tab cycles widgets; arrows nudge 1 pt, Shift+arrows 8 pt; Escape cancels all edits; Return or Done saves |
| Done | Saves every board's frames in one command and returns windows to desktop-icon level |

Accessibility: each widget in edit mode is focusable and announces "Memory widget, editing", plus its size.

### WidgetKit widgets in the gallery (v2.2)

No mock. Compose from `10-desktop-widgets` widget cards (CPU cores heatmap, Power rings, Memory bar) and `14-component-popover-panel` cards, redrawn in SwiftUI at WidgetKit's systemSmall, systemMedium and systemLarge sizes.

The widgets show the latest value and a 30-minute sparkline from the feed. When the feed is older than 5 minutes, the widget shows "as of 14:05" in the footer. When it is older than 30 minutes, it shows "Kelvo isn't running" instead of values. Stale data is always labelled, never presented as current.

### WidgetKit status in Settings (v2.2)

No mock. Compose from `13-settings` General group (label row with a trailing value, as in "History on disk 148 MB").

Only builds with the `widgetkit` feature show this row: "Desktop widgets (WidgetKit)" with "Feed written 12 s ago" as the trailing value and a "Reload widgets" button. If the extension is not registered with the system (checked via `pluginkit`), the row says "Extension not registered. Launch Kelvo from /Applications once, then reopen the widget gallery."

### Alert rules editor (v2.3)

No mock. Compose from `13-settings` Modules table (rows with a select and a trailing switch) for the rule list, and the `09-widget-composer` inspector rows for the edit panel.

The Alerts page lists rules as rows: name, a one-line condition summary ("Memory pressure above 80% for 2 min"), last fired time, and an enabled switch. "New rule" opens an edit panel with: metric (picker grouped by module, from the catalog), aggregation (average or maximum over the sustain window), comparator, threshold with unit, sustained for (30 s to 30 min), cooldown, and outputs (notification, sound, timeline annotation, include top process). v1.2's built-in rules appear in the list and can be edited or disabled but not deleted.

Empty state: "No rules yet" with template buttons (memory pressure, battery low, disk free below, fan above, power above).

### Customize Overview (v2.3)

The Overview header gets a "Customize" button next to the "Live" pill, above the grid of six module cards. Edit mode reuses the composer preview affordances (drag handle, × on the selected card, Cancel and Save).

Customize turns the Overview into edit mode: each card gets a drag handle and a remove button, hidden cards appear in an "Add card" menu, and each card gets one option, "Top processes" (off, 3, 5). The machine header can be collapsed. Cancel and Save behave as in the composer.

### Per-widget history window (v2.3)

The composer inspector row "Window 60s".

The Window option grows from the live range (60 s, 5 min, 15 min, 1 h) to include 6 h and 24 h for chart widgets. Windows up to 1 hour read the in-memory ring buffer as in v2.0. Longer windows read the 10 s tier and append the live tail. Gaps from sleep show as gaps.

## Architecture changes

### Widget manifest (v2.0)

Widget definitions move into `crates/kelvo-schema/widgets/manifest.json`. Rust loads it with `include_str!` and serde (`deny_unknown_fields`). Rust types for the manifest live in `kelvo-schema` and are exported to TS through tauri-specta like every other schema type. The manifest's contents (the list of widgets) are emitted as a TS `const` by a generator, `cargo run -p kelvo-schema --bin gen-widgets`, into `src/core/generated/widgets.ts`, along with a discriminated union of per-widget option types keyed by widget id. CI runs the generator and fails on a diff.

Each entry carries:

| Field | Purpose | Example |
|---|---|---|
| `id` | Stable id, never reused | `cpu.usage` |
| `version` | Bumped when options change shape; drives option migration | `1` |
| `module` | Accent, grouping in the library, capability check | `cpu` |
| `title`, `glyph` | Library tile | "CPU usage", area glyph |
| `metrics` | `SeriesSelector`s (metric plus label subset, from architecture item 7) the widget reads | `cpu.load{mode=user}`, `cpu.load{core=*}` |
| `processes` | Whether it needs the top-N process list, and N | `{ "top": 5, "sort": "cpu" }` |
| `surfaces` | Where it may appear | `popover`, `board`, `overview`, `widgetkit` |
| `sizes` | Per surface: popover height class; board min/default/max in pt; WidgetKit families | `board: { min: [240,96], default: [344,148], max: [640,320] }` |
| `options` | A small JSON Schema subset (boolean, string with max length, enum, integer range) with defaults | `chart: enum[area,bars,line]` |
| `cadence` | Minimum useful refresh | `1s` or `10s` |

The v2.0 manifest entries, matching the composer library plus Power, which the desktop widgets and the v1 popover use:

| id | Library name | Module | Reads | Surfaces | WidgetKit families (v2.2) |
|---|---|---|---|---|---|
| `cpu.usage` | CPU usage | CPU | user and system load | popover, board, widgetkit | small, medium |
| `cpu.cores` | CPU cores | CPU | per-core load, cluster frequency | popover, board, widgetkit | medium, large |
| `cpu.tasks` | Task CPU | CPU | top processes by CPU | popover, board | none |
| `mem.usage` | RAM usage | Memory | used, total, pressure, swap, wired, compressed | popover, board, widgetkit | small, medium |
| `mem.tasks` | Task RAM | Memory | top processes by memory | popover, board | none |
| `gpu.usage` | GPU | GPU | utilization, frequency, power | popover, board, widgetkit | small, medium |
| `disk.storage` | Storage | Disk | per-volume used and total, read/write rates | popover, board | none |
| `net.rates` | Network | Network | per-interface rx/tx | popover, board, widgetkit | medium |
| `power.rails` | Power | Power & Sensors | CPU, GPU, ANE, DRAM, system watts | popover, board, widgetkit | small, medium |
| `sensors.fans` | Fans | Power & Sensors | fan rpm per fan | popover, board | none |
| `sensors.temps` | Sensors | Power & Sensors | SoC zone temperatures | popover, board, widgetkit | medium |
| `battery.status` | Battery | Battery | charge, health, cycles, time remaining | popover, board, widgetkit | small |

Power is in the v1 popover and the desktop widgets, so the manifest includes it and the library shows it under Power & Sensors. Per-widget options beyond the common set (show title, custom title, chart, window): `cpu.cores` has style (tiles as in the popover, or a heatmap as on the desktop), `net.rates` has interface (auto or a named one), `disk.storage` has volume, `sensors.temps` has a zone selection, `power.rails` has style (rings or stacked bar).

A Rust test validates the manifest: every metric pattern resolves against the v1 metric catalog, min ≤ default ≤ max for every size, every option default satisfies its own schema, and ids are unique. Metric names in this doc are descriptive; the catalog in `kelvo-schema` is authoritative, and the test is what keeps the two in step.

### Widget layouts (v2.0)

The store already uses "layout" for an ordered list of series ids, so this doc calls the new thing a widget layout and the type `WidgetLayout`, to keep the two apart in code and conversation.

```rust
// crates/kelvo-schema/src/widgets.rs
pub struct WidgetLayout {
    pub id: WidgetLayoutId,           // uuid; distinct from kelvo-store's series LayoutId
    pub surface: Surface,             // Popover | Board { board_id, display_uuid, name } | Overview
    pub items: Vec<WidgetInstance>,
    pub schema_version: u32,
}

pub struct WidgetInstance {
    pub instance_id: Uuid,
    pub widget_id: String,            // manifest id, e.g. "cpu.usage"
    pub widget_version: u32,
    pub options: serde_json::Value,   // validated against the manifest option schema
    pub frame: Option<Rect>,          // board only, in points relative to the display's visible frame
    pub appearance: Option<Appearance>, // board only: Vibrant | Tinted | Solid, opacity 40..=100
    pub host: Option<HostId>,         // None means the local host; no v2 UI sets it
}
```

Rust owns widget layouts under the v1 single-writer rule. They are stored next to settings with `tauri-plugin-store`, written only from Rust. New commands: `widget_layouts_get`, `widget_layout_save(layout)` (validates every instance against the manifest, migrates old option versions, rejects with a typed error) and `board_frames_save(Vec<(instance_id, Rect)>)` for edit mode. A `widget-layouts-changed` event updates each window's read-only mirror, the same way `settings-changed` does in v1.

On first launch of v2.0, a migration builds the default popover layout from the v1 popover settings, so nothing visibly changes on upgrade.

### Widget host (v2.0)

Widgets in `src/app/widgets/**` stay render-only under the v1 boundary rule. A new `src/app/widget-host/` sits outside that boundary. It takes a `WidgetInstance`, resolves the manifest entry, selects the needed slice from the per-host zustand store (or, for long windows in v2.3, a TanStack Query history query keyed `(hostId, module, range, tier)`), and renders the registered component with props. The popover, composer preview, board windows and Overview all render through the widget host.

Should: a `channel_set_interest(window, patterns)` command narrows the window's live channel to the union of its widgets' metric patterns. The engine still samples every enabled module; this only cuts serialization and IPC for windows that show two widgets.

The composer uses `@dnd-kit/core` and `@dnd-kit/sortable` for drag and drop rather than HTML5 drag events, which behave inconsistently in WKWebView (unverified for Tauri 2's WKWebView, and the reason to test drag early in phase 2.0b).

### Board windows (v2.1)

A `BoardManager` in `src-tauri/src/boards/` owns one window per connected display, labelled `board-<display_uuid>`, routed to `board/:display`.

| Concern | Decision |
|---|---|
| Display identity | `CGDisplayCreateUUIDFromDisplayID`, so boards come back when a display reconnects |
| Window | Borderless, transparent, non-activating, no shadow, covering the display's visible frame |
| Level at rest | `kCGDesktopIconWindowLevel` via `objc2`, collection behaviour `stationary | ignoresCycle | canJoinAllSpaces`, `ignoresMouseEvents = true` |
| Level in edit mode | Floating level, `ignoresMouseEvents = false`; restored on Done or Escape |
| One webview per display | All widgets on a display share one WebContent process. One window per widget would multiply the per-process cost |
| Display changes | `NSApplicationDidChangeScreenParametersNotification` triggers a reconcile: create or close windows, clamp out-of-bounds widgets at render time without rewriting saved frames |
| Pause | Rust stops the board's channel and sends `board-paused`; the page stops animation. On resume it sends `board-resumed` and a ring-buffer backfill |

Pause conditions:

| Condition | Detection | Action |
|---|---|---|
| Board window fully covered | `NSWindowDidChangeOcclusionStateNotification`, occlusion state not visible | Stop that board's channel |
| Display asleep | v1 `PowerSignals` display sleep, plus `NSWorkspaceScreensDidSleepNotification` | Stop all boards |
| Screen locked | v1 `PowerSignals` lock signal | Stop all boards |
| Fullscreen Space on that display | Occlusion state, re-checked on `NSWorkspaceActiveSpaceDidChangeNotification` (whether occlusion alone covers this is unverified) | Stop that board's channel |
| Low Power Mode or battery | v1 rule | Keep streaming at 2 s, tweens off |
| Edit mode | | Always stream |

Board geometry (snapping, guides, overlap checks, clamping) is pure TS in `src/core/board-geometry.ts` with unit tests, so it runs the same in Vitest and in the window.

Appearance follows the two-layer vibrant surface rule in [design-system.md](design-system.md#vibrant-surfaces): a native material behind a transparent webview, with a CSS tint on top. A board window spans the whole display, so a window-wide material would blur the entire desktop. Instead the board gets one `NSVisualEffectView` per widget, inserted below the webview and positioned from the widget frames by a `board_set_material_rects` command that the page calls after layout and during edit-mode drags. Vibrant is that material plus `--color-widget` (and `--color-widget-editing` for the widget being edited). Tinted is the CSS tint alone, a `color-mix` of the widget token with the module accent at the widget's opacity. Solid uses the opaque fallback tokens. Whether WKWebView with `drawsBackground = false` composites cleanly over sibling effect views, and whether moving them during a drag keeps up at 60 fps, is unverified and is the first task of phase 2.1c; if it fails, Tinted becomes the default and Vibrant is dropped. Reduce Transparency (Rust reads `accessibilityDisplayShouldReduceTransparency` and sets `data-reduce-transparency`, as for the popover) removes the materials and forces Solid at 100% opacity.

### WidgetKit dev build (v2.2)

Research supports this approach for personal use with a free Apple account, but nobody has confirmed the full recipe end to end. Everything below is the plan to verify it, and phase 2.2a exists to prove or kill it cheaply.

#### Signing

WidgetKit caches widgets by signing identity. Plain ad-hoc signing changes the code directory hash on every build, so the widget drops out of the gallery after a rebuild. The build therefore needs a stable local identity:

- a self-signed code-signing certificate in the login keychain (Keychain Access, Certificate Assistant, type Code Signing), or
- the Personal Team "Apple Development" certificate that Xcode creates for a free Apple account.

Either is selected with `APPLE_SIGNING_IDENTITY`, which Tauri's bundler reads. `scripts/widgetkit/ensure-identity.sh` checks `security find-identity -v -p codesigning` for it and prints setup steps if it is missing.

#### Extension and data

The SwiftUI extension lives in `native/KelvoWidgets/`, created from Xcode's Widget Extension template. Widget kinds and the Swift feed types are generated from the v2.0 manifest and `WidgetFeedDoc` by the same `gen-widgets` binary (a `--swift` mode emitting kind ids, display names, supported families and `Codable` structs). A fixture test pins the generated decoder to what Rust writes.

The extension must be sandboxed. App Groups need a team, so v2.2 skips them. Instead:

- The unsandboxed app writes one document per widget kind to `~/Library/Application Support/Kelvo/widget-feed/<kind>.json`, for example `cpu.usage.json`. This is deliberately a fixed, product-named path, separate from Tauri's identifier-based app data directory, so the entitlement string never changes.
- The extension's entitlements are `com.apple.security.app-sandbox` and `com.apple.security.temporary-exception.files.home-relative-path.read-only` with the value `/Library/Application Support/Kelvo/widget-feed/`.

A new bus subscriber, `WidgetFeed`, in `src-tauri/src/widget_feed/`, builds one document per kind and hands it to a sink. The trait is the one sketched in [architecture.md](architecture.md#deliberately-deferred):

```rust
pub trait WidgetFeedSink: Send + Sync + 'static {
    fn write(&self, kind: &WidgetKindId, doc: &WidgetFeedDoc) -> io::Result<()>;
    fn request_reload(&self, kinds: &[WidgetKindId]); // no-op without the Swift bridge
}

pub struct FileWidgetFeed { dir: PathBuf }           // v2.2: widget-feed/ under Application Support
pub struct AppGroupWidgetFeed { container: PathBuf } // v3.1: TEAMID.com.tryopendata.kelvo container
```

`FileWidgetFeed` writes `<kind>.json.tmp` and renames it over `<kind>.json`, so the extension never reads a half-written file. `WidgetFeedDoc` is a `kelvo-schema` type, so its JSON shape has one source of truth. The `cpu.usage` document:

```json
{
  "feed_version": 1,
  "written_at": 1791200000000,
  "host_id": "3f1c…",
  "app_version": "2.2.0",
  "kind": "cpu.usage",
  "value": 18.2,
  "parts": { "user": 12.4, "system": 5.6 },
  "series": { "start": 1791198200000, "step_s": 60, "values": [17.1, 19.4, null, 18.0] }
}
```

`null` in a series is a gap and the extension draws it as one. Only kinds that list `widgetkit` as a surface get a document, each stays under 4 KB, and they are written every 60 s when the 1 min bucket closes, and immediately after the app starts.

#### Reloads

`WidgetCenter.reloadAllTimelines()` is Swift-only, so the app reaches it through a small Swift package, `native/KelvoBridge`, linked with the `swift-rs` crate and exposing one `@_cdecl` function. `FileWidgetFeed::request_reload` calls it at most every 5 minutes (configurable), not on every write. If reloads are throttled or ignored, the extension's timeline policy is the fallback: each timeline ends with `.after(now + 5 min)`. WidgetKit's reload budget for an accessory (menu bar) app is unverified, so phase 2.2b measures it.

#### Packaging

Everything sits behind a `widgetkit` cargo feature and a config overlay, `src-tauri/tauri.widgetkit.conf.json`, so the default build and the public release DMG are unchanged. Build with `bun tauri build --features widgetkit --config src-tauri/tauri.widgetkit.conf.json`. Order matters:

1. `beforeBundleCommand` runs `scripts/widgetkit/build-appex.sh`. It regenerates the Swift kinds and feed types, computes a build number, and runs `xcodebuild` with `CODE_SIGNING_ALLOWED=NO` and `CURRENT_PROJECT_VERSION=<build>`.
2. The same script signs the appex first: `codesign --force --sign "$APPLE_SIGNING_IDENTITY" --entitlements native/KelvoWidgets/KelvoWidgets.entitlements KelvoWidgets.appex`.
3. Tauri copies it in through `bundle.macOS.files` with the key `"PlugIns/KelvoWidgets.appex"`.
4. Tauri signs the outer app with the same identity. No `--deep`, because a deep re-sign would strip the appex's sandbox entitlements. Whether tauri-bundler re-signs nested bundles under `PlugIns/` on its own is unverified. If it does, a post-bundle step re-signs the appex and then the outer app, in that order.
5. `CFBundleVersion` is bumped on every build for both the app and the appex. `chronod` keeps rendering a stale extension when the version doesn't change.

The app must launch once before its widgets show in the gallery, so the install script copies the app to `/Applications`, opens it, and checks registration with `pluginkit -m -v -p com.apple.widgetkit-extension | grep com.tryopendata.kelvo`.

#### Sources

- AIQuotaBar WidgetKit with a free account: github.com/yagcioglutoprak/AIQuotaBar/pull/17
- Self-signed WidgetKit clock: github.com/goranimperator/imperator-widget-clock
- Tauri WidgetKit plugin notes: s00d.github.io/tauri-plugin-widgets
- Tauri appex embedding and signing order: github.com/lidge-jun/opencodex/pull/5299 and github.com/lidge-jun/opencodex/pull/5345
- Widget caching by signing identity and stale renders: developer.apple.com/forums/thread/758375
- App Groups require a team: developer.apple.com/help/account/reference/supported-capabilities-macos

### Alerts editor (v2.3)

Rule definitions are already serializable data in `kelvo-schema` (v1.2) and evaluated by the engine. v2.3 adds rule kinds and fields, not a new evaluator: threshold rules over any catalog series with an aggregation, comparator, sustain window and cooldown, plus outputs. It also adds `scope: RuleScope` to `AlertRule`, with a single variant, `Local`, in v2.3. The field costs nothing now and lets v4.2 add remote scopes without a rule migration. New commands `alert_rules_get`, `alert_rule_save`, `alert_rule_delete`, `alert_rule_test` (evaluates the rule against the last hour of the 10 s tier and returns when it would have fired, so the editor can preview "would have fired 3 times today").

### Overview layout (v2.3)

The Overview becomes a `WidgetLayout` with surface `Overview`. The six module cards from v1 become manifest entries with only the `overview` surface (`overview.cpu`, `overview.gpu`, and so on), with the "Top processes" option. The default Overview layout reproduces v1.

## Data and schema changes

| Change | Where | Version | Migration |
|---|---|---|---|
| Widget manifest | `kelvo-schema/widgets/manifest.json` | 2.0 | None; new |
| `WidgetLayout`, `WidgetInstance`, `Surface`, `Appearance` | `kelvo-schema`, persisted in the settings store as `widget_layouts` with `schema_version` | 2.0 | Default popover layout generated from v1 popover settings |
| Board surfaces with frames and appearance | Same store | 2.1 | None; new surface variant |
| `WidgetFeedDoc` | `kelvo-schema`, JSON fixture `fixtures/widget-feed-v1.json` | 2.2 | None; files are rewritten every 60 s |
| Alert rule fields (aggregation, sustain, cooldown, outputs, `scope`) | `kelvo-schema` alert-rule data, `rule_version` bump | 2.3 | v1.2 rules upgraded in place with defaults |
| Overview surface | Settings store | 2.3 | Default Overview layout from v1 card order |

No SQLite schema changes. Per-widget history reads the existing 10 s tier.

## Performance budget deltas

v1's budget stays as the baseline: idle means no visible surface, and that number must not move in v2. The deltas below apply when the surface is visible.

| Scenario | CPU (coalition delta) | Memory (phys_footprint delta) | Method |
|---|---|---|---|
| Popover rendered from a widget layout vs the v1 popover, open, 7 widgets | Within 5% of v1 | Within 5 MB of v1 | `scripts/bench-vs-stats.sh` with the popover pinned open |
| One board window visible, 4 widgets, 1 s sampling | ≤ 0.15% | ≤ 45 MB (one WebContent process) | Same script, per display |
| One board window covered or display asleep | ≤ 0.01% | Unchanged | Same script, window occluded |
| WindowServer cost per visible board | ≤ 0.1% | | WindowServer CPU-time delta |
| WidgetKit feed writer | ≤ 0.01% (one write per minute) | ≤ 1 MB | Same script with `widgetkit` feature on |
| Composer open | Not budgeted (interactive) | | |

The per-display budget is why boards share one webview per display. Twelve widgets is the soft cap per display; the composer warns beyond it.

## Milestones

Each minor version is installable on its own. Phases within a version are ordered.

### v2.0: Manifest and popover composer

#### Phase 2.0a: Manifest and widget layouts

Schema and codegen:
- [ ] `crates/kelvo-schema/widgets/manifest.json` with the 12 entries in the table above
- [ ] Manifest Rust types with `deny_unknown_fields`, exported through tauri-specta
- [ ] Manifest validation test: metric patterns resolve against the catalog, sizes ordered, option defaults valid, ids unique
- [ ] `gen-widgets` binary emits `src/core/generated/widgets.ts` (data plus per-widget option union)
- [ ] CI step runs `gen-widgets` and fails on `git diff --exit-code src/core/generated`

App shell:
- [ ] `WidgetLayout` persisted in the settings store, written only from Rust
- [ ] Commands `widget_layouts_get`, `widget_layout_save` with typed validation errors; `widget-layouts-changed` event
- [ ] Option migration by `(widget_id, from_version)`; unknown widget ids preserved on save
- [ ] First-run-of-v2 migration from v1 popover settings, with a Rust test using a v1 settings fixture

Acceptance: `cargo test -p kelvo-schema` passes; saving a layout with an out-of-range option returns a typed error; a v1 settings fixture migrates to a layout with the seven v1 popover cards in v1 order.

#### Phase 2.0b: Widget host and popover

Frontend:
- [ ] `src/app/widget-host/` renders a `WidgetInstance` through the registry; Biome boundary rule still passes for `src/app/widgets/**`
- [ ] Popover card stack renders from the popover layout
- [ ] "Not available" and "Unavailable widget" states per the Experience section
- [ ] Spike: `@dnd-kit` drag inside the Tauri WKWebView, with pointer and keyboard sensors (result noted in PROGRESS.md)

Acceptance: a Playwright screenshot of the default popover layout (mock transport) matches the v1 popover baseline within the existing diff threshold; popover open time stays under 150 ms.

#### Phase 2.0c: Composer

Frontend:
- [ ] Widgets route with the composer header (tabs, Unsaved changes, Cancel, Save changes)
- [ ] Library grouped by module from the manifest, accent dot per group, "Drag into the preview" hint
- [ ] Preview with live data, insertion line, drag handle reorder, × remove, select ring
- [ ] Inspector driven by the option schema (switch for boolean, text for string, segmented control for enum)
- [ ] Keyboard: click-to-add, Option+Up/Down reorder, Delete remove
- [ ] Leave-with-unsaved-changes prompt
- [ ] Playwright test: add Network between RAM usage and GPU, rename CPU usage to "Build machine", save, reopen, order and title persist

Acceptance: the composer matches the layout described under "Widget composer, Menu bar popover tab" in a screenshot review; the saved layout shows in the popover within one `widget-layouts-changed` event.

#### Phase 2.0d: Interest narrowing (Should)

- [ ] `channel_set_interest(window, patterns)` command; engine bus filters per window
- [ ] Bench: popover with 2 widgets vs 7 widgets, IPC bytes per second logged

Acceptance: IPC bytes per second for a 2-widget popover drop by at least half against the 7-widget default.

### v2.1: Floating desktop widgets

#### Phase 2.1a: Board windows

App shell:
- [ ] `BoardManager` creates one window per display keyed by display UUID
- [ ] Window level `kCGDesktopIconWindowLevel`, collection behaviour `stationary | ignoresCycle | canJoinAllSpaces`, mouse ignored at rest
- [ ] Reconcile on screen parameter changes; widgets on a disconnected display stay saved
- [ ] Pause and resume per the pause table, using v1 `PowerSignals` and occlusion notifications
- [ ] `board-paused` / `board-resumed` events; ring-buffer backfill on resume

Acceptance: with one external display, unplug and replug restores that board's widgets in the same positions; covering a board with a fullscreen window stops its channel (verified in logs) within 1 s.

#### Phase 2.1b: Board composer and edit mode

Frontend:
- [ ] Composer tabs for boards, "+" creates a board on the main display, scaled display preview
- [ ] Inspector fields: Name, Display, Appearance, Opacity
- [ ] `src/core/board-geometry.ts`: grid snap, edge snap with 6 pt threshold, guides, overlap rejection, clamp; Vitest table tests for each rule
- [ ] Edit mode overlay: corner handles, size readout in pt, dashed guides, previous-position outline, bottom pill with Done
- [ ] Keyboard nudge, Tab cycling, Escape cancel, VoiceOver labels

App shell:
- [ ] Edit mode raises all boards to floating level and accepts mouse; Done and Escape restore
- [ ] `board_frames_save` writes all frames in one command
- [ ] Tray menu item "Edit desktop widgets"

Acceptance: the reference arrangement under "Desktop board edit mode" can be reproduced on a 1280×800 display and its size readout shows 344×148 for the Memory widget; every geometry test passes.

#### Phase 2.1c: Appearance and accessibility

- [ ] Spike: one `NSVisualEffectView` per widget rect behind a transparent WKWebView, moved during a drag (result noted in PROGRESS.md)
- [ ] Vibrant, Tinted and Solid appearances with opacity 40% to 100%; Tinted becomes the default if the spike fails
- [ ] Reduce Transparency forces Solid at 100% and disables the control
- [ ] `board_set_material_rects` command, called after layout and throttled to one call per frame during drags
- [ ] Contrast test: widget text over a light and a dark wallpaper fixture at 40% opacity meets 4.5:1 with Vibrant and Tinted

Acceptance: toggling Reduce Transparency in System Settings updates all boards without restart.

#### Phase 2.1d: Budget

- [ ] Add a board scenario to `scripts/bench-vs-stats.sh` (visible, covered, display asleep)
- [ ] Record results per display in PROGRESS.md

Acceptance: the numbers meet the performance budget table; v1 idle numbers unchanged.

### v2.2: WidgetKit dev build

#### Phase 2.2a: Prove the recipe

- [ ] Create the self-signed identity per the docs and verify with `security find-identity -v -p codesigning`
- [ ] Xcode widget template in `native/KelvoWidgets/` with one static `cpu.usage` kind reading a hard-coded feed path
- [ ] Hand-run the packaging order (appex sign, embed, outer sign) and confirm with `codesign -d --entitlements - Kelvo.app/Contents/PlugIns/KelvoWidgets.appex` that the sandbox and temporary exception survive
- [ ] Confirm the widget appears in the gallery, survives a rebuild with a bumped `CFBundleVersion`, and reads the feed file
- [ ] Record whether supported versions (26+) prompt for access to the feed directory
- [ ] Write the outcome to `decisions.md`; stop v2.2 here if the recipe fails and move WidgetKit wholly to v3.1

Acceptance: a rebuilt app with a bumped build number still shows the widget in the gallery with fresh data, on the developer's Mac.

#### Phase 2.2b: Feed and reloads

App shell:
- [ ] `WidgetFeedDoc` in `kelvo-schema` with the committed JSON fixture
- [ ] `WidgetFeed` bus subscriber, `WidgetFeedSink` trait, `FileWidgetFeed` with atomic rename per kind
- [ ] `native/KelvoBridge` via `swift-rs`, `kelvo_widgets_reload_all()`, called at most every 5 minutes
- [ ] Settings row for feed status, "Reload widgets", and the extension-not-registered message

Extension:
- [ ] Generated Swift feed types plus an `xcodebuild test` that decodes the Rust-produced fixture
- [ ] Timeline policy `.after(now + 5 min)`; "as of" footer past 5 minutes, "Kelvo isn't running" past 30
- [ ] Log reload outcomes for a day and record the observed refresh interval in PROGRESS.md

Acceptance: a Rust change to `WidgetFeedDoc` that breaks the Swift decoder fails `xcodebuild test`; with the app quit, widgets show "Kelvo isn't running" within 35 minutes.

#### Phase 2.2c: Kinds and packaging

- [ ] `gen-widgets --swift` emits kinds for every manifest entry with the `widgetkit` surface
- [ ] SwiftUI views for the kinds in the manifest table, small/medium/large per entry
- [ ] `tauri.widgetkit.conf.json` overlay with `beforeBundleCommand` and `bundle.macOS.files` `PlugIns/KelvoWidgets.appex`
- [ ] `scripts/widgetkit/build-appex.sh` (xcodebuild, build number bump, appex signing) and `ensure-identity.sh`
- [ ] `scripts/widgetkit/install.sh` (copy to /Applications, launch, `pluginkit` check)
- [ ] README section "Build with WidgetKit" for contributors with a free Apple account
- [ ] Release CI asserts the public DMG contains no `PlugIns/` directory

Acceptance: from a clean checkout and a configured identity, `bun tauri build --features widgetkit --config src-tauri/tauri.widgetkit.conf.json` followed by `install.sh` yields gallery widgets with live data.

### v2.3: Alerts editor, configurable Overview, history windows

#### Phase 2.3a: Alert rules editor

Engine and schema:
- [ ] Rule fields: aggregation, comparator, sustain, cooldown, outputs, `scope` (`Local` only); `rule_version` migration from v1.2
- [ ] `alert_rule_test` against the 10 s tier
- [ ] Engine tests for sustain and cooldown edges, including a sleep gap inside the sustain window (gap resets the window)

Frontend:
- [ ] Alerts route with the rule list, edit panel, templates, and "would have fired" preview

Acceptance: a memory-pressure rule created in the editor fires a notification during a synthetic pressure run, and does not fire again inside its cooldown.

#### Phase 2.3b: Customize Overview

- [ ] `overview.*` manifest entries and the Overview surface with a default layout matching v1
- [ ] Customize mode: reorder, hide, Add card menu, Top processes option, collapsible machine header, Cancel and Save

Acceptance: hiding Disk and moving Network first persists across app restarts; the default layout screenshot matches the v1 Overview.

#### Phase 2.3c: Per-widget history windows

- [ ] Window option values 6 h and 24 h for chart widgets (manifest `version` bump with migration)
- [ ] Widget host reads the 10 s tier through TanStack Query for windows over 1 h and stitches the live tail
- [ ] Gap rendering test: a fixture with a sleep gap renders a gap in a 24 h widget

Acceptance: a 24 h CPU widget on a board shows the same shape as Timeline's 24 h CPU lane for the same period.

## Success criteria

- The default popover after upgrading from v1 is visually identical to v1 (screenshot diff).
- A new user can build a custom popover with three widgets and a custom title in under a minute in an unmoderated test with two people.
- Board windows meet the per-display budget, and v1 idle numbers do not move.
- At least one contributor other than the maintainer builds the WidgetKit edition from the README steps.
- No interpolated data on any v2 surface: gap fixtures render as gaps in popover, board, WidgetKit and history-window widgets.

## Risks and open questions

| Risk or question | Impact | Plan |
|---|---|---|
| The WidgetKit free-account recipe has not been confirmed end to end | v2.2 may not ship | Phase 2.2a proves it in a day or two before any polish; failure moves WidgetKit to v3.1 and is recorded in decisions.md |
| WidgetKit reload budget for an accessory app (unverified) | Widgets update less often than hoped | Timeline policy fallback; measure and document the real interval |
| Supported versions (26+) may prompt for access with temporary exceptions (unverified) | A confusing prompt on first widget render | Record in 2.2a; v3.1's Team-ID App Group removes it |
| tauri-bundler might re-sign nested bundles in `PlugIns/` (unverified) | Appex loses its sandbox entitlements and fails to load | Verify in 2.2a with `codesign -d --entitlements -`; post-bundle re-sign fallback |
| Drag and drop in WKWebView | Composer feels broken | dnd-kit with pointer sensors; spike in 2.0b |
| Native blur per widget behind a transparent WKWebView (unverified) | No blur on desktop widgets | Spike first in 2.1c; Tinted (CSS only) is the fallback default and still reads as the mock's translucent card |
| Whether occlusion state reports fullscreen Spaces on the board's display (unverified) | Boards keep streaming while hidden | Add the active-space check; verify in 2.1a |
| `CGDisplayCreateUUIDFromDisplayID` stability across docks and adapters | Boards reappear on the wrong display or not at all | Fall back to matching by display name and resolution; keep unmatched boards saved |
| Interpretation of "Desktop widget N" tabs as boards assigned to displays | If the intent was one free-floating window per widget, the per-display webview model doesn't fit | Confirm with the maintainer before phase 2.1b |
| Click-through at rest means widgets can't open the dashboard | Some users expect a click to open details | Won't for v2.1; a hit-testing WKWebView subclass is the known approach if demand appears |

## Infra this version lays for later versions

- The widget manifest is the single source for widget ids, metric dependencies and sizes for TS and Swift. v3.1 reuses the generated Swift types unchanged.
- `WidgetInstance.host` exists from v2.0, so remote-host widgets (after v4) need no layout migration. v4.2's remote menu bar metrics use tray items instead.
- `WidgetFeedSink` lets v3.1 swap `FileWidgetFeed` for `AppGroupWidgetFeed` without touching the feed builder.
- `WidgetFeedDoc` and its fixture give v3.1 a contract test that already covers the extension's decoder.
- The `widgetkit` cargo feature and overlay config are where v3.0's Developer ID signing and v3.1's App Group entitlements plug in.
- The alert rule editor writes serializable rules with a scope that v4.2 extends to remote hosts, where agents evaluate them.
- `BoardManager`'s pause logic is the same visibility contract v4.2's fleet view uses to decide when to hold full live streams.

## Depends on

Everything here consumes interfaces defined in v1 (see [v1-local-monitor.md](v1-local-monitor.md) and [architecture.md](architecture.md)), or earlier in v2:

| Interface | Defined in | Used by |
|---|---|---|
| Metric catalog and series keys (`metric_id` plus labels) in `kelvo-schema` | v1.0 | Manifest `metrics` and the validation test |
| Typed `Snapshot` view and per-host zustand stores keyed `hosts[hostId]` | v1.0 | Widget host |
| Rust-owned settings with `settings-changed` and read-only mirrors | v1.0 | Widget layouts follow the same pattern |
| Per-window live channels started and stopped by Rust, ring-buffer backfill | v1.0 | Popover, boards, pause and resume |
| Widget boundary rule for `src/app/widgets/**` | v1.0 | All surfaces render through it |
| `PowerSignals` (display sleep, lock, Low Power Mode, battery) | v1.0 | Board pause rules |
| tauri-specta bindings into `src/core/generated/` | v1.0 | Manifest and layout types |
| History queries with keys `(hostId, module, range, tier)` and the 10 s tier | v1.0 | Per-widget history, `alert_rule_test` |
| Event rows and alert rule data evaluated in the engine | v1.2 | Alerts editor |
| The bus and its subscriber model | v1.0 | `WidgetFeed` subscriber |
| Widget manifest and `WidgetLayout` | v2.0 | v2.1 boards, v2.2 kinds, v2.3 Overview |
