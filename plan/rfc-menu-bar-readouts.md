# RFC: Menu bar readouts

Status: Accepted after review (see "Review outcome" at the end). Date: 2026-10-08. Decision entry: D-102.

## Problem

The combined menu bar item shows three bars and one number, the hottest SoC temperature. Users
who care about something else (power draw on a laptop, memory, a nearly full disk) cannot put
that number next to the bars. The pieces that exist are hard to find: to see watts you set the
"Power & Sensors" row's Menu bar select to "Watts as value", which also removes the temperature,
and the result is a stacked "PWR" label rather than something that reads at a glance. Disk usage
(% of the boot volume used) is not available in the menu bar at all; the Disk value there is
throughput.

The Settings UI makes this worse. Every menu bar decision lives in one select per module
("In combined item", "Value + label", "Temp in combined", "Watts as value", "Own item: graph",
"Own item: value", "Own item: cores", "Hidden"). The select mixes three different ideas (is
this module a bar, is its number printed in the combined item, does it get a status item of
its own) and nothing on the page shows what the menu bar will look like.

## Who and what job

The Kelvo user glances at the menu bar dozens of times a day. The job is "tell me the one or two
numbers I care about on this Mac without opening anything". Which numbers differs by person and
machine: a MacBook user on battery watches watts, someone running local models watches memory,
someone with a 256 GB disk watches disk usage, and the default (temperature) suits people who
care about fan noise and throttling.

Success: a user who wants watts next to the bars finds and sets it in one step from Settings,
sees the result in a preview before looking up at the real menu bar, and every number in the
menu bar says what it is without a legend.

## Prior art

Stats (exelban) and iStat Menus both configure the menu bar per module, each module choosing a
widget (label + value, mini chart, bar) and an order. Stats labels values with tiny stacked
three-letter labels, which Kelvo already copied for its Values style. iStat Menus uses small
SF Symbol-style glyphs (bolt, thermometer, drive) next to values. Both give a live preview in
the settings window. The common pattern: the menu bar is a row of small "readouts", each a
marker plus a value, and the user ticks which ones appear.

## Proposal

### The model: bars, values, separate items

The menu bar config becomes three independent things, each with its own control:

1. Bars. CPU, GPU and Memory can each draw a bar in the combined item. Unchanged geometry.
2. Values. An ordered, fixed catalog of readouts the combined item prints after the bars. Each
   is a marker plus a number. Any number of them can be on.
3. Separate items. A module can get a status item of its own (value, graph, per-core graph),
   as D-080 built. Unchanged behaviour.

These are independent: CPU can be a bar, print "CPU 18%" after the bars, and have a sparkline
item of its own, all at once. Today's single per-module select made them mutually exclusive,
which is why watts and temperature could not both be shown.

The readout catalog, in display order (module order, so the default and the old Values style
look as they do today):

| Readout | Series | Marker | Example | Min width (chars) |
|---|---|---|---|---|
| CPU | `cpu.total` | stacked label "CPU" | `CPU 18%` | 3 |
| GPU | `gpu.util` | stacked label "GPU" | `GPU 36%` | 3 |
| Memory | `mem.pressure` | stacked label "MEM" | `MEM 42%` | 3 |
| Temperature | `thermal.hottest` | none: the degree sign says what it is | `61°` | 3 |
| Power | `power.system` | bolt glyph | `⚡ 14.8W` | 5 |
| Network | `net.tx_total`, `net.rx_total` | arrows, two stacked lines | `38.4 MB/s ↑` over `1.2 MB/s ↓` | 11 at 8 pt (existing Rates layout) |
| Disk | `disk.used`, `disk.total` of the boot volume | drive glyph | `🖴 62%` | 3 |
| Battery | `battery.charge` | stacked label "BAT" | `BAT 87%` | 3 |

