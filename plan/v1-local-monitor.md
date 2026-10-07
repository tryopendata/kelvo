# v1.x: Local monitor

v1 is the first version people install. It covers one Mac, the one Kelvo runs on: a menu bar item, a popover, a dashboard window with an Overview, module pages, a Timeline backed by persistent history, Settings and onboarding. It ships in three installable steps, v1.0, v1.1 and v1.2.

Related docs: [architecture.md](architecture.md) for the system design every version shares, [design-system.md](design-system.md) for tokens, components and chart rules, [decisions.md](decisions.md) for why things are the way they are, and [PROGRESS.md](PROGRESS.md) for status. Task checkboxes in this doc are the progress tracker for v1; tick them as work lands and log the phase in PROGRESS.md.

## Contents

1. [Goal and user value](#1-goal-and-user-value)
2. [Problem and jobs to be done](#2-problem-and-jobs-to-be-done)
3. [Scope](#3-scope)
4. [Experience](#4-experience)
5. [Architecture changes](#5-architecture-changes)
6. [Data and schema](#6-data-and-schema)
7. [Performance budget](#7-performance-budget)
8. [Milestones and tasks](#8-milestones-and-tasks)
9. [Success criteria](#9-success-criteria)
10. [Risks and open questions](#10-risks-and-open-questions)
11. [Infrastructure v1 lays for later versions](#11-infrastructure-v1-lays-for-later-versions)

## 1. Goal and user value

v1 answers two questions for an Apple Silicon developer: what is my Mac doing right now, and what was it doing at 3pm when the fans spun up. The first is the menu bar and the popover. The second is the Timeline, which keeps 30 days of history on disk and shows which processes were on top at any moment.

The user gets this without paying for it in CPU. Kelvo measures its own overhead the same way the benchmark does and prints it in the popover footer ("kelvo 0.4% cpu"). The target is to cost no more than Stats with the same modules enabled, and under 0.5% CPU across the app and its WebKit helpers.

The user also gets honest data. Sensor names that Apple does not document stay as their raw keys ("PMU tdie4"), gaps in history are drawn as gaps, and a chip Kelvo does not know gets a clear "not mapped yet" state with a way to send a sensor dump.

| Version | What the user can do after installing it |
|---|---|
| v1.0 | Glance at CPU, GPU, memory and SoC temperature in the menu bar; open a popover with every module; open a dashboard with Overview, nine module pages and Settings; scroll the last 1 or 24 hours on the Timeline with the top processes at any minute |
| v1.1 | Look back 7 and 30 days, scan a 30-day hourly heatmap, export CSV, choose the graph and per-core tray styles, split modules into separate menu bar items |
| v1.2 | See which process is using the network and the GPU, get Timeline annotations that explain events ("Fans ramped up · kernel_task + Xcode build"), and get a notification for a sustained hot process or thermal pressure |

## 2. Problem and jobs to be done

People on Apple Silicon cannot answer "what made my fans spin up at 3pm?" with the tools they have. Stats is broad but keeps no history, costs noticeable CPU itself (Stats issues #2351, #606, #2901), and mislabels sensors on M3, M4 and M5. iStat Menus 7's redesign alienated long-time users. macmon and asitop read the best Apple Silicon data but are terminal tools with no history.

The first users are developers who already run macmon or asitop. They care about P and E cluster behavior, package watts, ANE use and thermal state, and they will notice if the monitor itself shows up in the top five.

| Job | Statement | v1 coverage |
|---|---|---|
| J1 Glance | When I am working, I want to see at a glance whether my Mac is under load, so I can tell if a slowdown is the machine or the app | Tray (v1.0), popover (v1.0), sidebar live values (v1.0) |
| J2 Look back and attribute | When something happened earlier (fans, heat, battery drain), I want to see what the machine and its processes were doing at that moment, so I can find the cause | Timeline 1h and 24h with process tooltip (v1.0), 7d, 30d and heatmap (v1.1), CSV (v1.1), annotations and per-process network/GPU (v1.2) |
| J3 Trust the numbers | When I read a value, I want to know it is accurate, labelled honestly and not distorted by the monitor's own cost | Self-overhead readout (v1.0), accuracy and overhead benchmarks (v1.0), raw sensor names and unknown-chip state (v1.0) |
| J4 Ambient display | I want metrics on my desktop without opening anything | Not in v1; v2.1 desktop widgets, v2.2 WidgetKit |
| J5 Watch my servers | I want the same view for my Linux boxes and other Macs | Not in v1; v4 |
| J6 Be told | When something sustained goes wrong, I want a notification, so I do not have to watch | First built-in alert (v1.2); editor in v2.3 |

## 3. Scope

Every Must traces to a job. "Should" items ship in the version if they fit; otherwise they move to the next minor version without a decision entry. "Won't" items need a decision entry to come back.

### v1.0

| Priority | Item | Job |
|---|---|---|
| Must | Tray: combined item and values style, template images, accessibility label, 1 s or 2 s on battery | J1 |
| Must | Popover: header, CPU, Cores, Memory, GPU, Power, Network, Battery cards, footer with Open dashboard, Activity and self-CPU readout; light and dark | J1, J3 |
| Must | Dashboard shell with sidebar live values | J1 |
| Must | Overview: machine header and six module cards | J1 |
| Must | Module pages: CPU, GPU, Memory, Power & Sensors, Network, Disk, Battery, Processes | J1, J2 |
| Must | Quit and Force Quit a process from the Processes page, with confirmation and a refuse list (D-029) | J2 |
| Must | Timeline: 1h and 24h, stacked lanes, synced crosshair with top processes, sleep bands, back and Live controls | J2 |
| Must | Persistent tiered history (10 s for 24 h, 1 min for 7 days, 15 min for the rest of 30 days; D-076), gap rows, process snapshots | J2 |
| Must | Settings (modules, sampling, units, general) plus an update-check toggle | J1, J3 |
| Must | Onboarding | J1 |
| Must | States: empty history, sleep gap, unsupported chip with sensor dump, module not present, paused, stale | J2, J3 |
| Must | Benchmarks: perf gates for allocations, OS calls, store volume, frontend main-thread time and whole-app CPU; history size fill test | J3 |
| Moved | Distribution (ad-hoc signed DMG, Homebrew tap, updater), overhead vs Stats and accuracy vs macmon: the v1.x release phase after v1.2 and manual QA (D-078) | all |
| Must | All nine "Infrastructure laid in v1" items from architecture.md, plus the serializable widget prop contract | later versions |
| Should | Popover interval pill opens a 0.5/1/2/5 s menu | J1 |
| Should | Keyboard shortcuts ⌘1 to ⌘9 for sidebar entries | J1 |
| Should | "Show as table" alternative per Timeline lane | J2 (accessibility) |
| Won't | Intel Macs (IOReport, SMC sensor maps and the power model are Apple Silicon only) | |
| Won't | Quitting processes owned by another user (no privilege escalation; D-029) | |
| Won't | Disk card in the popover (Disk lives in the dashboard) | |
| Won't | Overview "Customize" button (v2.3) and sidebar Widgets entry (v2.0) | |
| Won't | Any telemetry beyond the opt-out update check | |

### v1.1

| Priority | Item | Job |
|---|---|---|
| Must | Timeline 7d and 30d with min/max envelopes | J2 |
| Must | 30-day hourly heatmap, Avg CPU or Temperature, click opens that hour | J2 |
| Must | Export CSV of the visible Timeline range | J2 |
| Must | Tray styles: graphs, cores + histogram | J1 |
| Must | Per-module menu bar items with ⌘-drag reorder | J1 |
| Should | Onboarding "Graph per module" option | J1 |

### v1.2

| Priority | Item | Job |
|---|---|---|
| Must | Per-process network rates via in-process NetworkStatistics, sampled only while visible | J2 |
| Must | Per-process GPU time (IORegistry client `accumulatedGPUTime`, unverified) | J2 |
| Must | Event detectors and Timeline auto-annotations: fans ramped, thermal state change, sustained process, ANE or power spike, each with attribution | J2 |
| Must | First alert rule: sustained hot process or thermal pressure, as a macOS notification | J6 |
| Should | Power page chart annotations from power-spike events ("ANE 1.4 W · Photos face analysis") | J2 |
| Should | Battery chart annotation for optimized charging ("held at 80%") | J2 |

## 4. Experience

### 4.1 Journeys

First launch (v1.0). The user opens Kelvo.app from the DMG and follows the documented Gatekeeper step (unsigned app). The onboarding window appears with detected modules switched on, the chip status in the footer ("M4 Pro detected · all sensors mapped"), and the Combined menu bar style preselected. Continue applies the choices and shows step 2 (updates and privacy). The tray item appears and starts updating. The onboarding window closes; no dashboard opens unless the user clicks the tray.

Glance (v1.0). The tray icon shows three bars and the hottest SoC zone. A click opens the popover under the status item without stealing focus. The user scrolls the cards, clicks Open dashboard or Activity, or clicks outside to dismiss.

Investigate a fan spike (v1.0, improved in v1.2). The user opens the dashboard, picks Timeline, chooses 24h, and sees a CPU peak at 14:02 lining up with a temperature rise. Hovering the peak shows the values at that minute and the top processes then (`swift-frontend (Xcode)` at 412%). In v1.2 the same moment has an annotation pill, "14:02 Fans ramped up · kernel_task + Xcode build", and the user can get a notification next time.

Unknown chip (v1.0). On a Mac whose sensors Kelvo does not map yet, Power & Sensors is hidden, CPU and GPU watts still show on their pages, and a notice offers "Share sensor dump". The user reviews the JSON in a sheet and opens a prefilled GitHub issue.

### 4.2 Tray

The status item image is drawn in Rust as a template `NSImage`. The full geometry and the decision not to tween are in [design-system.md, Tray icon spec](design-system.md#tray-icon-spec).

Which elements appear is driven by each module's "Menu bar" setting in Settings. There is no separate global style setting: Combined is the state where CPU, GPU and Memory are "In combined item"; Values is the state where they are "Value + label". Onboarding presets these per-module modes. Network, Disk and Battery can only be "Value + label" or "Hidden" in v1.0. All visible elements render into one status item in v1.0; separate items per module arrive in v1.1.

| Element | Data | Cadence | Source |
|---|---|---|---|
| CPU bar | `cpu.total` | base tick (1 s, 2 s on battery) | live bus |
| GPU bar | `gpu.util` | base tick | live bus |
| Memory bar | `mem.pressure` | base tick | live bus |
| Temperature text "61°" | `thermal.hottest` | every 5 ticks (D-055) | live bus |
| Values text | `cpu.total`, `gpu.util`, `mem.pressure`, `thermal.hottest`, `power.system`, `net.rx` + `net.tx` summed over interfaces, `disk.read` + `disk.write`, `battery.charge` | as above | live bus |
| Accessibility label | same values in words | every redraw | live bus |

Clicking the item toggles the popover. Right-click or Control-click opens a native menu: Open dashboard, Settings, Pause sampling, Quit Kelvo. On display sleep and screen lock the image stops updating. When sampling is paused, the bars drop to their tracks and the text shows "–".

### 4.3 Popover

A 360 × 680 pt non-activating panel with a vibrant surface (see [design-system.md, Vibrant surfaces](design-system.md#vibrant-surfaces)). The header is fixed; the card column scrolls with an overlay scrollbar, and the header gains a bottom border once scrolled. The footer is fixed. Cards appear in module order and only for enabled modules that the host has. On open, Rust sends a 60 s backfill from the ring buffer before streaming, so charts are full on the first frame.

| Region | Element | Data | Cadence | Source |
|---|---|---|---|---|
| Header | "Kelvo", "MacBook Pro · up 3d 4h" | `HostRecord.display_name`, `HostInfo.boot_time_ms` | on open, then every 60 s | `get_host` command |
| Header | Interval pill "1s" with live dot | effective base interval, paused and on-battery flags | on change | `LiveMsg::Status` |
| Header | Pause button | toggles sampling; writes a gap with reason `paused` | on click | `set_paused` command |
| Header | Settings button | opens dashboard at `/dashboard/settings` | on click | `open_dashboard` command |
| CPU card | Headline "18%", area chart, 60 s, autoscaled ceiling label "40%", "60s" | `cpu.total` series, last 60 samples | base tick | backfill + live |
| CPU card | User and System rows with bars | `cpu.user`, `cpu.system` | base tick | live |
| Cores card | Tiles per core grouped by cluster, cluster label with GHz | `cpu.load{core}`, `cpu.cluster.freq{cluster}` | base tick | live |
| Memory card | "17.6 / 24 GB" | `mem.used`, `HostInfo.mem_total_bytes` | base tick | live |
| Memory card | Pressure bar with state word ("normal") | `mem.pressure`, `mem.pressure_level` | base tick | live |
| Memory card | Composition bar and legend: App, Wired, Compressed, Cached files, Free, Swap | `mem.app`, `mem.wired`, `mem.compressed`, `mem.cached`, `mem.free`, `mem.swap_used` | base tick | live |
| GPU card | Headline, area chart 60 s, FREQ, POWER, CORES | `gpu.util` series, `gpu.freq`, `power.gpu`, `HostInfo` GPU core count | base tick | backfill + live |
| Power card | "14.8 W system", stacked bar CPU, GPU, ANE (hatched), DRAM, remainder as track | `power.system`, `power.cpu`, `power.gpu`, `power.ane`, `power.dram` | base tick | live |
| Network card | Interface label "Wi‑Fi · en0", UPLOAD and DOWNLOAD values, mirrored bars for 48 s | `net.tx{iface}`, `net.rx{iface}` for the primary interface | base tick | backfill + live |
| Battery card | Charge bar and "87%", REMAINING, HEALTH, CYCLES | `battery.charge`, `battery.time_remaining`, `battery.health`, `battery.cycles` | every 10 ticks | live |
| Footer | Open dashboard (cyan CTA) | opens or focuses dashboard at last route, else Overview | on click | `open_dashboard` |
| Footer | Activity | opens dashboard at `/dashboard/processes` | on click | `open_dashboard` |
| Footer | "kelvo 0.4% cpu" | `self.cpu`, 60 s average | every 10 ticks | live |

The primary interface is the one carrying the default route (unverified how to read it cheaply; `SCDynamicStore` `State:/Network/Global/IPv4` is the likely source). The memory legend uses Activity Monitor's terms. The original popover mock labelled the second segment "Active" while the Overview mock called the same 11.6 GB "App"; v1 uses "App" in both places so the numbers reconcile (App + Wired + Compressed = Used).

### 4.4 Dashboard shell and sidebar

A standard window with an overlay title bar so the traffic lights sit over the 220 px sidebar. Sidebar groups: Overview and Timeline; the seven modules with live values; Processes and Settings (Widgets appears in v2.0). A module the host does not have (Battery on a Mac mini) is not listed. A module the user disabled is listed dimmed without a value. Footer: live dot plus "Sampling every 1s", "Sampling every 2s · on battery" or "Paused", and the app version.

| Element | Data | Cadence | Source |
|---|---|---|---|
| CPU value "18%" | `cpu.total` | base tick | live |
| GPU value "36%" | `gpu.util` | base tick | live |
| Memory value "42%" | `mem.pressure` | base tick | live |
| Power & Sensors "14.8W" | `power.system` | base tick | live |
| Network "38.4M" | sum of `net.rx` and `net.tx` over interfaces, MB/s | base tick | live |
| Disk "220M" | sum of `disk.read` and `disk.write` over devices, MB/s | base tick | live |
| Battery "87%" | `battery.charge` | every 10 ticks | live |
| Footer | effective interval, paused, on battery | on change | `LiveMsg::Status` |

The original sidebar mock showed the download rate for Network and the read rate for Disk. v1 shows the sum of both directions, because a sidebar value reads as "how busy is this", and a write-heavy disk would otherwise show near zero.

### 4.5 Overview

The page header has the title, a "Live" status pill, and (from v2.3) the Customize button. The machine header shows an illustration, the model name with macOS version and build, and a two-column spec grid. Below, a three-column `CardGrid` holds CPU, GPU, Memory, Power & Sensors, Network and Disk. Each card is a link to its module page. Each card's process list needs process data, so the Overview registers process interest while visible and processes are sampled every tick.

Machine header data: `HostInfo` (model name, marketing year, chip, CPU and GPU core counts, memory size and type, model identifier), `disk.total` and `disk.free` for the boot volume, `battery.charge`, `battery.health`, `battery.cycles`, `HostInfo.boot_time_ms` for uptime, and the last `DidWake` time from the `gaps` table. Static fields load once; battery and storage update every 60 s.

| Card | Ring (center, segment 1, segment 2) | Bar 1 | Bar 2 | Legend | Top-5 list |
|---|---|---|---|---|---|
| CPU | `cpu.total` "LOAD"; `cpu.user`; `cpu.system` | P-cluster `cpu.cluster.freq` ÷ cluster max | E-cluster, same | User, System | processes by %CPU |
| GPU | `gpu.util` "LOAD"; `gpu.render`; `gpu.tiler` | `gpu.freq` ÷ max GPU frequency | `power.gpu` ÷ 24 h max (floor 5 W) | Render, Tiler | v1.0: a 60 s `gpu.util` StreamArea, because per-process GPU time arrives in v1.2. v1.2: processes by GPU time |
| Memory | `mem.used` "GB USED"; `mem.app` ÷ total; (`mem.wired` + `mem.compressed`) ÷ total | `mem.pressure` | `mem.swap_used` ÷ swap allocated | App, Wired + comp. | processes by memory footprint |
| Power & Sensors | `power.system` "WATTS"; `power.cpu` ÷ system; (`power.gpu` + `power.dram`) ÷ system | `thermal.hottest` ÷ 105 °C | `fan.rpm` (max of fans) ÷ `fan.max` | CPU, GPU + DRAM | processes by energy impact |
| Network | `net.rx` ÷ `net.link_rate` "OF LINK" | `net.rx` ÷ link | `net.tx` ÷ link | Down, Up in Mb/s | v1.0: interfaces by total rate. v1.2: processes by rate |
| Disk | boot container used ÷ size "USED"; data volume; system volume | `disk.read` ÷ 24 h max (floor 500 MB/s) | `disk.write`, same | Data, System in GB | processes by disk I/O |

Cadence for every card is the base tick from the live bus, except the 24 h maxima (one `query_history` call on mount, refreshed every 10 minutes) and disk capacity (60 ticks). The subtitle on each card is static host data ("10P + 4E", "20-core", "24 GB", "Wi‑Fi 7 · en0", "APPLE SSD · 1 TB") except Power & Sensors, which shows "on battery" or "on power adapter" from `battery.external`.

Two items differ from the original mocks. The Overview mock's GPU legend said "Render 28% / Compute 8%"; Apple's IOAccelerator performance statistics expose renderer and tiler utilization, not a compute split (unverified), so v1 labels what it can measure. And on a Mac without fans (MacBook Air), the Fans bar is replaced by "Passive cooling".

### 4.6 Timeline

Header: title, subtitle "Last 24 hours, ending Sun Oct 4 · 22:40", range control (1h and 24h in v1.0; 7d and 30d appear in v1.1), a back button that shifts the window by one range length, a forward button that appears once you have gone back, Live (returns to the window ending now and resumes following), and Export CSV (v1.1).

Lanes, top to bottom: CPU, GPU, Memory, Power, Temperature, Network. Each lane has a label column (name with swatch, current value, a sub line) and a plot. Lanes share one x axis with hour ticks and "now" at the right edge. The crosshair follows the pointer across all lanes, puts a dot on each series, and shows one tooltip. Above the plot, an annotation row shows Sleep and Wake markers from gap rows in v1.0, and event annotations in v1.2.

| Lane | Series | Domain | Sub line |
|---|---|---|---|
| CPU | `cpu.total` avg with min/max envelope | 0 to 100% | "peak 71% · 14:02" (max in range and its time) |
| GPU | `gpu.util` | 0 to 100% | "avg 27% · 1.1 GHz" (range avg, avg `gpu.freq`) |
| Memory | `mem.pressure` | 0 to 100% | "pressure · 24 GB" |
| Power | `power.system` faint, `power.cpu` solid | 0 to nice ceiling | "CPU solid · rest faint" |
| Temperature | `thermal.hottest` | 30 to 100 °C | "hottest SoC zone" |
| Network | `net.tx` summed above, `net.rx` summed below | nice ceiling each side | "↑ above · ↓ below" |

Data by range:

| Range | History source | Buckets drawn | Live tail |
|---|---|---|---|
| 1h | `tier_10s` | 360 | ring buffer from the last closed 10 s bucket, aggregated to 10 s |
| 24h | `tier_1m` (`auto`: `tier_15m` for a day older than 7 days, 96 buckets) | 1,440 | ring buffer from the last closed minute, aggregated to 1 min |
| 7d (v1.1) | `tier_1m`, downsampled server-side to 2 points per plot pixel | about 2,000 | same as 24h |
| 30d (v1.1) | `tier_15m` with the last 7 days' minutes folded into the same 15-minute slots (`auto`) | 2,880 | same as 24h |

Minutes older than 7 days are rolled down into 15-minute buckets (D-076), so any range that starts before the 7-day line reads at 15-minute resolution; requests use `auto` rather than a fixed tier.

The writer commits every 5 minutes (D-070), so the newest up to 5 minutes are only in the ring buffer. The Timeline stitches history up to the last closed bucket with the ring tail, as architecture.md's data-flow section describes. When following Live, the view appends a bucket each time one closes (every 10 s or every minute), not every tick.

Crosshair tooltip: header "Sun Oct 4 · 14:02:00" and the resolution ("1 MIN AVG" or "10 S AVG"); one row per lane with the bucket avg, plus Fans (`fan.rpm` max) and Network down and up; then "TOP PROCESSES THEN" with values in "% of 1 core". Processes come from the `proc_snap` row nearest the cursor within 10 s when the cursor is inside the last 72 hours, otherwise from `proc_top_1m` for that minute. The query is a `query_processes_at` command, debounced to 50 ms of cursor rest and cached per bucket.

Sleep bands come from `gaps` rows in range; the label gives the reason and duration ("Asleep 5h 15m · no samples", "Kelvo not running", "Paused"). The lines stop at the band edge with a hollow dot, per the gap rule in design-system.md.

### 4.7 CPU page

Header subtitle: "Apple M4 Pro · 10 performance + 4 efficiency cores" from `HostInfo`. The 5m, 15m, 30m and 1h control sets the window for every time-based chart on every module page (`general.chart_window`, D-091); all four come from the ring buffer, which holds one hour.

| Region | Element | Data | Cadence | Source |
|---|---|---|---|---|
| Total card | Stat strip: TOTAL hero, USER, SYSTEM, IDLE, LOAD AVG (1, 5, 15 min) | `cpu.total`, `cpu.user`, `cpu.system`, 100 minus total, `cpu.loadavg{window}` | base tick; load average every 5 ticks | live |
| Total card | Area chart, total and system, 0 to 100, ticks "-60s" to "now" | `cpu.total`, `cpu.system` over the window, downsampled to 2 points per pixel for 15m and 1h | base tick | backfill + live |
| Cluster frequency | One ring per cluster: GHz, "max 4.51 · active 41%" | `cpu.cluster.freq{cluster}`, max from the DVFS table in `HostInfo`, `cpu.cluster.active{cluster}` | base tick | live |
| Cluster frequency | P-CLUSTER POWER, E-CLUSTER POWER | `cpu.cluster.power{cluster}` | base tick | live |
| Per-core load | One row per core (P0 to P9, a gap, E0 to E3), 60 columns of 10 s, current % at right; legend 0% to 100% | `cpu.load{core}` from the ring, averaged into 10 s columns; columns before app start or inside a gap are hatched | recomputed when a 10 s column closes; rightmost column updates every tick | ring |
| Cluster residency | Per cluster: "active 41%", stacked bar and a table of the top four frequency states plus idle, with any others merged into "other" | `cpu.cluster.residency{cluster,state}` averaged over the last 60 s | base tick | ring |
| Top processes | Columns: Process, PID, % CPU (bar), Threads, Idle wake-ups, Energy, User; note "% CPU is of one core; 14 cores = 1400%" | `LiveMsg::Processes` rows | every tick while visible | live |

Machines with more than one P cluster (Max and Ultra chips) get one ring per cluster and one residency block per cluster; the grid wraps. The process table shows 8 rows with a "Show all" link to the Processes page, sorted by % CPU by default, sortable by any column. Rows are keyed by `(pid, start_time)` so a reused PID does not inherit another process's row. Energy is an approximation of Activity Monitor's energy impact, documented in the column tooltip (see risk 10.1, R6).

### 4.8 GPU page

Mock: No mock. Compose from `04-overview-dark` card patterns (the GPU card's ring, bars and legend) and `07-cpu-detail` layout (stat strip over a large chart, a two-column row of detail cards, a process table at the bottom).

| Region | Element | Data | Cadence | Source |
|---|---|---|---|---|
| Header | "GPU", subtitle "Apple M4 Pro · 20-core GPU" | `HostInfo` | once | `get_host` |
| Usage card | Stat strip: UTILIZATION hero, RENDERER, TILER, FREQUENCY, POWER | `gpu.util`, `gpu.render`, `gpu.tiler`, `gpu.freq`, `power.gpu` | base tick | live |
| Usage card | Area chart over the global chart window (D-091) | `gpu.util` | base tick | backfill + live |
| Frequency card | Ring GHz of max, `ResidencyBar` of GPU frequency states over 60 s | `gpu.freq`, `gpu.residency{state}` | base tick | ring |
| Power card | 10-minute power line | `power.gpu` | base tick | ring |
| Processes | v1.0: hidden. v1.2: table of process, PID, GPU time %, user | per-process GPU time | while visible | v1.2 collector |

### 4.9 Memory page

Mock: No mock. Compose from `04-overview-dark` card patterns (the Memory card) and the popover Memory card in `14-component-popover-panel` (composition bar and legend), laid out like `07-cpu-detail`.

| Region | Element | Data | Cadence | Source |
|---|---|---|---|---|
| Header | "Memory", subtitle "24 GB unified · LPDDR5X" | `HostInfo` | once | `get_host` |
| Composition card | USED hero "17.6 / 24 GB", PRESSURE with state word, SWAP, COMPRESSED | `mem.used`, `mem.pressure`, `mem.pressure_level`, `mem.swap_used`, `mem.compressed` | base tick | live |
| Composition card | Large `StackBar` and two-column legend: App, Wired, Compressed, Cached files, Free | `mem.app`, `mem.wired`, `mem.compressed`, `mem.cached`, `mem.free` | base tick | live |
| Pressure card | Pressure line with 1m to 1h window, warn and critical thresholds as labelled gridlines | `mem.pressure` | base tick | backfill + live |
| Swap card | Swap used line, swap-in and swap-out rates | `mem.swap_used`, `mem.swap_in`, `mem.swap_out` | base tick | ring |
| Processes | Process, PID, Memory (footprint), Compressed, Threads, User, sorted by memory | `LiveMsg::Processes` | every tick while visible | live |

The total memory label is the marketing size ("24 GB") in both unit modes, because that is what the user bought; the GiB setting applies to measured values.

### 4.10 Power & Sensors page

Header subtitle: "On battery · 14.8 W system draw · 6:12 remaining" or "On power adapter · 14.8 W system draw".

| Region | Element | Data | Cadence | Source |
|---|---|---|---|---|
| CPU ring card | "61 °C", "Max of SMC CPU group", P-CLUSTER "3.2 GHz" | `thermal.cpu`, `cpu.cluster.freq{cluster=P*}` | temperature every 5 ticks (D-055), frequency every tick | live |
| GPU ring card | "54 °C", "Max of SMC GPU group", FREQUENCY | `thermal.gpu`, `gpu.freq` | temperature every 5 ticks (D-055), frequency every tick | live |
| Fans ring card | "1,850 RPM" of max, "Left 1,840 · right 1,860", MODE "Automatic" | `fan.rpm{fan}`, `fan.max{fan}`, `fan.mode` | every 2 ticks | live |
| System ring card | "14.8 W", "Drawn from battery" or "From power adapter", PACKAGE "10.4 W" | `power.system`, `battery.external`, `power.package` | base tick | live |
| SoC thermal zones | Explanation line; table ZONE, SENSOR, bar (20 to 110 °C), NOW, 10M range, hottest first | `thermal.zone{sensor}`; 10-minute min and max from the ring | every 5 ticks (D-055); order re-sorts at most every 10 s so rows do not jump | live + ring |
| SoC thermal zones | Footer: Battery, SSD (NAND), Wi‑Fi module temperatures | `thermal.sensor{name}` | every 5 ticks (D-055) | live |
| Power by component | Legend with current watts; stacked area CPU, GPU, ANE (hatched), DRAM over 10 min; "10.4 W package" | `power.cpu`, `power.gpu`, `power.ane`, `power.dram`, `power.package` | base tick | ring |
| Battery, last 24 hours | CHARGE, HEALTH, CYCLES, REMAINING, FULL CHARGE "68.2 of 72.6 Wh" | `battery.charge`, `battery.health`, `battery.cycles`, `battery.time_remaining`, `battery.capacity_wh`, `battery.design_wh` | every 10 ticks | live |
| Battery, last 24 hours | 24 hourly bars "charge at end of hour", charging marks under bars | `battery.charge` last 1 min bucket avg of each hour; `battery.charging` max in the hour | on mount, then when an hour closes | `query_history` on `tier_1m` |

The "ANE 1.4 W · Photos face analysis" annotation needs power-spike attribution, which is v1.2. The "Optimized charging: held at 80%" annotation needs a source for the optimized-charging state (unverified; `AppleSmartBattery` properties are the likely place) and is a v1.2 Should. Without a fan (MacBook Air), the Fans card shows "Passive cooling" and no ring. Without a battery, the battery section is omitted and the System card says "From power adapter".

### 4.11 Network page

Mock: No mock. Compose from `04-overview-dark` card patterns (the Network card) and the popover Network card in `14-component-popover-panel` (mirrored bars), laid out like `07-cpu-detail`.

| Region | Element | Data | Cadence | Source |
|---|---|---|---|---|
| Header | "Network", subtitle "Wi‑Fi 7 · en0 · 2.4 Gb/s link" | primary interface, `net.link_rate` | every 60 ticks | live |
| Throughput card | DOWN hero, UP, OF LINK | `net.rx`, `net.tx` summed, `net.link_rate` | base tick | live |
| Throughput card | Large `MirrorBars`, upload above and download below, with 1m to 1h window | `net.tx`, `net.rx` summed | base tick | backfill + live |
| Interfaces | Table: interface, kind (Wi‑Fi, Ethernet, VPN, bridge), down rate, up rate, received and sent since boot | `net.rx{iface}`, `net.tx{iface}`, counters from the collector | base tick | live |
| Processes | v1.0: hidden. v1.2: process, PID, down, up, total, sorted by total | per-process network | while visible | v1.2 collector |

No IP addresses are shown; v1 has no use for them and they are personal data in screenshots.

### 4.12 Disk page

Mock: No mock. Compose from `04-overview-dark` card patterns (the Disk card) and the `MirrorBars` pattern from the popover, laid out like `07-cpu-detail`.

| Region | Element | Data | Cadence | Source |
|---|---|---|---|---|
| Header | "Disk", subtitle "APPLE SSD · 1 TB" | `HostInfo` and IORegistry model string | once | `get_host` |
| Throughput card | READ hero, WRITE, and a mirrored chart (read above, write below) with 1m to 1h window | `disk.read{dev}`, `disk.write{dev}` summed | base tick | backfill + live |
| Volumes | Table: volume, used, free, size, usage bar; boot container first | `disk.used{vol}`, `disk.free{vol}`, `disk.total{vol}` | every 60 ticks | live |
| Processes | Process, PID, read rate, write rate, total, sorted by total | `LiveMsg::Processes` disk columns | every tick while visible | live |

### 4.13 Battery page

The page reuses the Power & Sensors "Battery, last 24 hours" section as-is and composes the rest from the Power & Sensors ring cards and the CPU page layout.

| Region | Element | Data | Cadence | Source |
|---|---|---|---|---|
| Ring row | Charge ring with "Charging" or "On battery"; health ring with condition word; power ring with adapter watts or discharge rate | `battery.charge`, `battery.charging`, `battery.health`, `battery.power`, `battery.external` | every 10 ticks | live |
| Details | Cycles, full charge and design capacity, temperature, time to full or time remaining | `battery.cycles`, `battery.capacity_wh`, `battery.design_wh`, `battery.temp`, `battery.time_remaining` | every 10 ticks | live |
| History | The Power & Sensors battery section | as in 4.10 | as in 4.10 | `tier_1m` |

On a Mac without a battery, the page does not exist and the sidebar omits the entry.

### 4.14 Processes page

Mock: No mock. Compose from the `07-cpu-detail` "Top processes" table, full height, with a search field and a column-set control in the header.

| Element | Data | Cadence | Source |
|---|---|---|---|
| Search field | filters by name or PID, client-side | on input | n/a |
| Column set control (`SegmentedControl`: CPU, Memory, Energy, Disk; Network from v1.2) | chooses default sort and visible columns | on click | n/a |
| Table, virtualized: Process, PID, % CPU, Memory, Threads, Idle wake-ups, Energy, Disk read, Disk write, User | `LiveMsg::Processes` with every process, not just the top N | every tick while visible | live |
| Footer | process count, thread count | same | live |
| Row action and context menu: Quit, Force Quit | selected row's `pid` and `start_time` | on click | `process_signal` command |

While the pointer is over the table, row order is frozen and values keep updating in place, so a row does not move under the cursor. Order resumes when the pointer leaves.

Quit and Force Quit (D-029) appear as a row action on hover and in the row's context menu. Quit asks the process to exit: `NSRunningApplication.terminate` when the process is a regular app, `SIGTERM` otherwise. Force Quit uses `forceTerminate` or `SIGKILL`. Both open a confirm dialog naming the process and PID; the Force Quit dialog also says that unsaved data in that process will be lost. The actions are disabled, with a tooltip saying why, for PID 1 (`launchd`), `kernel_task`, `WindowServer` and Kelvo's own processes, and Rust refuses them again regardless of what the UI sends. A process owned by another user fails with `EPERM`; the UI shows a toast ("Kelvo can't quit processes owned by another user") and does not offer to escalate. If the process has already exited, or its PID now belongs to a different process, the command reports that and the table drops the row on the next tick.

### 4.15 Settings

| Section | Row | Control | Effect |
|---|---|---|---|
| Modules | One row per module: swatch, name, Menu bar select, On switch | Select options for CPU, GPU, Memory: In combined item, Value + label, Hidden. Power & Sensors: Temp in combined, Watts as value, Hidden. Network, Disk, Battery: Value + label, Hidden | Off stops the module's collectors, removes it from tray, popover and Overview, and makes its series end (drawn as a gap). Modules the host lacks are listed disabled with "Not present on this Mac" |
| Sampling | Sample interval | `SegmentedControl` 0.5s, 1s, 2s, 5s, 10s, 30s, 60s, with "Kelvo uses about 0.4% CPU at 1s" | Reconfigures the ticker. The sentence uses the measured `self.cpu` 10-minute average, not a constant. Live chart windows scale with the interval (D-059) |
| Sampling | Slow down on battery | Switch, "to 2s" (twice the interval, at most 60s) | Base tick doubles on battery, capped at 60 s (D-061) |
| Sampling | Keep history | Select: 7, 30, 90 days, each with its projected size | Sets how long history is kept (`tier_15m` beyond the 7 days of `tier_1m`, gaps, events; D-076). When the size limit cuts it short: "Limited to about N days by the X MB limit" (D-059) |
| Sampling (not in mock) | History size limit | Select: 150 MB, 300 MB, 500 MB, 1 GB | Oldest history is trimmed to stay under it; a change prunes immediately (D-057, D-059). Low-disk and trim notices appear under the card |
| Sampling | History on disk | Size "148 MB" and Clear | Clear asks for confirmation, deletes the host's rows, vacuums, and the empty-history state appears |
| Units | Temperature | °C, °F | Display only |
| Units | Network rate | MB/s, Mb/s | Display only |
| Units | Memory | GB, GiB | Display only |
| General | Launch at login | Switch | `SMAppService` register or unregister |
| General | Show in Dock | Switch | Activation policy Regular or Accessory |
| General | Appearance | Select: Match system, Light, Dark | Sets theme in every window |
| General (not in mock) | Check for updates automatically | Switch, on by default | The only network request Kelvo makes; off means no request at all |
| General (not in mock) | Version and "Check now" | Text and button | Runs the updater check once |

Data: `get_settings`, `update_settings`, `settings-changed`, `history_size` (refreshed every 60 s while visible), `clear_history`, `history_health` and `history-health-changed`, and `self.cpu` from the live bus.

### 4.16 Onboarding

A fixed-size window (820 × 566 pt) shown on first run only. The modules list comes from `Capabilities`: present modules are switched on (Disk off by default), absent modules are shown disabled with "Not present on this Mac". Menu bar style options are cards with a live `TrayPreview` drawn from current values: Combined (Recommended), Values only, and in v1.1 Graph per module. Choosing a style sets the per-module menu bar modes described in 4.2. The footer chip status reads "M4 Pro detected · all sensors mapped" or "Mac17,4 detected · sensors not mapped yet". Launch at login is checked by default.

Skip applies the defaults and closes. Continue applies the choices and shows step 2 of 2, "Updates and privacy", which is not mocked. It has one switch, "Check for updates automatically" (on), with one sentence saying it is the only network request Kelvo makes and can be turned off any time in Settings. Below it, a plain statement: Kelvo has no telemetry, and all history stays on this Mac, with the history location (`~/Library/Application Support/com.tryopendata.kelvo/`) and its expected size ("about 150 MB for 30 days"). A Done button closes the window. Step 2 follows the step 1 layout and controls.

Data: `get_capabilities`, `get_host`, live values for the previews, `history_size` and the retention setting for the size line, `update_settings` on Continue and Done.

### 4.17 States

| State | Where | Trigger | Presentation |
|---|---|---|---|
| Empty history | Timeline, history charts on module pages | Recorded span in the range is under 25% of the range (proposed threshold) | Dashed baselines, the recorded tail drawn at the right edge, overlay "Collecting · timeline fills in as you work" and "First 4 min recorded. History is kept for 30 days on this Mac only."; header right "started 22:36 · 1 sample/s" |
| Sleep gap | Every history chart | `gaps` row with reason `sleep` | Hatched band, label "Asleep 11:02–11:31 · not interpolated", hollow dots at both edges |
| Other gaps | Every history chart | reasons `app_not_running`, `paused`, `module_disabled` | Same band, label "Kelvo not running", "Paused", "CPU sampling off" |
| Unknown chip | Overview, sidebar, Power & Sensors | `HostInfo.chip_known == false` or sensor collector `Unsupported(UnknownChip)` | Power & Sensors hidden; the other modules shown; notice with info icon: "Temperature and fan sensors aren't mapped for this chip (Mac17,4) yet, so Power & Sensors is hidden. CPU and GPU watts still show on their own pages." and "Share sensor dump" |
| Module not present | sidebar, popover, Overview | `ModuleCap::NotPresent` | Entry omitted; Overview reflows to the remaining cards |
| Not available in this edition | same | `ModuleCap::Unsupported(MissingEntitlement)` (appstore builds only) | Card with "Not available in this edition" |
| Series disappeared | any chart | capability change removed a series (disk ejected, eGPU removed) | Gap from that point; the row in tables shows "Disconnected" until the next layout without it |
| Paused | popover, sidebar, tray | user paused | Status pill "Paused", charts stop scrolling, tray bars at track |
| Stale | popover, dashboard | no frame for 3 × interval | Status pill "Stale", values at 50% opacity; clears on the next frame |
| History unavailable | dashboard | store failed to open or write (disk full, corruption) | Banner on Timeline and history charts: "History is unavailable: disk is full. Live values still work." with "Reset history" when the file is corrupt |
| Sensor read failed | one card | collector error on consecutive ticks | Card shows "Sensor read failed" and the time of the last good value; other cards unaffected |

Share sensor dump opens a sheet with the JSON Kelvo collected: model identifier, chip string, macOS version, SMC key names with types and current values, HID sensor product names, IOReport group and channel names, fan count. No serial numbers, user names, host names or network identifiers. Buttons: Copy, Save…, and Open GitHub issue, which opens a prefilled issue template in the browser asking the user to paste the JSON (a full dump is too long for a URL).

## 5. Architecture changes

v1.0 builds the system described in [architecture.md](architecture.md) from the scaffold. This section lists what each minor version adds and the deltas this doc made to architecture.md.

| Area | v1.0 | v1.1 | v1.2 |
|---|---|---|---|
| Workspace | Five crates plus `src-tauri`, dependency direction enforced by Cargo | | |
| Collectors | CPU, IOReport (CPU, GPU, energy), memory, network, disk I/O and capacity, SMC/HID sensors, battery, processes, self CPU, thermal state; Linux stub | | NetworkStatistics per-process network, IORegistry per-process GPU, both `Cadence::OnDemand` |
| Engine | GCD ticker, PowerSignals, sampler, ring buffer, S10 and M1 accumulators, back-off, sleep gaps, bus, capability re-probe, process interest | | Detectors, alert evaluation, notification sink |
| Store | Full DDL including `events`, writer, queries, pruning, cursors, fill test | Hour-of-day aggregate query for the heatmap, CSV streaming reader | Event writes and range queries |
| App shell | HostRegistry, LocalSource, settings owner, tray (one item), popover panel, dashboard and onboarding windows, live channels, updater, launch at login | Tray styles and multiple status items | Notification permission and delivery |
| Frontend | Theme, fonts, transport, stores, router, chart primitives, all v1.0 screens | Long-range Timeline, CalendarHeatmap, export | AnnotationRow events, per-process columns |

Deltas this plan made to architecture.md, now adopted there:

- The `gaps.reason` values gain `paused` and `module_disabled`, and `gaps` gains a nullable `module` column (NULL means the whole host) so a disabled module does not blank other modules' charts.
- `LiveMsg` carries `Status` (interval, paused, on battery, Performance mode reason) and `Processes` messages in addition to frames, layouts and capabilities (see 6.3).
- Pause writes a gap and stops collectors rather than only freezing the UI, so history shows that nothing was measured.
- The battery collector cadence is every 10 ticks.

## 6. Data and schema

### 6.1 Metric catalog for v1

This is the v1 content of `kelvo-schema::CATALOG`. IDs are stable; renaming one is a decision entry. "Persisted" means the series goes to `tier_10s` and `tier_1m`; non-persisted series live only in the 1 s ring buffer.

| metric_id | Labels | Unit | Kind | Cadence (base ticks) | Persisted | Source | Entitlement |
|---|---|---|---|---|---|---|---|
| `cpu.total` | | % | gauge | 1 | yes | `host_processor_info` | None |
| `cpu.user` | | % | gauge | 1 | yes | same | None |
| `cpu.system` | | % | gauge | 1 | yes | same | None |
| `cpu.load` | `core` (P0…, E0…) | % | gauge | 1 | yes | same, per core | None |
| `cpu.loadavg` | `window` (1, 5, 15) | count | gauge | 5 | yes | `getloadavg` | None |
| `cpu.cluster.freq` | `cluster` (P0, P1, E0) | Hz | gauge | 1 | yes | IOReport CPU Stats residency-weighted | IoReport |
| `cpu.cluster.active` | `cluster` | % | gauge | 1 | yes | IOReport CPU Stats | IoReport |
| `cpu.cluster.residency` | `cluster`, `state` (MHz or `idle`) | % | gauge | 1 | no | IOReport CPU Stats | IoReport |
| `cpu.cluster.power` | `cluster` | W | gauge | 1 | yes | M3 Max: SMC P-cluster keys scaled to PMP, no E cluster (D-054); other chips: IOReport Energy Model | SmcUserClient or IoReport |
| `gpu.util` | | % | gauge | 1 | yes | IOAccelerator PerformanceStatistics "Device Utilization %" | None |
| `gpu.render` | | % | gauge | 1 | yes | "Renderer Utilization %" | None |
| `gpu.tiler` | | % | gauge | 1 | yes | "Tiler Utilization %" | None |
| `gpu.freq` | | Hz | gauge | 1 | yes | IOReport GPU Stats | IoReport |
| `gpu.residency` | `state` | % | gauge | 1 | no | IOReport GPU Stats | IoReport |
| `mem.used` | | bytes | gauge | 1 | yes | `host_statistics64` | None |
| `mem.app` | | bytes | gauge | 1 | yes | same (internal minus purgeable) | None |
| `mem.wired` | | bytes | gauge | 1 | yes | same | None |
| `mem.compressed` | | bytes | gauge | 1 | yes | same | None |
| `mem.cached` | | bytes | gauge | 1 | yes | same (external pages) | None |
| `mem.free` | | bytes | gauge | 1 | yes | same | None |
| `mem.pressure` | | % | gauge | 1 | yes | `kern.memorystatus_level`, as 100 minus level (unverified definition) | None |
| `mem.pressure_level` | | enum 0 to 2 | gauge | 1 | yes | `kern.memorystatus_vm_pressure_level` (unverified) | None |
| `mem.swap_used` | | bytes | gauge | 1 | yes | `vm.swapusage` | None |
| `mem.swap_in`, `mem.swap_out` | | pages/s | rate | 1 | yes | `host_statistics64` counters | None |
| `power.cpu` | | W | gauge | 1 | yes | M3 Max: SMC P-cluster keys, scaled live to PMP energy, E cluster left out (D-054); other chips: IOReport Energy Model deltas | SmcUserClient or IoReport |
| `power.gpu`, `power.ane`, `power.dram` | | W | gauge | 1 | yes | IOReport Energy Model deltas | IoReport |
| `power.package` | | W | gauge | 1 | yes | sum of the four above | IoReport |
| `power.system` | | W | gauge | 1 | yes | SMC `PSTR` (unverified key on all chips) | SmcUserClient |
| `power.cpu_source` | | enum 1 to 3 | gauge | 1 | no | 1: SMC P clusters, uncalibrated; 2: calibrated to PMP this session; 3: scaled by a seed (last session's scale or the chip default) until a window closes; absent: PMP, all clusters (D-054, D-065) | SmcUserClient |
| `thermal.zone` | `sensor` (raw HID name, e.g. "PMU tdie4") | °C | gauge | 5 (D-055) | yes | IOHID sensors | HidSensors |
| `thermal.cpu`, `thermal.gpu` | | °C | gauge | 5 | yes | max of the chip's SMC CPU or GPU key group | SmcUserClient |
| `thermal.hottest` | | °C | gauge | 5 | yes | max of `thermal.zone` | HidSensors |
| `thermal.sensor` | `name` (battery, ssd, wifi) | °C | gauge | 5 | yes | SMC or HID per chip map | SmcUserClient |
| `thermal.state` | | enum 0 to 3 | gauge | 2 | yes | `NSProcessInfo.thermalState` | None |
| `fan.rpm` | `fan` (0, 1) | rpm | gauge | 2 | yes | SMC `F%dAc` | SmcUserClient |
| `fan.max` | `fan` | rpm | gauge | 60 | no | SMC `F%dMx` | SmcUserClient |
| `fan.mode` | | enum | gauge | 60 | no | SMC `F%dMd` (unverified) | SmcUserClient |
| `net.rx`, `net.tx` | `iface` | bytes/s | rate | 1 | yes | `getifaddrs` `if_data` | None |
| `net.link_rate` | `iface` | bits/s | gauge | 60 | no | `if_data.ifi_baudrate`; Wi‑Fi transmit rate via CoreWLAN (unverified) | None |
| `disk.read`, `disk.write` | `dev` | bytes/s | rate | 1 | yes | IOBlockStorageDriver statistics | None |
| `disk.used`, `disk.free`, `disk.total` | `vol` | bytes | gauge | 60 | yes | `statfs` | None |
| `battery.charge` | | % | gauge | 10 | yes | IOPowerSources | None |
| `battery.charging`, `battery.external` | | 0 or 1 | gauge | 10 | yes | IOPowerSources | None |
| `battery.time_remaining` | | minutes | gauge | 10 | no | IOPowerSources | None |
| `battery.health` | | % | gauge | 60 | yes | AppleSmartBattery max ÷ design capacity | None |
| `battery.cycles` | | count | gauge | 60 | yes | AppleSmartBattery | None |
| `battery.capacity_wh`, `battery.design_wh` | | Wh | gauge | 60 | no | AppleSmartBattery | None |
| `battery.power` | | W, signed | gauge | 10 | yes | AppleSmartBattery voltage × amperage | None |
| `battery.temp` | | °C | gauge | 10 | yes | AppleSmartBattery | None |
| `self.cpu` | | % | gauge | 10 | yes | `proc_pid_rusage` over the app and its WebKit helpers | None |

On an M4 Pro this is roughly 100 persisted series, under the 150 the budget math in architecture.md assumes. Gauges sampled less often than every tick are `NaN` on the other ticks; accumulators skip `NaN`, and the JSON bridge sends it as `null`.

### 6.2 Process data

Processes are not series. Each process sample is a row: `pid`, `start_time`, `name`, `cpu_pct` (percent of one core), `mem_bytes` (phys_footprint), `compressed_bytes`, `threads`, `idle_wakeups_per_s`, `energy` (approximate energy impact), `disk_read_bps`, `disk_write_bps`, `user`. v1.2 adds `net_rx_bps`, `net_tx_bps` and `gpu_pct`. The store keeps the top 30 by CPU every 10 s for 72 hours in `proc_snap`, and the top 5 per minute in `proc_top_1m` for the retention period (architecture.md, Store).

### 6.3 IPC contract

Every command and event is generated by tauri-specta into `src/core/generated/`. All commands take `host: HostId` where data is per host, even in v1.

| Command or event | Arguments | Returns | Used by | Version |
|---|---|---|---|---|
| `list_hosts` | | `HostRecord[]` | app boot | v1.0 |
| `get_host` | `host` | `HostRecord` | headers, onboarding | v1.0 |
| `get_capabilities` | `host` | `Capabilities` | onboarding, sidebar | v1.0 |
| `subscribe_live` | `host`, `channel: Channel<LiveMsg>`, `backfill_ms` | `SubscriptionInfo` | every window | v1.0 |
| `set_process_interest` | `host`, `interested: bool` | | Overview, CPU, Memory, Disk, Processes pages | v1.0 |
| `query_history` | `HistoryRequest { host, selectors, from_ms, to_ms, tier: Auto or a tier, max_points }` | `HistoryPage { tier, series: { key, points: { t, min, max, avg }[] }[], gaps }` | Timeline, battery bars, 24 h maxima | v1.0 |
| `query_processes_at` | `host`, `t_ms` | `ProcessSample[]` with resolution | Timeline tooltip | v1.0 |
| `query_heatmap` | `host`, `metric: Cpu or Temp`, `days` | `{ dateIso, hours: (number or null)[] }[]` | Timeline heatmap | v1.1 |
| `export_csv` | `host`, `selectors`, `from_ms`, `to_ms`, `tier`, `path` | bytes written | Timeline | v1.1 |
| `query_events` | `host`, `from_ms`, `to_ms` | `Event[]` | Timeline annotations | v1.2 |
| `get_settings`, `update_settings` | `patch` | `Settings` | all windows | v1.0 |
| `history_size`, `clear_history` | `host` | bytes | Settings | v1.0 |
| `history_health` | `host` | `HistoryHealth { low_disk_paused, trimmed_before_ms, trimmed_limit_bytes, cap_met }` | Settings, Timeline (D-059) | v1.0 |
| `set_paused` | `paused` | | popover, tray menu | v1.0 |
| `open_dashboard` | `route?` | | popover, tray | v1.0 |
| `sensor_dump` | `host` | `SensorDump` JSON | unsupported notice | v1.0 |
| `process_signal` | `host`, `pid`, `start_time`, `kind: Quit or ForceQuit` | `Result<(), ProcessSignalError>` (`Refused`, `NotFound`, `PidReused`, `PermissionDenied`) | Processes page | v1.0 |
| `check_for_updates` | | `UpdateStatus` | Settings | v1.0 |
| event `settings-changed` | | `{ revision, settings }` | all windows | v1.0 |
| event `capabilities-changed` | | `{ host, capabilities }` | all windows | v1.0 |
| event `history-health-changed` | | `{ health }` | Settings, Timeline | v1.0 |

`LiveMsg` variants: `Layout { layout_no, series: SeriesKey[] }`, `Backfill { layout_no, start_ms, interval_ms, rows: (number or null)[][] }`, `Frame { ts_ms, layout_no, values: (number or null)[] }`, `Processes { ts_ms, rows: ProcessSample[] }`, `Caps(Capabilities)`, `Status { interval_ms, frame_period_ms, paused, on_battery, performance }`.

### 6.4 Settings schema

| Key | Type | Default |
|---|---|---|
| `modules.<id>.enabled` | bool | true for present modules except Disk |
| `modules.<id>.menu_bar` | enum per 4.15 | CPU, GPU, Memory: InCombined; Power: TempInCombined; others Hidden |
| `sampling.interval_ms` | 500, 1000, 2000, 5000, 10000, 30000, 60000 | 1000 |
| `sampling.slow_on_battery` | bool | true |
| `history.retention_days` | 7, 30, 90 | 30 |
| `history.size_limit_mb` | 150, 300, 500, 1000 | 150 |
| `units.temperature` | C, F | C |
| `units.network` | MBps, Mbps | MBps |
| `units.memory` | GB, GiB | GB |
| `general.launch_at_login` | bool | true (set in onboarding) |
| `general.show_in_dock` | bool | false |
| `general.appearance` | System, Light, Dark | System |
| `general.check_updates` | bool | true |
| `general.chart_window` | 5m, 15m, 30m, 1h | 15m (D-091) |
| `onboarding.completed` | bool | false |

### 6.5 Files on disk

| File | Path | Writer |
|---|---|---|
| History database | `~/Library/Application Support/com.tryopendata.kelvo/history.sqlite` (+ `-wal`, `-shm`) | store writer thread |
| Settings | `~/Library/Application Support/com.tryopendata.kelvo/settings.json` | Rust settings owner via `tauri-plugin-store` |
| Host identity | `~/Library/Application Support/com.tryopendata.kelvo/host-id` (UUID text) and `hosts` table | app shell on first run |
| Logs | `~/Library/Logs/com.tryopendata.kelvo/` | `tracing` file appender, 5 files × 5 MB |

## 7. Performance budget

The base budget is the table in [architecture.md, Performance budget](architecture.md#performance-budget). v1 adds these targets. Values marked proposed are first guesses to confirm with measurements in phase 6 or the release phase; a target that turns out wrong is changed with a decision entry, not silently.

| Metric | Target | Method | Version |
|---|---|---|---|
| Idle coalition CPU, popover and dashboard closed | under 0.5% and at or below Stats | `scripts/bench-vs-stats.sh` | v1.0 |
| Coalition CPU with the popover open or the dashboard on Overview | no product target; regression-guarded (D-088 withdrew the proposed 2% and 3%) | `perf-gate.spec.ts`, `coalition.visible` in `scripts/bench-coalition.sh` | v1.0 |
| Engine alone, all v1.0 collectors at 1 s | under 0.2% (proposed) | engine example binary, 10 min | v1.0 |
| Tray WindowServer cost | under 0.2% added | WindowServer CPU time, tray on vs off | v1.0; re-measured with several items in v1.1 |
| Tray frames skipped by hash on an idle machine | over 50% (proposed) | counter in debug builds | v1.0 |
| Popover open | under 150 ms p95 over 50 opens | instrumented timestamps | v1.0 |
| Coalition memory with warm panel | under 150 MB | `footprint` | v1.0 |
| `query_history` 24 h, 6 lanes | under 100 ms p95 (proposed) | Rust benchmark on a filled DB | v1.0 |
| Timeline 30 d first render | under 500 ms (proposed) | instrumented | v1.1 |
| CSV export of 30 days, all lanes | under 3 s (proposed) | instrumented | v1.1 |
| NetworkStatistics collector while visible | under 0.3% added (proposed); zero when not visible | bench script with Network page open and closed | v1.2 |
| Detectors | under 0.05% added (proposed) | engine example | v1.2 |
| Frontend main-thread time at 1 Hz (Chromium, dev build, mock transport) | popover under 40 ms/s, Overview under 50 ms/s, no long task over 50 ms | `tests/e2e/perf-gate.spec.ts`, thresholds in `perf-budget.json` (D-060) | v1.0 |

## 8. Milestones and tasks

Until the release phase, Kelvo runs from source: `bun run tauri dev`, or a local `bun run tauri build` for packaged-only checks. Each phase ends with a build that does everything the phase lists. There are no tags, releases or updater builds before the release phase (D-078). The order is v1.0 (done), v1.1, v1.2, then manual QA and polish, then the release phase. Tasks marked "(infra N)" are the "Infrastructure laid in v1" items from architecture.md, numbered as there; section 11 maps them back.

Every UI task is done only after its screen is checked against design-system.md and the screens already built: screenshot the route with the mock transport, and list any difference left in place in the PR.

### v1.0, phase 0: workspace, toolchain and spikes

Installable result: a Kelvo-branded app that opens a token test page in light and dark.

Workspace and crates
- [x] Root `Cargo.toml` workspace with members `crates/*` and `src-tauri`, resolver 2, shared `[workspace.dependencies]` for serde, ciborium, rusqlite (bundled), tokio, uuid, smallvec, compact_str, thiserror, tracing
- [x] Create `kelvo-schema`, `kelvo-proto`, `kelvo-collect`, `kelvo-store`, `kelvo-engine` with empty `lib.rs`, each depending only on what the direction table in architecture.md allows (infra 6)
- [x] Rename the app in `tauri.conf.json`: productName "Kelvo", identifier `com.tryopendata.kelvo`, bundle category Utilities, `minimumSystemVersion` "26.0" (macOS N-1 policy, D-028)
- [x] Update Tauri to the latest 2.x at or above 2.12 and confirm in the changelog that the tray icon and template flag can be set in one call; record the version in decisions.md
- [x] Pin `tauri-specta` and `specta` to exact versions, note their release status (RC or stable) in decisions.md
- [x] Remove the template `App.tsx`, `App.css`, `assets/react.svg` and the `greet` command

Spikes (each ends with a short entry in decisions.md: works, works with caveat, or does not work plus the fallback)
- [x] `tauri-nspanel` with the pinned Tauri: non-activating panel shows under a tray item and hides on outside click (D-033)
- [x] Native vibrancy behind a transparent webview in that panel (`windowEffects` or `window-vibrancy`), and whether `macOSPrivateApi` is required (D-034)
- [x] Hook for WebContent process termination through Tauri's webview API, or the objc2 fallback (D-035)
- [x] Window occlusion notifications reachable from Rust for a Tauri window (D-036)
- [ ] Status item ⌘-drag reorder and position persistence with Tauri's tray API (needed in v1.1; decide early whether objc2 is required) (D-037: objc2 access confirmed; needs a manual ⌘-drag check on the bundled app)

Frontend tooling
- [x] Tailwind v4 with `@tailwindcss/vite`; shadcn "new-york" `components.json` targeting `src/app/components/ui`; `cn()` in `src/app/lib/utils.ts`; lucide, cva, tailwind-merge installed
- [x] tsconfig strict with `noUncheckedIndexedAccess` and `verbatimModuleSyntax`; aliases `~/` to `src/app` and `@core/` to `src/core` in tsconfig and Vite
- [ ] Biome config (`biome.json`, D-026) with recommended + a11y + React hooks rules and the widget boundary rule (`noRestrictedImports` override); a fixture file under `src/app/widgets/` importing `@core/transport` fails lint in a test (infra 10) (config and rules in place; the fixture test that proves the widget rule fails lint is not written)
- [x] Biome `noRestrictedImports` override forbidding React imports in `src/core/**`
- [x] Biome formatter with `useSortedClasses` for Tailwind class order; Vitest with happy-dom; Playwright config that starts the Vite dev server with the mock transport
- [x] `bun run check` runs format check, lint, typecheck and Vitest

CI
- [x] Replace the opendata-derived `.github/workflows/ci.yml` with a Kelvo workflow; choose hosted or self-hosted macOS runner and record why in decisions.md
- [x] macOS job: `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `bun run check`, Playwright, `bun tauri build --debug`
- [x] Run the macOS job on both supported majors (macOS 27 and 26, D-028), or record in decisions.md why only one runner image is available and how the other is covered before release
- [x] Linux job on `ubuntu-latest`: `cargo check` and `cargo test` for the five crates (infra 6)
- [x] Generated-bindings drift check: build exports bindings and the job fails if `src/core/generated/` differs from the commit

Acceptance criteria
- `bun tauri build` produces `Kelvo.app`; it launches and shows a page using `bg-background`, `bg-card`, `text-muted-foreground` and the module accents in both themes.
- Both CI jobs pass on a clean checkout.
- Every spike has a decisions.md entry.

### v1.0, phase 1: schema, proto and store

Installable result: unchanged UI. The value of this phase is in tests, and in the one-way doors being closed before collectors depend on them.

kelvo-schema
- [x] `MetricId`, `Labels` (sorted, canonical display form `core=7`), `SeriesKey` with parse and display round-trip tests (infra 1)
- [x] `MetricDef`, `Unit`, `MetricKind`, `Module` and `CATALOG` containing every metric in 6.1; test that IDs are unique and label keys match their `MetricDef` (infra 1)
- [x] `HostId(Uuid)`, `HostRecord` with `is_local`, `HostInfo`, `ClusterInfo` with DVFS state tables (infra 2)
- [x] Doc comment and a test asserting millisecond timestamps and `seq` stay under 2^53 for the next 200 years
- [x] `Capabilities`, `ModuleCap`, `UnsupportedReason`, `Entitlement` (infra 9)
- [x] `Snapshot` and module views with `from_frame`; test that a missing series yields `None`, never `0.0` (infra 1)
- [x] `AlertRule`, `Condition`, `SeriesSelector` as serializable data with CBOR and JSON round-trip tests, unused until v1.2 (infra 7)
- [x] `Tier`, `Cursor`, `Settings` with a `validate()` covering the allowed values in 6.4
- [x] All IPC-facing types derive `specta::Type`

kelvo-proto
- [x] Length-prefixed framing (u32 BE) and CBOR encode/decode via ciborium (infra 3)
- [x] `Message` enum per architecture.md including `HostSummary` (reserved) and `Unknown` (infra 3)
- [x] Handshake negotiation function: version range check, feature intersection, error on incompatible versions; tests for equal, older-compatible and incompatible peers (infra 3)
- [x] Skew fixtures under `crates/kelvo-proto/tests/fixtures/v1/` with a test that decodes them; a test that an unknown message variant carrying content decodes as `Unknown`, which settles the `#[serde(other)]` question in architecture.md (record the answer in decisions.md) (infra 3)
- [x] Test that a `LiveFrame` whose layout contains an unknown `metric_id` is accepted and the unknown series ignored (infra 3)

kelvo-store
- [x] Open or create with `auto_vacuum=INCREMENTAL` before any table, WAL, `journal_size_limit`, `synchronous=NORMAL`; migrations via `PRAGMA user_version`
- [x] DDL from architecture.md, including `events`, with `gaps.reason` accepting `paused` and `module_disabled` and the nullable `gaps.module` column
- [x] `meta` rows: `db_instance_uuid` created once, `schema_version`, `next_seq`; `seq` assigned inside each write transaction (infra 4)
- [x] Host upsert keyed by UUID; series interning; layout minting with hash dedupe (infra 1, 2)
- [x] Single writer thread owning the only write connection, fed by a channel, committing every 5 minutes (D-070) and on flush requests (sleep, wake, pause, resume, module switch, gaps reopened after a clock-step discard, store swap, shutdown; the discard commits on its own) (infra 8)
- [x] Tier ingest upserting on `(host_id, bucket_ts, layout_id)`; test that replaying the same batch leaves identical rows (infra 4)
- [x] Gap open and close; on startup, close any open gap and write an `app_not_running` gap from the last persisted bucket to now
- [x] Process tables: name interning, `proc_snap` writes, roll-down of snapshots older than 72 h into `proc_top_1m`
- [x] Range query across layouts for a set of selectors, returning per-series points with min, max and avg plus gaps, with server-side min/max/avg merge down to `max_points`
- [x] `query_processes_at` against `proc_snap` then `proc_top_1m`
- [x] Cursor read API: rows, gaps and events after `(epoch, seq)` per tier, returning `Truncated` when pruned past (infra 4)
- [x] In-process sync test: store A syncs into store B through the cursor API and proto message types, including a replayed page, a truncated cursor that becomes a gap row, and an epoch change that forces a full resync (infra 3, 4)
- [x] Pruning by retention in 5,000-row batches plus `incremental_vacuum`; `size_on_disk` including WAL; `clear_host`
- [x] Synthetic fill test writing 30 days at 150 and 250 series through the real writer, running pruning, asserting at most 150 MB for 150 series and that the byte cap holds 250 series under 150 MB; runs in CI. Since D-089: 89.2 MB at 150 series and 115.0 MB at 250 (69.9 and 95.7 MB before per-app network history), both without the cap, and a third run with a 111 MB cap keeps the trim exercised
- [x] Byte cap (150 MB incl. WAL, trim oldest history to 90%, `pruned` marks moved, no gap written), WAL `TRUNCATE` checkpoint after prune, 16 KiB pages, and the low-disk guard that pauses S10 (D-057)

Acceptance criteria
- `cargo test --workspace` passes on macOS and Linux, including the fill test, with the measured file size logged and copied into PROGRESS.md.
- decisions.md records the `#[serde(other)]` result.

### v1.0, phase 2: collectors and engine

Installable result: the app runs the engine in the background and logs a one-line summary per minute; `cargo run -p kelvo-engine --example dump` prints a Snapshot every tick.

kelvo-collect
- [x] `Collector` trait, `Probe`, `Cadence`, `SampleBuf`, `CollectorId`; every collector declares `required_entitlements()` (infra 6)
- [x] CPU collector via sysinfo with narrow refresh kinds: `cpu.total`, `cpu.user`, `cpu.system`, `cpu.load{core}` with P and E labels from the topology, `cpu.loadavg` (done on `host_processor_info` directly: sysinfo has no user/system split)
- [x] Vendor macmon's IOReport and SMC/HID code with its license and attribution in `kelvo-collect/src/macos/vendor/` (confirm the license permits it; unverified) (MIT, verified; D-042)
- [x] IOReport subscription builder that subscribes only to channel groups the active collectors need; delta sampling against the previous sample
- [x] Cluster frequency, active residency, per-state residency and cluster power (cluster power is a gap on macOS 27, where the PMP counters refresh about every 5 minutes; D-043)
- [x] GPU utilization, renderer and tiler (confirm the source; if the split is unavailable, drop `gpu.render` and `gpu.tiler` from the catalog and the Overview legend), frequency and residency (split available from `IOAccelerator` `PerformanceStatistics`; D-042)
- [x] Energy model power: CPU, GPU, ANE, DRAM, package; system power from SMC (CPU, ANE, DRAM and package are gaps on macOS 27; D-043)
- [x] Sensor map per chip family (M1, M2, M3, M4 and their Pro, Max, Ultra variants) mapping SMC keys and HID names to `thermal.cpu`, `thermal.gpu`, `thermal.sensor{name}`; unknown chips return `Unsupported(UnknownChip)` for the mapped metrics while raw `thermal.zone` still works if HID sensors exist (SMC keys only; HID names go raw to `thermal.zone`. Verified on M3 Max only; an Ultra uses its family's map with the second die unmapped; M5 is `UnknownChip`)
- [x] Fans: count, rpm, max, mode; zero fans reports `NotPresent` for fan series
- [x] Memory composition, pressure and pressure level, swap usage and swap rates
- [x] Network per-interface rates with counter wrap handling, interface kind, link rate (link rate is `ifi_baudrate`; CoreWLAN not used)
- [x] Disk I/O per physical device; capacity per mounted volume every 60 ticks, boot APFS container identified
- [x] Battery via IOPowerSources and AppleSmartBattery; `NotPresent` on Macs without a battery (`battery.temp` absent on macOS 27: no `Temperature` key)
- [x] Processes via `proc_listallpids`, `proc_pid_rusage`, `proc_pidinfo`: all fields in 6.2, keyed by `(pid, start_time)`, with the energy approximation documented in code (own-user processes only without root; `compressed_bytes` is `None`)
- [x] `self.cpu` across the app process and its WebKit helper processes (find them by responsible PID or coalition; unverified which works without entitlements) (responsible PID works without entitlements)
- [x] `thermal.state` from `NSProcessInfo`
- [x] `linux` module behind `cfg(target_os = "linux")` that returns `Unsupported` for everything and compiles in the Linux CI job (infra 6)
- [x] `appstore` Cargo feature and `allowed_entitlements()` filter; a test that with the feature on, IOReport, SMC, HID and NetworkStatistics collectors are dropped and report `MissingEntitlement` (infra 6) (`crates/kelvo-collect/tests/entitlements.rs`, with fakes for the private-API collectors)

kelvo-engine
- [x] `Ticker` trait, GCD `DispatchSource` implementation on a utility-QoS queue with 10% leeway, and `FakeTicker` (infra 6) (start/stop/now into one engine inbox rather than a blocking `next()`; D-046)
- [x] `PowerSignals` trait, macOS implementation (sleep and wake, display sleep, screen lock, Low Power Mode, on battery) and a fake (infra 6) (sleep/wake via `IORegisterForSystemPower` with an ack before sleep; the rest polled; D-046)
- [x] Sampler loop on one thread: cadence multiples, `NaN` for series not sampled this tick, layout change detection publishing `Layout` before the first frame that uses it (frames also carry a held latest-value array for the Snapshot; D-047)
- [x] One-hour ring buffer per host with a backfill API returning rows for a span; layout changes inside the ring handled (evenly spaced segments, split at layout, interval and holes)
- [x] Accumulators for S10 and M1 aligned to wall-clock multiples, `NaN`-aware, closing into store ingest; tests with `FakeTicker` across a bucket boundary and across a layout change
- [x] Back-off: base tick to 2 s on battery (when enabled) or in Low Power Mode; a `display_asleep or screen_locked` flag consumers read (`EngineStatus::display_idle`)
- [x] Sleep handling: on `WillSleep` close open buckets, flush the writer, open a `sleep` gap, stop the ticker; on `DidWake` close the gap using the continuous clock and restart; tests with fakes (plus a stall gap when ticks stop without a sleep event)
- [x] Pause: stop collectors, open a `paused` gap, keep the bus alive for status messages
- [x] Bus: `tokio::sync::broadcast` per host carrying frames, layouts, capability changes, process rows and status; a lagging subscriber drops frames and the engine never blocks
- [x] Capability re-probe on IOKit match notifications for disks and network interfaces; emits `CapabilitiesChanged` with a new revision (infra 9)
- [x] Process interest counter: `Cadence::Adaptive` samples processes every tick while any window has interest, every 10 ticks otherwise
- [x] `Source` trait and `LocalSource` wrapping the engine, with `start(sink)`, `host()`, `capabilities()` (infra 5)
- [x] `examples/dump.rs` printing the Snapshot each tick and the series count at startup

Benchmarks
- [x] `scripts/accuracy-vs-macmon.sh`: runs `kelvo-engine` dump and `macmon` side by side for 10 minutes, compares CPU and GPU utilization, cluster frequency, power components and temperatures, and fails outside ±5% or ±2 °C (written, not yet run: macmon is not installed on the dev Mac, and the script exits 2 saying so)
- [x] Engine-only overhead measured with the example binary for 10 minutes at 1 s; result in PROGRESS.md (1.39% on an M3 Max, over the proposed 0.2%; profiled and attributed, not yet reduced)

Acceptance criteria
- The accuracy script passes on the development Mac; the chip and macOS version are logged with the result.
- A real lid-close for at least 5 minutes produces one `sleep` gap whose span matches the wall-clock sleep within 2 s.
- Engine-only CPU is recorded; if it exceeds 0.2%, the hottest collector is profiled before phase 3 starts.

### v1.0, phase 3: app shell, tray and windows

Installable result: a working menu bar item with real values, a popover that opens fast and shows a placeholder page with live values, a dashboard window with an empty shell, settings persistence, launch at login.

App shell
- [x] `AppState` with `HostRegistry`, the local `HostRecord`, `LocalSource` started at launch, the store opened; if the store fails to open, start in live-only mode with a `history_unavailable` flag (infra 5)
- [x] Host UUID created on first run, persisted to `host-id` and the `hosts` table, read on later runs; `is_local = true` (infra 2)
- [x] Accessory activation policy at start; Show in Dock switches between Accessory and Regular
- [x] Settings owner: `tauri-plugin-store` file written only from Rust, `get_settings`, `update_settings` with validation, `settings-changed` with a revision; engine-affecting changes reconfigure the ticker and collector set before the event fires (infra 8)
- [x] Every command and event in 6.3 marked v1.0, exported with tauri-specta to `src/core/generated/`; every command takes `HostId` where data is per host (infra 2)
- [x] Live channel registry keyed by window label and host: `subscribe_live` sends `Layout`, then `Backfill`, then frames; Rust stops sending when the window is hidden, minimized, occluded, or the display sleeps, and resumes with a fresh backfill (infra 8)
- [x] `set_process_interest` reference-counted per window and cleared when a window hides or closes
- [x] Each window root receives `data-performance` (D-088; was `data-power-saver`), `data-reduce-transparency` and the theme through a startup command and change events
- [x] Launch at login with `smappservice-rs`, wired to the setting
- [x] Logging with `tracing` to `~/Library/Logs/com.tryopendata.kelvo/` with rotation

Tray
- [x] Renderer with `tiny-skia` and `ab_glyph`: combined glyph per the design-system spec and the values layout, at 1x and 2x
- [x] Bundle JetBrains Mono (OFL) for tray text
- [x] Quantize values, hash the frame, skip unchanged frames; debug counter of skipped and drawn frames (equality of the quantized frame, not a hash: D-056)
- [x] Set image and template flag atomically; set the accessibility label each redraw
- [x] Redraw on the base tick, stop on display sleep and screen lock, track the back-off interval
- [x] Per-module menu bar modes from settings decide which elements render
- [x] Left click toggles the popover; right click or Control-click opens the native menu (Open dashboard, Settings, Pause sampling, Quit Kelvo)

Popover panel
- [x] `tauri-nspanel` non-activating panel, 360 × 680, created hidden at startup with the `popover` label
- [x] Positioned under the status item on the display that holds it, kept on screen near the notch and screen edges
- [x] Hides on outside click, Esc and second tray click; occlusion stops the channel
- [x] Native vibrancy material with the CSS tint on top; Reduce Transparency removes the material and switches tokens to opaque fallbacks
- [x] Reloads its URL after a WebContent process termination, the next time it is hidden
- [x] Open-latency instrumentation: Rust records the click time, the webview reports first paint after show; p95 logged in debug builds

Dashboard and onboarding windows
- [x] `dashboard` window with an overlay title bar and inset traffic lights over the sidebar, minimum size 1024 × 700 (proposed), size and position remembered; close hides it, and it is destroyed after 5 minutes hidden
- [x] `open_dashboard(route)` creates or focuses the window and navigates
- [x] `onboarding` window shown when `onboarding.completed` is false

Acceptance criteria
- Tray values match the `dump` example within one tick.
- With the machine idle for 10 minutes, the hash skips at least half of the frames, or the measured ratio is recorded and the target revised.
- Popover open p95 under 150 ms over 50 opens on the development Mac.
- Logs show the popover channel stopping within one tick of hide and occlusion.
- WindowServer CPU with the tray on vs off measured and recorded.

### v1.0, phase 4: frontend foundation

Installable result: the popover and dashboard render a component gallery fed by live data.

Styles and fonts
- [x] `src/app/styles/theme.css`: the `@custom-variant dark` and `@theme` tokens, light defaults and `.dark` overrides, the `.surface-vibrant` scope, Reduce Transparency fallbacks, Increase Contrast overrides
- [x] `cards.css` (`.vt-card`, `.vt-card--chart`, `@property --g`), `effects.css` (`.data-mono`), `motion.css` (tokens, everything inside `prefers-reduced-motion: no-preference`, power-saver overrides)
- [x] Self-hosted Inter Variable and JetBrains Mono Variable woff2 with `cv01` and `ss03` on `html`, root weight 510

Core (no React)
- [x] `core/transport.ts`: the `Transport` interface, `tauriTransport` over the generated bindings and Channels, `mockTransport` with a seeded generator that produces stable sample values and can script scenarios (sleep gap, unknown chip, no battery, no fans, paused, stale)
- [x] `core/format/`: percent, bytes in GB and GiB, rates in MB/s and Mb/s, temperature in °C and °F, watts, durations ("3d 4h", "6:12"), with tests
- [x] `core/chart-math/`: gap splitting on `null`, min/max/avg downsampling, nice ceiling with 60 s shrink hysteresis, heatmap alpha, ring arc math, with tests
- [x] `core/query-keys.ts` with `(hostId, module, range, tier)` keys (infra 2)

App foundation
- [x] Host store factory with `createStore` and `HostStoreProvider`; `reduceLive` handling `Layout`, `Backfill`, `Frame`, `Processes`, `Caps`, `Status`; frontend state keyed `hosts[hostId]` (infra 2)
- [x] Selectors per module; a Vitest render-count test proving a frame that only changes CPU values does not re-render the Memory card
- [x] Settings mirror store replaced on `settings-changed`, invalidating dependent TanStack Query keys (infra 8)
- [x] TanStack Query client
- [x] Memory router keyed by window label: `popover`, `dashboard/*`, `onboarding`
- [x] shadcn primitives: Button, Switch, Select, ToggleGroup, Tooltip, Dialog, Checkbox, ScrollArea, Table, ContextMenu, Sonner (toast)
- [x] Render-only components under `src/app/widgets/` with JSON-serializable props as specified in design-system.md (plain numbers, strings, booleans, `(number | null)[]`, ms timestamps; no `Date`, `Map`, typed arrays): Card, MetricCard, ModuleCard, RingGauge, ClusterFreqRing, RingStatCard, InlineBar, StackBar, Legend, StatGrid, StatStrip, StreamArea, MirrorBars, CoreTiles, CoreHeatmap, ResidencyBar, PowerStack, BatteryHistoryBars, ChartAnnotation, ProcessList, InitialChip, GapBand, StatusPill
- [x] A Vitest test per widget that `JSON.parse(JSON.stringify(props))` renders identically to the original props (infra 10)
- [x] App components outside `widgets/`: CardGrid, ZoneTable, ProcessTable (virtualized), InterfaceTable, VolumeTable, SegmentedControl, Sidebar, MachineHeader, PopoverHeader, PopoverFooter, TrayPreview, TrayStyleOption, ModuleToggleList, SettingsRow, CollectingOverlay, UnsupportedNotice
- [x] Dev-only `/dev/gallery` route rendering every component with mock props in both themes
- [x] Playwright: screenshot every route and the gallery in light and dark with the mock transport; axe contrast check; fail on any `--color-muted-foreground` text below 4.5:1

Acceptance criteria
- The gallery renders every listed component in both themes with no console errors.
- The render-count test and the serializability tests pass.
- Playwright contrast checks pass.

### v1.0, phase 5: screens

Installable result: the full v1.0 experience. Each screen task is done only when the screen works with the mock transport and in the packaged app, in light and dark, and has been compared with its original design mock.

Popover (4.3)
- [x] Header with host name and uptime, interval pill, pause, settings
- [x] Cards in module order for enabled, present modules: CPU, Cores, Memory, GPU, Power, Network, Battery (GPU "Cores" stat is Render: no GPU core count in `HostInfo`)
- [x] Scroll behavior with overlay scrollbar and header border when scrolled
- [x] Footer: Open dashboard, Activity, self-CPU readout
- [x] 60 s backfill on show so charts are full on the first frame
- [x] Compare with the original popover mocks (default, scrolled and light)

Dashboard shell and sidebar (4.4)
- [x] Sidebar groups, icons, live values, active row, footer with sampling status and version (footer also says Stale)
- [x] Modules not present omitted; disabled modules dimmed
- [x] Keyboard navigation of the sidebar (native links in tab order)

Overview (4.5)
- [x] Page header with Live pill
- [x] Machine header with illustration and spec grid (no marketing name, year, GPU cores, memory type or OS build: not in `HostInfo`)
- [x] Six MetricCards with rotating glow origins, links to module pages, process interest while visible
- [x] GPU card 60 s chart slot and Network card interface list (until v1.2)
- [x] 24 h maxima for GPU power and disk bars from `query_history`
- [x] Passive-cooling and no-battery variants

CPU page (4.7)
- [x] Header and window control; stat strip; total chart
- [x] Cluster frequency rings and cluster power, multi-cluster wrap (cluster power reads "—" wherever the series is a gap, D-043, D-048)
- [x] Per-core heatmap with 10 s columns from the ring and hatched columns for gaps
- [x] Cluster residency over 60 s with "other" merge
- [x] Top processes table with sorting and "Show all"

Power & Sensors page (4.10)
- [x] Four ring cards with fan and battery variants (fan mode 0/1 = Automatic/Manual is unverified)
- [x] SoC thermal zones table with throttled re-sort and footer sensors
- [x] Power by component stacked chart over 10 minutes (mostly broken on macOS 27 where CPU, ANE and DRAM are gaps, D-043; no annotations until v1.2)
- [x] Battery last 24 hours from `tier_1m`

GPU, Memory, Network, Disk, Battery, Processes pages (4.8, 4.9, 4.11 to 4.14; no mocks)
- [x] GPU page per 4.8 (no GPU core count in the subtitle: not in `HostInfo`)
- [x] Memory page per 4.9 (no memory type in the subtitle and no pressure threshold lines: no source for either)
- [x] Network page per 4.11 (interface kind and since-boot totals are "—": not in the schema)
- [x] Disk page per 4.12 (volumes named by mount path; no boot volume or container: not in the schema)
- [x] Battery page per 4.13, reusing the Power & Sensors battery section
- [x] Processes page per 4.14 with search, column sets, frozen order on hover
- [x] `process_signal` command: re-read the PID's start time and fail with `PidReused` on mismatch, refuse PID 1, `kernel_task`, `WindowServer` and Kelvo's own PIDs, use `NSRunningApplication` for apps and `kill(2)` otherwise, map `EPERM` and `ESRCH` to typed errors
- [x] Quit and Force Quit row action and context menu with confirm dialogs (Force Quit warns about unsaved data), disabled state with tooltip for refused processes, toast on `PermissionDenied`
- [x] Tests: Rust tests that spawn a child process and quit and force-quit it, that a stale `start_time` returns `PidReused`, that each refused target is rejected without a signal sent, and that another user's PID returns `PermissionDenied`; a Vitest test that the dialog sends the command only after confirmation

Timeline (4.6)
- [x] Header with 1h and 24h, back, forward, Live
- [x] Six lanes with label columns, shared x axis, uPlot-backed plots with min/max envelopes
- [x] Stitch history and ring tail; append on bucket close while following Live
- [x] Synced crosshair with dots and tooltip; top processes via `query_processes_at`
- [x] Gap bands and Sleep and Wake markers with a non-overlapping annotation row
- [x] "Show as table" per lane (Should)

Settings (4.15)
- [x] Modules table with menu bar select and On switch
- [x] Sampling: interval with measured overhead sentence, slow down on battery, keep history with projected size for 90 days, history on disk with confirmed Clear
- [x] Units, General (launch at login, show in Dock, appearance), update checks and version
- [x] History size limit (150 MB to 1 GB) with projected size per retention option and the "limited to about N days" line; low-disk and trim notices from `history_health` (D-059)
- [x] Interval options 0.5 s to 60 s; popover and Overview live windows, the popover backfill and module window controls follow the interval (D-059, D-061)

Onboarding (4.16)
- [x] Step 1: modules from capabilities, style cards with live TrayPreview, launch at login, chip status, Skip and Continue
- [x] Step 2, Updates and privacy: update checks switch, no-telemetry statement with the history location and expected size, and Done

States (4.17)
- [x] Empty history overlay and header text (`isCollecting`, `CollectingNote`; wired on the Timeline)
- [x] Gap bands with reason labels on every history chart (`gapBands`/`gapLabel` in `core/history-state.ts`; the Timeline and the module pages' live charts through `useGapBands`; the battery 24 h bars keep their hatched hours)
- [x] Unknown chip notice and Share sensor dump sheet with Copy, Save and Open GitHub issue; `sensor_dump` command excludes serials, user and host names (Save… untested in WKWebView; issue URL assumes github.com/tryopendata/kelvo)
- [x] Paused, stale, history unavailable, sensor read failed, series disappeared (`HistoryUnavailableBanner` on the Timeline, the battery card and Settings)

Acceptance criteria
- Every v1.0 route renders with the mock transport in light and dark, and Playwright screenshots exist for each.
- Each mocked screen has a written comparison with its original design mock listing remaining differences.
- The scripted manual checklist (release phase) passes on the packaged app.

### v1.0, phase 6: benchmarks and gates

Result: perf gates and the dependency check run in `make check` and CI. Distribution, the updater, the Stats comparison and the packaged checklist moved to the v1.x release phase (D-078).

- [x] Runtime dependency check: `make check-deps` and a CI step fail if the app links anything outside `/System/Library/` and `/usr/lib/` (D-058)
- [x] Frontend perf gate in Playwright: popover and Overview main-thread time per second and long tasks, thresholds in `perf-budget.json` (D-060)
- [x] Engine perf gates in `cargo test`: allocations per tick per collector and engine core, OS calls per tick by API family (tray-only and window-open), store rows and bytes per hour; thresholds in `perf-budget.json` (D-062)
- [x] `make perf`: release engine CPU, 120 s tray-only at 1 s plus a 30 s interval run, advisory step in CI (D-062)
- [x] Collector cadences as wall-clock periods at every interval; tray-only mode samples IOReport every 10 s (D-061)
- [x] CPU power at 1 Hz on the M3 Max from the SMC P-cluster keys, calibrated live to PMP, `power.cpu_source` for the UI (D-054)
- [x] UI labels `power.cpu` from `power.cpu_source` ("P cores", "calibrating"); the Cluster frequency card's E-CLUSTER POWER has no value on the M3 Max (D-054) ("CPU power: P cores, uncalibrated / estimated from last calibration" on the popover Power card and the Power stack; E-CLUSTER POWER says "Not measured" when its series is not in the layout)

### v1.1: long-range history, heatmap, CSV, tray styles

Result: runs from source (D-078).

Phase 1.1-A: Timeline 7d and 30d
- [x] Enable 7d and 30d in the range control; queries use `auto` (7d: `tier_1m`; 30d: `tier_15m`, D-076) with server-side downsampling to 2 points per plot pixel
- [x] Back and forward step by one range length; the subtitle shows the exact range
- [x] Tooltip resolution label reflects the merged bucket width ("10 MIN AVG")
- [x] Gap rendering verified with a 30-day fixture that includes many sleep gaps

Phase 1.1-B: 30-day heatmap
- [x] `query_heatmap` in the store: hourly averages of `cpu.total` or `thermal.hottest` from `tier_15m` and the minutes in `tier_1m` by local hour, `null` for hours with no buckets, correct on DST days (23 and 25 hour days)
- [x] `CalendarHeatmap` on canvas: 30 rows × 24 columns, alpha per the design-system formula with `vmax` 80 for CPU, hatched empty cells, today's row highlighted, current hour outlined
- [x] Metric toggle (Avg CPU, Temperature) and legend (0% to 80%+; temperature maps 40 to 90 °C, legend "40 °C" to "90 °C+")
- [x] Cell click opens the Timeline at 1h on that hour itself; arrow-key navigation; accessible names per cell. For a day older than 7 days only 15-minute buckets exist (D-076), so the click opens 6 h centered on the hour (24 buckets) instead of 1 h (4 buckets); so does a fall-back day's two-hour 01:00 cell. Hours still ahead are "not yet" and do not open

Phase 1.1-C: CSV export
- [x] `export_csv` streams from the store to a user-chosen path (save dialog), without loading the range into memory
- [x] Format: header row; columns `time_utc` (ISO 8601), `time_ms`, then `<series>_avg`, `_min`, `_max` per selected series; gap rows written as empty value cells with a `gap_reason` column
- [x] Round-trip test: export a fixture range and parse it back to the same values
- [x] Export button in the Timeline header exports the visible range and lanes

Phase 1.1-D: tray styles and per-module items
- [x] Renderer for the Graphs style (CPU sparkline, memory fill gauge, stacked network rates) and the Cores + histogram style (per-core strip with cluster gap, GPU histogram)
- [x] Menu bar modes extended: "Own item: graph" and "Own item: value" per module; each own-item module gets a separate status item
- [ ] ⌘-drag reorder of status items persists across launches (approach chosen in the phase 0 spike) Built (`autosaveName`, D-080); the persistence check needs a human and is in the QA list.
- [x] Onboarding "Graph per module" style card
- [ ] Re-measure WindowServer cost with every module in its own item; must stay under 0.2% added Not resolved: parallel load swamped the measurement (D-080); moved to the QA list.

Acceptance criteria
- 30-day Timeline first render under 500 ms on a DB filled with 30 days of synthetic data.
- The heatmap matches the original Timeline mock's heatmap, including hatched cells and the highlighted hour.
- CSV round-trip test passes; a 30-day export finishes under 3 s.
- Tray budget still met with all items enabled.

Status (2026-10-05): the first three are met (30d first render 38.6 ms, 30-day export 75.1 ms, round-trip test passes; heatmap checked against the original Timeline mock with differences named in PROGRESS). The tray budget is met for the default Combined item; own-item configurations measured +0.2 to +1.5 points of a core under heavy load and the WindowServer part is open (D-080, QA list).

### v1.2: per-process network and GPU, annotations, first alert

Result: runs from source (D-078).

Phase 1.2-A: per-process network
- [x] NetworkStatistics collector in process (NStatManager; works unprivileged for the user's own flows, D-081/D-082), `Cadence::OnDemand`, `Entitlement::NetworkStatistics`
- [x] Sampled only while a view with network process interest is visible; zero cost otherwise, verified with perf_gates call counters and `dump --perf` (D-082)
- [x] Per-process rx and tx rates merged into `ProcessSample`
- [x] Overview Network card switches from interfaces to top-5 processes; Network page and Processes page gain columns
- [x] If the API is unavailable, the columns are hidden and the interface list stays

Phase 1.2-B: per-process GPU
- [x] Collector reading IORegistry GPU client `accumulatedGPUTime` deltas (verified unprivileged on macOS 27, D-085), `Cadence::OnDemand`, `Entitlement::IoRegistryGpuClients`
- [x] GPU percent per process merged into `ProcessSample`
- [x] Overview GPU card switches from the 60 s chart to top-5 processes; GPU page gains its process table; Processes page gains a GPU column set

Phase 1.2-C: detectors and Timeline annotations
- [x] Detector trait in the engine consuming per-tick values; each detector writes `events` rows with `seq` and attribution stored as process name strings (infra 4, 7)
- [x] `fans_ramped`: fan rpm rises by more than a threshold within a window, attributed to the top processes by CPU over the preceding minute
- [x] `thermal_state`: `thermal.state` changes level
- [x] `sustained_process`: one process above a CPU threshold for a duration
- [x] `power_spike`: package or ANE power above a rolling baseline, attributed to the top energy process
- [x] Thresholds as data in `kelvo-schema` with tests against recorded fixture series (true positives and quiet periods)
- [x] `query_events` and AnnotationRow on the Timeline with collision handling; clicking a pill moves the crosshair there
- [x] Power by component chart annotations from `power_spike` events
- [ ] Optimized charging annotation on the battery chart if a source is found (Should) Not built: no documented source (D-083).

Phase 1.2-D: first alert
- [x] Two built-in `AlertRule`s evaluated in the engine: a process above 200% CPU for 5 minutes, and thermal state Serious or worse (thresholds proposed) (infra 7)
- [x] Settings rows to enable each rule (off by default until the user enables one)
- [x] macOS notification via `tauri-plugin-notification`, requesting permission on first enable; check behavior for an ad-hoc signed app (unverified)
- [x] Cooldown per rule; fired alerts written as `events` rows of kind `alert`
- [ ] Clicking the notification opens the Timeline at the event Not built: the plugin drops clicks on desktop; needs UNUserNotificationCenter (D-084).

Acceptance criteria
- Network and GPU process columns match Activity Monitor's ordering for the top 5 on a test workload (a download in Safari, a Metal sample app).
- Over one week of the developer's own use, detectors produce fewer than one false annotation per day by manual review (proposed bar).
- At least 80% of `fans_ramped` events carry a process attribution (proposed bar).
- With all v1.2 features on and no window visible, idle CPU is unchanged from v1.1 within measurement noise.

Status (2026-10-05): idle cost is unchanged by construction and gated (perf_gates: zero NetworkStatistics calls and no GPU walk tray-only; detectors add no allocations per tick). The Activity Monitor comparison and the week-long detector bars need the user and are in the QA list.

### v1.x QA and polish (manual, from source)

Starts when v1.2 lands. The user runs the dev build day to day and files bugs and UX issues; each session fixes a batch. No release infrastructure is built here. The phase ends when the user calls the dev build ready to release.

Checks that work without a packaged app:
- [ ] Lid-close sleep gap: close the lid for several minutes, reopen, and confirm a sleep band with no interpolated line on the Timeline and module charts
- [ ] Reduce Transparency on and off: popover and dashboard fall back to opaque surfaces
- [ ] Fullscreen first-show anomaly from D-033: open the popover over a fullscreen app
- [ ] Popover resubscribe after hide and show, onboarding focus, vibrancy in light and dark
- [ ] Display sleep and wake: no reconnect loop, the charts resume with the missed span
- [ ] Status item ⌘-drag position persistence (D-037; needs v1.1's per-module items)
- [ ] `fan.mode` encoding (0 Automatic, 1 Manual) confirmed on a Mac whose fans can be forced
- [ ] Disk rates under a known load (copy a large file) match Activity Monitor
- [ ] Calibrated CPU power checked against `powermetrics` over whole PMP windows (`live_cpu_power_calibration_matches_pmp`, D-054)
- [ ] CSP checked in a locally built app (`bun run tauri build`): every page loads with no console CSP violations, IPC works (D-075 notes `tauri dev` applies no CSP)
- [ ] WindowServer cost with every module in its own item, re-measured on a quiet machine (D-080; the v1.1 measurement was swamped by parallel load)
- [ ] Alerts: switching a rule on in a locally built app shows the "Alerts are on" notification and macOS asks for Kelvo's permission (under `tauri dev` the plugin posts as Terminal, D-084)
- [ ] Detector review over a week of use: false annotations per day and the share of `fans_ramped` events with a process (the v1.2 proposed bars), including a cargo build and a game or video export
- [ ] Per-process network and GPU columns against Activity Monitor under a known download and a GPU load

Acceptance criteria
- The user has used the dev build for an extended period and closed or deferred every bug and UX issue they filed.

### v1.x release: distribution, updater and install

Starts only after QA and polish. Result: the current v1.x on GitHub Releases and in the personal tap, installable by someone who has never built from source.

- [ ] Bundle config: app icon set, DMG background and layout, ad-hoc signing (`signingIdentity: "-"`), arm64 only
- [ ] Generate a minisign key pair for the updater; private key and password in GitHub Actions secrets, public key in `tauri.conf.json`
- [ ] `tauri-plugin-updater` against a `latest.json` on GitHub Releases; respects `general.check_updates`; "Check now" in Settings
- [ ] Release workflow on a `v*` tag: build, sign update artifacts, upload DMG, `.app.tar.gz`, signature and `latest.json` to a draft release
- [ ] Verify whether an updater-installed build carries the quarantine attribute and launches without a Gatekeeper prompt (the blueprint's assumption); record the result in decisions.md
- [ ] Personal Homebrew tap repo with a `kelvo` cask pointing at the release DMG; document `brew install --cask` and the first-launch step
- [ ] README install section: DMG and tap, and the first-launch step for an unsigned app on current macOS (System Settings, Privacy & Security, Open Anyway, since Control-click Open does not bypass Gatekeeper on supported versions)
- [ ] `scripts/bench-vs-stats.sh` complete: matching modules, 10-minute runs, coalition CPU, energy via `powermetrics`, footprint; output as a Markdown table
- [ ] Run the full budget table from section 7 and architecture.md on the development Mac; results in PROGRESS.md with chip and macOS version
- [ ] Scripted manual checklist in `plan/checklists/v1.0-packaged.md`: install from DMG, onboarding, tray styles, popover show and hide, channel stop on hide (log), lid-close gap, unknown-chip scenario via a debug flag, update from the previous alpha, launch at login after reboot
- [ ] Install test on a second Mac or a fresh macOS user account
- [ ] Run the accuracy script and the packaged checklist on both supported macOS majors (27 and 26, D-028); record both versions in PROGRESS.md

Acceptance criteria
- A fresh user can install from the DMG and from the tap by following the README alone.
- Updating from the previous tag through the updater works.
- Every budget row is met, or the miss is recorded in decisions.md with the plan to fix it.

## 9. Success criteria

| Criterion | Target | Measured by | Version |
|---|---|---|---|
| Idle overhead | At or below Stats with the same modules, and under 0.5% coalition CPU | `bench-vs-stats.sh` per release | v1.0 onward |
| Accuracy | Within ±5% and ±2 °C of macmon and powermetrics | `accuracy-vs-macmon.sh` per release | v1.0 onward |
| History size | At most 150 MB for 30 days | fill test in CI, and Settings on the dev Mac after 30 days | v1.0 |
| Popover open | Under 150 ms p95 | instrumentation | v1.0 |
| Chip coverage | All M1 to M4 family chips mapped at v1.0; each sensor dump received leads to a mapping or a documented reason within one minor release | sensor map tests, GitHub issues | v1.0 onward |
| Adoption | Release download counts, tap installs (Homebrew analytics for taps, if available; unverified) and `latest.json` request counts from GitHub release stats; no in-app telemetry | GitHub release API | v1.0 onward |
| Attribution | At least 80% of fan-ramp events attributed to a process | event table review | v1.2 |
| Honesty | No chart draws a line across a gap; no sensor shown with an invented name | Vitest gap tests; sensor map review | v1.0 |

## 10. Risks and open questions

### 10.1 Risks

| ID | Risk | Impact | Mitigation | Check by |
|---|---|---|---|---|
| R1 | IOReport, SMC and HID are private interfaces and can change with a macOS release | Power & Sensors breaks on an OS update | Vendored code isolated in `kelvo-collect`; collectors report `Unsupported` instead of crashing; test on macOS betas each summer | each macOS beta |
| R2 | The idle budget is missed because of WebKit helpers rather than our Rust code | v1 cannot claim "cheaper than Stats" | Channels stop when hidden; the dashboard is destroyed after 5 minutes hidden; measure per phase, not at the end | phase 3, 5, 6 |
| R3 | `tauri-nspanel` or native vibrancy does not work with the pinned Tauri | Popover loses non-activating behavior or blur | Phase 0 spike; fallback is an objc2 panel or an opaque popover using the Reduce Transparency tokens | phase 0 |
| R4 | tauri-specta is still a release candidate and changes its API | Binding regeneration breaks | Pin exact versions; the drift check catches changes | phase 0 |
| R5 | The tray icon tween in the original mock ("150 ms tween") would multiply WindowServer work | Budget miss | Not implemented; bars jump. Revisit only if users ask and the budget has room | decided |
| R6 | Energy impact has no public formula | The "Energy" and "EI" numbers differ from Activity Monitor | Document the approximation in the column tooltip; compare ordering, not absolute values | phase 2 |
| R7 | GPU renderer and tiler utilization or the "compute" split are not available | Overview GPU legend and GPU page lose a series | Labels follow what is measured; remove series from the catalog if absent | phase 2 |
| R8 | The unsigned app's first-launch friction grows with newer macOS releases | Fewer installs | Clear README steps; tap install; Developer ID in v3 | release phase |
| R9 | Homebrew may restrict casks for unsigned apps, including in personal taps (unverified) | The tap stops working | Track Homebrew policy; DMG remains the primary path | release phase |
| R10 | Updater-installed builds might be quarantined (blueprint assumption unverified) | Each update needs the Gatekeeper step again | Verify in the release phase; document if so | release phase |
| R11 | Annotation pills overlapped in the original Timeline mock | Unreadable annotations | AnnotationRow collision layout (design-system chart rules) | v1.2 |
| R12 | Per-core and residency series inflate history on Max and Ultra chips | Budget miss on large chips | Residency is ring-only; fill test at 250 series; levers listed in architecture.md | phase 1 |
| R13 | NetworkStatistics needs privileges or is blocked | No per-process network in v1.2 | Hide the columns and keep interfaces; record in decisions.md | v1.2 |

### 10.2 Open questions

| ID | Question | Proposed answer | Decide by |
|---|---|---|---|
| Q1 | Should the Processes page allow quitting or force-quitting a process? | Resolved: yes, in v1.0, with confirmation, a refuse list and no privilege escalation (D-029, 4.14) | resolved |
| Q2 | Is "pressure %" defined as 100 minus `kern.memorystatus_level`? | Probably, matching Stats (unverified); confirm against Activity Monitor's pressure graph | phase 2 |
| Q3 | What is the second onboarding step? | Resolved: "Updates and privacy", the update-check switch plus the no-telemetry statement with history location and size (4.16) | resolved |
| Q4 | Oldest supported macOS version | Resolved: N-1, today macOS 26 and 27, tested on both (D-028) | resolved |
| Q5 | Empty-history threshold | Show the collecting overlay until 25% of the range is recorded | phase 5 |
| Q6 | Retention options beyond 30 days | 7, 30, 90 days, with a projected size; anything larger waits for real size data | phase 5 |
| Q7 | Should pausing also stop the tray? | Tray shows empty bars and "–", so a paused state is visible at a glance | phase 3 |
| Q8 | Overview bar denominators for GPU power and disk throughput | 24-hour observed maximum with floors (5 W, 500 MB/s), labelled in the tooltip | phase 5 |

## 11. Infrastructure v1 lays for later versions

Each item below is built in v1.0 even though a later version is its main consumer. The numbers match architecture.md's "Infrastructure laid in v1" section; the last row is the widget contract from the v2 roadmap.

| # | Item | Built in | Consumed by | Tasks |
|---|---|---|---|---|
| 1 | Series data model: `metric_id` plus labels, interned series, immutable layouts, Snapshot as a view only | phase 1 | v4 sync, v4.1 Linux collectors, any new chip | schema `MetricId`, `CATALOG`, `Snapshot`; store interning and layouts |
| 2 | Stable host identity: persisted UUID, `is_local` flag, `HostId` in every command, frontend keyed `hosts[hostId]` | phases 1, 3, 4 | v4 remote hosts, fleet view, host switcher | schema `HostId`; shell host UUID; commands take `HostId`; host store and query keys |
| 3 | Wire format and version skew: CBOR framing, handshake, unknown metrics ignored, skew test in CI | phase 1 | v4 agent and controller | proto framing, `Message`, handshake, fixtures, unknown-variant and unknown-metric tests |
| 4 | Sync cursors: `seq` on every row, `db_instance_uuid`, cursor reads, `Truncated` to gap, idempotent upserts | phase 1 | v4 sync | store `meta`, seq assignment, upsert test, cursor API, in-process sync test |
| 5 | Sources produce; the UI reads the local store | phases 2, 3 | v4 `RemoteSource`, a possible launchd helper | `Source` trait, `LocalSource`, `HostRegistry` |
| 6 | Platform seams: `Ticker`, `PowerSignals`, `cfg`-gated collectors with declared entitlements, `appstore` feature, Linux CI | phases 0, 2 | v4 Linux agent, v3 App Store evaluation | crate skeletons, Linux job, Ticker and PowerSignals traits and fakes, Linux stub, entitlement filter test |
| 7 | Alerts and detectors in the engine, rules as serializable data | phase 1 (types), v1.2 (evaluation) | v2.3 editor, v4 agent-side evaluation | schema `AlertRule` round-trip tests; v1.2 detectors and rules |
| 8 | Single writers: one SQLite writer, Rust-owned settings, `settings-changed` mirrors, Rust-owned channel lifecycle | phases 1, 3, 4 | v2 multiple board windows, v2.2 widget feed | store writer thread; settings owner; channel registry; settings mirror store |
| 9 | Dynamic capabilities with change events and gap semantics | phases 1, 2, 5 | v4 hosts with changing hardware and containers | `Capabilities` types; re-probe; series-disappeared state |
| 10 | Widget render components take plain, JSON-serializable props, so their data contract can be written to the widget feed JSON that v2.2's SwiftUI widgets read | phase 4 | v2.0 manifest and composer, v2.1 desktop widgets, v2.2 WidgetKit feed | widget components under `src/app/widgets/`, boundary lint rule, serializability tests |

Deliberately not built in v1, per architecture.md: the widget manifest (v2.0), the `WidgetFeedSink` (v2.2) and a launchd helper (reconsidered after v2.2). v1 does nothing that would have to be undone for them; the serializable widget props and the bus are the two seams they attach to.