Why these markers. CPU, GPU and memory have no glyph that reads at 11 pt (a chip, a card and
a memory stick look alike that small), so they keep the stacked three-letter label the Values
style already uses. Power and disk have glyphs everyone reads instantly. Battery keeps a text
label because a battery glyph would sit a few items away from macOS's own battery icon and
look like a duplicate. Network's up/down pair already has a two-line layout with arrows
(D-080 Rates), which is reused. Temperature keeps its bare "61°": the degree sign identifies
it (the user's call during review), so the default look does not change.

The "SOC" stacked label for temperature without bars goes away: a readout looks the same
whether or not bars are drawn, and "61°" is clear on its own.

Disk is the % used of the boot volume, `disk.used / disk.total` for `HostInfo.boot_mounts[0]`
(the same volume the Overview disk card and the Disk page headline use, D-092). The tray model
receives the boot mount alongside the frame layout; with no boot mount known, the Disk readout
is absent. `disk_capacity` samples every 60 s whatever the interest, so this readout costs no
per-tick work and must not mark the Disk module as menu-bar-shown (that would pull disk
throughput onto the background tick). The separate Disk item keeps showing read + write
throughput, and Settings labels that option "Read + write rate" so the two are not confused
(review finding 2).

### Settings schema (`kelvo-schema`)

```rust
pub struct Settings {
    pub modules: BTreeMap<Module, ModuleSettings>, // ModuleSettings { enabled } only
    #[serde(default)]
    pub menu_bar: MenuBarSettings,
    // ...unchanged
}

pub struct MenuBarSettings {
    pub bars: BarSettings,         // { cpu, gpu, memory: bool }
    pub readouts: ReadoutSettings, // { cpu, gpu, memory, temperature, power, network, disk, battery: bool }
    pub items: ItemSettings,       // { cpu, gpu, memory, power, network, disk, battery: ItemMode }
}
// Each struct is #[serde(default)] field by field: a file written before a readout existed
// gets that readout's default, and a field from a newer build is ignored.

pub enum Readout { Cpu, Gpu, Memory, Temperature, Power, Network, Disk, Battery }
impl Readout {
    pub const ALL: [Readout; 8]; // display order, exported to TS as READOUTS
    pub fn module(self) -> Module; // Temperature -> Sensors, Power -> Power, Disk -> Disk...
}

pub enum ItemMode { Off, Value, Graph, Cores }
// allowed_for: CPU [Off, Graph, Cores, Value]; GPU, Memory, Network [Off, Graph, Value];
// Power, Disk, Battery [Off, Value]. Exported to TS as ITEM_MODES.
```

`MenuBarMode` is deleted, along with `TempInCombined`, `WattsValue` and `ValueLabel`. The
patch type gets `menu_bar: Option<MenuBarPatch>` whose three structs patch per field (D-050),
so toggling one readout never overwrites another window's change to a different one.

Defaults keep today's look: bars CPU, GPU, Memory on; readouts Temperature on, the rest off;
items all Off.

Old settings files: `ModuleSettings.menu_bar` is ignored on decode (no `deny_unknown_fields`)
and the missing `menu_bar` takes its default, so a pre-RFC file loads with the default menu bar
and every other setting intact. Pre-v1, no migration of custom menu bar choices.

`Settings::menu_bar_shows(module)` becomes: enabled, and (bar on, or a readout of that module
on other than Disk, or its item not Off). `Sensors` is shown when the Temperature readout is on
(Power & Sensors enabled); `Power` when the Power readout is on or its item is Value. A disabled
module shows nothing anywhere, as today, but keeps its menu bar choices for when it comes back.

### Tray model and renderer (`src-tauri/src/tray`)

`TrayFrame.combined_text` and the combined item's `values: Vec<Labeled>` become
`readouts: Vec<ReadoutFrame>`, where a readout is `{ marker: Marker, text: String }` and
`Marker` is `Label(&'static str)` (stacked) or `Glyph(Glyph::{Bolt, Drive})`, or `None` for temperature;
Network keeps `Graph::Rates` as its element so the two-line layout and its fixed width carry
over. Own items keep `values` and `graphs` as they are.

`build()` walks bars (CPU, GPU, Memory), then `Readout::ALL`, then own items. A readout whose
module is disabled or whose series the host lacks (`Reading::Absent`) is skipped; a gap or
pause prints the dash with the marker still drawn. The accessibility label keeps its words
("power 14.8 watts", "disk 62 percent used").

Glyph spec (design-system.md, Tray icon spec, gets these rows): drawn with tiny-skia paths in
black like everything else, 12 pt tall box centred on the 18 pt image, 3 pt gap to the value.

- Bolt: filled, 7 pt wide, the sidebar's `power` outline as a fill.
- Drive: 11 × 7 pt rounded rect (radius 1.5), 1.2 pt stroke, a 1.5 pt dot 2 pt in from the
  bottom-right.

Each is checked by eye at 1x and 2x through the existing `dump_tray_rows` PNG test, light and
dark menu bar, before the spec is called done.

### Settings UI

The Modules panel keeps swatch, name and the On switch; its Menu bar column goes. A new
"Menu bar" panel sits under it in the left column:

```
Menu bar
┌──────────────────────────────────────────────────────┐
│  ▮▮▮ 61°  ⚡14.8W                            10:40   │  live TrayPreview of these settings
├──────────────────────────────────────────────────────┤
│ Bars                          [ CPU | GPU | Memory ]  │  multi-select toggle group
├──────────────────────────────────────────────────────┤
│ Values after the bars                                │  field label
│ CPU  CPU                                 18%    ( )  │
│ GPU  GPU                                 36%    ( )  │
│ MEM  Memory                              42%    ( )  │
│ 61°  Hottest temperature                 61°    (●)  │
│ ⚡   Power                             14.8 W   (●)  │
│ ↑↓   Network                  ↑ 38 KB/s ↓ 1.2 MB/s ( )│
│ 🖴   Disk used                           62%    ( )  │
│      Turn on Disk in Modules to use this             │  (disabled row, module off)
│ BAT  Battery                             87%    ( )  │
├──────────────────────────────────────────────────────┤
│ Separate items                                       │  field label
│ CPU                                      [ Off   ▾ ] │
│ GPU                                      [ Off   ▾ ] │
│ ...                                                  │
│ Each gets its own place in the menu bar; ⌘-drag to   │
│ reorder.                                             │  footnote
└──────────────────────────────────────────────────────┘
```

- The preview is the existing `TrayPreview`, refactored to draw from a `MenuBarSettings` value
  plus live readings instead of the three hard-coded styles, so the Settings panel and the
  onboarding cards share one component. It mirrors the Rust geometry as it does today
  (separate items to the left of the combined item, the order macOS uses for new items).
- Each value row shows its marker exactly as the menu bar draws it (stacked label or glyph,
  from the same component the preview uses), its name, the live value it would print, and a
  Switch. The live value is what makes the list self-explanatory: "Power 14.8 W" says what
  the readout is better than any description.
- A row whose module is off is disabled with "Turn on <Module> in Modules to use this"; a row
  whose module the host lacks reads "Not on this Mac", as the Modules panel does. Its switch
  shows off while the module is off; the stored value is kept.
- The Bars group follows the same rule per segment.
- Separate items use the existing `Select`, options from `ITEM_MODES` (exported by Rust like
  `MENU_BAR_MODES` today): Off, Value, Graph, Per-core graph.
- With nothing on (no bars, no values, no items) the preview, like the real item, shows three
  empty tracks; no extra warning.

Copy: "Values after the bars" is the label even with no bars on; that is rare and the preview
shows what it means. Option labels: "Off", "Value", "Graph", "Per-core graph".

### Onboarding

The three style cards stay. They become presets of `MenuBarSettings`:

- Combined: bars CPU, GPU, Memory; Temperature on.
- Values: no bars; CPU, GPU, Memory, Temperature on.
- Graphs: CPU, Memory and Network items Graph; nothing in the combined item.

`TrayPreview` draws each card from its preset, so the cards and Settings agree by construction.

### Engine and performance

Default settings draw what they draw today plus one glyph, so the tray scenario's per-tick work
does not change. `EngineSettings::menu_bar` comes from the new `menu_bar_shows`. Network and
Power readouts pull their modules onto the background tick exactly as "Value + label" did. The
Disk readout pulls nothing (60 s capacity cadence). `make bench` is not rerun for this change;
`cargo test -p kelvo-engine --test perf_gates` covers calls per tick with the defaults.

`src/core/performance.ts` (Performance mode copy that counts own items) and
`scripts/bench-coalition.sh` / `src-tauri/src/bench.rs` (bench presets set
`/modules/<m>/menu_bar`) move to the new `menu_bar.items` paths.

## Out of scope

Reordering readouts (display order is fixed), per-readout colour, choosing which disk volume,
custom label text, and preset shortcuts in Settings (the onboarding cards are the presets; in
Settings the preview makes six toggles quick enough).

## Alternatives considered

A single "readout" select (one metric after the bars). Simplest to explain, and what the
request literally describes, but it leaves "Value + label" in place as a second, overlapping
way to print numbers in the combined item, and the Values style (four numbers) cannot be
expressed with one readout. Making every printed number a readout and allowing several removes
the overlap.

Keeping one select per module and adding modes ("Bar + value", "Watts and temp"). Every new
combination multiplies the options; this is how the select got to eight entries.

SF Symbols for the glyphs, drawn through AppKit into the bitmap. Better glyphs for free, but
the renderer is tiny-skia into one pixmap and testable without AppKit; three hand-drawn glyphs
keep it that way.

## Verification

Rust: schema tests for defaults, validation (`ItemMode::allowed_for`, every key present), old
settings file decode, `menu_bar_shows` per readout (Disk readout does not mark Disk); tray
model tests for each readout (value, gap, pause, absent, disabled module), boot-volume
resolution, readout order, accessibility words; renderer tests for glyph geometry and steady
width; `dump_tray_rows` images checked by eye.

Frontend: settings-patch tests for presets and patches; Menu bar panel tests (toggle writes a
one-key patch, disabled row copy, preview reflects settings); onboarding preset test; e2e
settings flow. Screenshots of the panel next to the Modules and Alerts panels, light and dark,
and the dev gallery `TrayPreview` section.

## Review outcome

A devil's-advocate UX review (2026-10-08) raised these; all are adopted unless noted.

1. Width. Everything on is roughly 330 pt in one image, and macOS hides a status item that
   does not fit beside the notch, taking the bars with it. No hard cap. The panel estimates
   the combined item's width from the same geometry the preview draws and, past 150 pt, shows
   one line under the preview: "Wide items can end up hidden behind the camera on MacBooks
   with a notch." Tested at the threshold.
2. Disk meaning. The separate Disk item's option reads "Read + write rate"; Network's reads
   "Total rate" and Power's "Watts". Other modules keep "Value".
3. The preview scrolling away. The preview is sticky at the top of the Menu bar panel while
   the page scrolls.
4. The combined item disappearing. When bars and values are all off and a separate item is
   on, the real combined item goes away (`build()`); the preview mirrors that rule, with a
   panel test.

Concerns, as applied: disabled copy uses the Settings module names ("Power & Sensors"), never
the `Readout::module()` enum; the Bars group gets one line under it instead of per-segment
copy; a row whose module is off shows its switch off (the stored value is kept underneath),
matching how the Modules panel shows an off module; the Temperature row reads "Hottest
temperature"; live values in rows are not in a live region, each switch is named ("Show power
in the menu bar") and the markers are `aria-hidden`; markers in rows are drawn in the muted
foreground so "CPU  CPU" does not stutter.

Changed after review by the user: no thermometer glyph; temperature stays a bare "61°".

Accepted as is: bars stay unlabelled (true today; the preview shows which bars are on).
Stacked 6.5 pt labels are kept over the inline "CPU 18%" the request sketched, because they
match the Values style and cost a third of the width; their 1x legibility is checked in the
`dump_tray_rows` images. Disk throughput in the combined item is removed on purpose; it stays
available as the separate Disk item.
