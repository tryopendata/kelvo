# Disk space report

Status: v1 implemented on `main`, 2026-10-19 (D-114). Phase 0 ran first; its results,
the deviations from this plan and the checks still owed on real hardware are in
"Implementation record" below.

Add a **Space** view to Disk. The user starts a scan of a volume, and Kelvo shows which
folders and files use the space as a drillable treemap with a ranked table beside it.
The first results are the volume's top-level folders, filled in as each finishes; clicking a
folder replaces the treemap with that folder's largest files and folders. Scans are on
demand, local, read-only and kept in memory while the dashboard is open. Nothing is
deleted from Kelvo in this version.

## Problem

### Who

The first users from the [plan README](README.md#2-problem-and-first-users): Apple Silicon
developers. Their disks fill with Xcode DerivedData and simulators, `node_modules`, Docker
and OrbStack disk images, model weights, caches and old builds. Many have 256 or 512 GB
internal drives, which is why D-102 added a "Disk used" menu bar readout.

### What

A developer whose Mac warns that the disk is almost full struggles to find what is using
the space. Kelvo's Disk page shows that the boot volume is 91% used and how fast it is being
written to, and stops there. macOS Settings › Storage groups usage into opaque categories,
the largest of which is often "System Data" with no breakdown. The user leaves Kelvo for
DaisyDisk, GrandPerspective, `du -sh * | sort -h` or `ncdu`, or deletes things by guesswork.

### Evidence

The request comes from the project owner, who uses Kelvo daily. Its users already track used
space (D-102 readout, Volumes card, low-disk guard D-057). Paid and free single-purpose tools
exist for exactly this job, which says the job is real and recurring. There is no usage data,
since Kelvo has no telemetry.

### Why now

Disk is the only module where Kelvo shows a number (space used) without a way to explain it.
The Docker storage work (D-107) established the pattern this needs: an on-demand scan with
bounded session data, Rust-owned accounting, explicit unknowns and no new history series.

## Jobs to be done

When my disk is nearly full, I want to see what is taking the space, ranked and nested, so I
can decide what to remove or move without guessing. Functional: find the top consumers and
their paths in under a minute. Emotional: trust that the numbers are right and that looking
cannot break anything.

A second job comes with the first: "Why is used space higher than my files add up to, and
why didn't deleting that folder free anything?" The report states what it could not
attribute (other volumes in the container, snapshots, unreadable or skipped folders) instead
of leaving a silent difference, and says when deleting may not free space.

## Research

| Product | Approach | Strength | Weakness |
|---|---|---|---|
| [DaisyDisk](https://daisydiskapp.com) | Sunburst of a volume; drag items to a collector, then delete | Fast, polished, explains "hidden space" | Paid; sunburst areas are hard to compare; deletion is the product |
| [GrandPerspective](https://grandperspectiv.sourceforge.net) | Squarified treemap of files, colored by folder or type | Free; treemap shows large files at a glance | Every file is a tile, so the picture is noise with no labels; no table |
| OmniDiskSweeper | Miller columns sorted by size | Precise, scannable, keyboard friendly | No overview of proportions |
| WinDirStat, WizTree (Windows) | Tree table above, treemap below, linked selection | The pairing users learned; table gives precision, map gives shape | Cushion shading and per-extension rainbow colors read as dated |
| macOS Settings › Storage | Categories plus recommendations | Built in, knows about purgeable and iCloud | No per-folder view; "System Data" is unexplained |
| `du`, `ncdu`, `dust` | Ranked lists in the terminal | Trusted numbers, what our users already run | No proportion view; manual drill-down |

Users expect a ranked, drillable list with sizes, a proportional picture, Reveal in Finder,
and a scan that shows progress and can be cancelled. The tree-table-plus-treemap pairing from
WinDirStat is the familiar shape. Kelvo should break from GrandPerspective's one-tile-per-file
noise and from per-type rainbow coloring, and should be more honest than any of them about the
space no file accounts for.

## Experience

Qualities: Transparent (the user sees what was measured, what was not, and how old it is),
Safe (read-only, no surprise prompts, nothing that can delete), Fast (results in the time it
takes to make coffee, and the page stays responsive while it runs). Transparency costs some
visual simplicity: the unmeasured region and the skipped-locations line add complexity that
a prettier chart would hide. We accept that, per "Label what we don't know".

### Entry point and the Disk route

Disk gets a `SegmentedControl` directly below its `PageHeader`, **Activity** | **Space**,
placed and styled as in Containers (`containers/route.tsx`). The reason for a separate view:
the I/O page has one dominant region (the I/O chart), and a treemap is a second one. Stacking
it under Volumes would fail the 25% zoom test in `design-system.md`, and the two jobs ("is
storage slow?" and "what fills it?") share no time selection or data.

This splits `DiskPage` (the split its `biome-ignore` comment already asks for). The route
keeps the header and switcher; `DiskActivity` owns today's page, hooks and URL effect, and
unmounts in Space so its history queries stop. Each view sets its own header subtitle and
actions: Activity keeps the device subtitle and `WindowControl`; Space shows the selected
volume's name and capacity and no window control.

URL state: `?view=space&vol=<mount point>`. Activity's `device`, `from`, `to` and `response`
params are left in place while in Space, so switching back restores the investigation.
Activity selects an I/O device (`disk0`) and Space a volume (`/`); these are different
things, and each view shows only its own picker.

Activity's Volumes card stays where `io-performance-ux.md` put it, with an Analyze link per
row to `?view=space&vol=`. The Disk tray readout and Overview disk card keep opening Activity.

### First impression (no report yet)

Space's empty state is the existing `VolumeTable` with a Scan action per row (one optional
prop on the shared component), not a second capacity display. Rows show Finder volume names
("Macintosh HD"), which Rust must start publishing (see Volume identity). Above the table, one
sentence: "Kelvo reads file sizes only and changes nothing. Folders macOS protects are skipped
unless you include them." A disclosure lists the skipped locations with an include checkbox
each (see Permissions). Time Machine backup volumes and network volumes are not listed.

### Core action and feedback: progressive results

A full scan of a large boot volume takes tens of seconds to minutes, so the report fills in
while it runs instead of appearing at the end.

1. Within a second of Scan, Rust lists the volume root and publishes its top-level folders
   (Applications, Library, Users, opt, private and so on) with "Scanning…" in place of size.
2. While the root is incomplete, the treemap is laid out against the volume's used bytes:
   finished folders are tiles at their true share, and everything else is one region that
   shrinks as folders finish. Inside it, folders still being measured are drawn as live
   tiles at their size so far ("12.4 GB so far"), in discovery order so they grow in place,
   and the gap-hatched "Not yet measured" remainder fills the rest. A finished tile therefore never
   balloons and then collapses when a larger sibling lands. Tile positions are kept stable
   while scanning: finished tiles keep their slice order and the layout appends rather than
   re-sorting (an ordered slice-and-dice for the root during the scan, squarify once it
   completes). Re-layout happens at most once a second, with no tile animation.
3. The ranked table beside it lists every top-level folder, finished ones with size and share,
   pending ones as "Scanning…". The user's home folder is usually 70 to 90% of the Data
   volume, so its direct children publish as they finish too, and the table shows them
   indented under it; the biggest pending item shows progress instead of sitting blank.
4. A progress line above the treemap shows bytes measured against the volume's used bytes
   (labelled "about", since unattributed space means it can stop short of 100%), items
   scanned, elapsed time, the folder being read (truncated in the middle), and Cancel.
   A muted note says Kelvo will appear as a top disk reader in Activity while it scans.
5. A finished folder can be opened while others are still scanning, because its subtree is
   complete. A pending one cannot; its row says so.
6. When the scan completes, the "Not yet measured" region is replaced by the unattributed
   breakdown and the progress line becomes the summary.

The finished report, top to bottom:

1. A summary: hero "Used 418 GB" (the volume's own used space, see Volume identity), then
   "In files 371 GB", "Other volumes", "Other 47 GB", "Scanned 2 min ago · 41 s · 1.8 M items", using
   `StatStrip` for the figures with a separate Rescan button beside it (`StatStrip` is a
   render-only widget). A muted line where relevant: "About 22 GB of this is purgeable; macOS
   frees it when space is needed." and one sentence saying what Other holds, with the skipped
   folders in a "Skipped folders" disclosure. The summary's swatches double as the map legend.
2. The treemap, full card width, 360 px tall at 1280×900, showing the largest files and
   folders directly inside the current folder, two levels at once: a folder tile with room
   becomes a frame with a name row and its own largest entries inside it. Clicking a tile
   opens that folder (a nested tile opens itself, or its frame when it is a file), and the
   map zooms from the tile into its new breakdown. A breadcrumb above
   it (Macintosh HD › Users › riley › Library) is the way back up; each crumb is a button.
   Drill-down is instant because the scan already holds the subtree.
3. A table of the current folder's children: name, kind, size, share of the parent as an
   `InlineBar` plus percentage, item count, and a row menu (Reveal in Finder, Copy path,
   Rescan this folder). "Smaller items" and unattributed entries appear as rows that cannot
   be opened. A "Largest files" toggle switches the table to the 50 largest files anywhere in
   the scanned tree, with their parent paths.

Table and treemap are linked: hovering a row highlights its tile (a row inside "Smaller items"
highlights that tile) and hovering a tile highlights its row. The table is the keyboard and
screen-reader path: arrow keys move, Enter or Cmd+↓ opens a folder, Cmd+↑ goes up (Finder's
keys), and Escape goes up only when no menu or popover is open. The Reveal menu notes that
deleting may not free space at once while local snapshots or clones still reference the data.

### The treemap

Squarified layout ([Bruls, Huizing and van Wijk](https://www.win.tue.nl/~vanwijk/stm.pdf),
`d3-hierarchy`'s `treemapSquarify`, a new dependency from the d3 family already in use), area
proportional to allocated bytes. It follows the design-system chart rules.

Color encodes nothing but structure. All tiles are the Disk accent on the ramp with a 1 px
`--color-card` gutter: folder and file tiles at `--ramp-3`, the hovered or focused tile at
`--ramp-2` with a 1 px `--color-ring` outline. Folder versus file is carried by shape, not
color: folder tiles have a 2 px inset top band and a chevron when the tile is large enough,
and every tile's tooltip names its kind ("Folder · click to open", "File"). There is no
per-type or per-category hue.

Labels are text in `--color-foreground` (never accent-colored text, per light-mode ink), name
on the first line and `.figures` size on the second, drawn only when the tile fits them; a
tile too small for a label is named in its tooltip and the table.

Rust sends each folder entry a preview of its own 24 largest entries, so frames need no
extra command. When one subfolder holds at least 90% of its parent, the preview follows it
(up to three levels) and the frame's name row reads as a path, "Users › riley", since a frame
showing one giant tile says nothing. Live tiles are framed too, with "so far" in the name row.

Rust merges children smaller than 0.5% of the current folder, or beyond its 120 largest, into
one "Smaller items (N)" tile: a flat `--ramp-4` tile with its label. It is measured data, so
it gets no hatch.

The gap hatch (`--color-grid` stripes) is reserved for space Kelvo did not measure, matching
its meaning elsewhere: "Not yet measured" during a scan, and after it, "Other: snapshots, APFS
metadata, unmounted and unreadable areas". Measured unattributed space is a flat
`--color-track` tile with a label: other volumes in the same APFS container ("macOS system",
"Swap and sleep image", and user-added volumes by name), and "Skipped: protected by macOS"
when skipped locations have a known size. None of these can be opened; their tooltips say
what they are and why.

Pointer interaction lives on the tiles, which are `aria-hidden`; the SVG wrapper has an
accessible name naming the folder and its three largest children, and the table carries the
detail and all keyboard access. Drill-down zooms the map from the opened tile to the full
area (and back out on the way up) on `--motion-enter`, falling back to the cross-fade when
the tile is not on screen; both are zero under Reduce Motion and Performance mode. The treemap renders on navigation and on scan progress (at most 1/s),
never on the 1 Hz tick.

### Error and edge states

| Situation | User sees | Recovery |
|---|---|---|
| Some folders unreadable | Count in the summary; disclosure listing the first 50 paths; their size is part of Other | Include the location, if it was a skipped one |
| Locations skipped by default | "Skipped: protected by macOS" tile (sized when known) and the list of locations | Include and rescan |
| Scan cancelled | Previous completed report if any, otherwise the empty state | Scan again |
| Scan fails (volume ejected, I/O error) | "Couldn't finish scanning SanDisk Extreme: the volume was disconnected." Previous report kept and labelled with its time | Scan again |
| Dashboard hidden during a scan | Scan cancelled; on return the empty state or previous report, with "Scan cancelled when the window closed" | Scan again |
| Remote host selected | "Space reports are available for this Mac only" (same pattern as `docker-local-only`) | Switch host |
| App Store edition | Space hidden through capabilities, "Not available in this edition" | None |
| Files changed since scan | Report shows its age; Reveal on a deleted path shows "This item no longer exists" | Rescan this folder |

## Scope

### Must have (v1)

- Route split, Activity | Space switcher, `?view=space&vol=`, Analyze links from Volumes.
- Volume identity: Finder names, the Data volume resolved by APFS role, per-volume used space.
- Scan local volumes (boot Data volume and browsable external volumes; not network or Time
  Machine volumes). One scan at a time; progressive results, Cancel, Rescan.
- Protected locations skipped by default with per-location include; dataless-safe walking.
- Treemap plus linked table, drill-down, breadcrumb, keyboard support; largest files (top 50).
- Unattributed breakdown with measured and unmeasured parts kept distinct.
- Reveal in Finder, Copy path.
- Cancel on dashboard hide; one retained report within the memory budget.
- Local host only; remote and App Store edition states.

### Should have (v1 if time, else v1.1)

- Rescan this folder: re-walk one subtree and update its ancestors, for the reveal, delete in
  Finder, check loop.
- Scan a chosen folder (folder picker) instead of a whole volume.
- Hints for known regenerable folders (Xcode DerivedData, CoreSimulator devices,
  `node_modules`, Homebrew cache) as a muted, inferred label, like the Docker plan's database
  inference.
- Docker Desktop and OrbStack disk images, which live in protected app containers: show their
  size as a labelled tile linking to Containers › Storage & cleanup, if the spike finds a way
  to read that one file's allocation without the app-data prompt.

### Won't have (this version)

- Delete or Move to Trash. Kelvo is a monitor; a destructive action needs its own design
  and a decision record like D-107. Reveal in Finder hands the decision to Finder.
- Full Disk Access guidance. Under ad-hoc signing (D-113) a grant is very likely lost on every
  update, leaving a stale "on" entry that no longer works. Revisit with Developer ID signing.
- Scheduled or background scans, and FSEvents-driven live updates (idle-CPU budget).
- Persisting reports or charting space per folder over time. Volume-level used space already
  has history (`disk.used`); per-folder history would be new series with unbounded labels.
- Remote hosts. It needs a protocol message (a one-way door in `architecture.md`) and a
  remote file-system policy. Revisit with v4.
- Network volumes (slow, can hang, load someone else's server); Time Machine backup volumes.
- File-type or category breakdown. The folder tree answers the job without classification.
- Looking inside archives and disk images; they count as one file. Bundles (`.app`,
  `.photoslibrary`) are folders and are scanned like folders.

## Technical design

### Scan engine: the `du` approach, done once

macOS has no public API or system utility that returns "largest folders" without walking the
tree. Finder's "Calculate all sizes", Settings › Storage, DaisyDisk and `du` all enumerate
file metadata. The standard tool is `/usr/bin/du` (`du -x -k -d 1`), which walks with `fts`.

Two candidates go into the Phase 0 spike:

- Spawn `/usr/bin/du -x -k`, parsing its output. Familiar, trusted numbers. But it has no
  largest-files output, gives totals only to the depth asked, each drill-down below that
  depth needs another run, parallelism is limited to one process per subtree (so the home
  folder is one serial walk), and cancelling means killing processes.
- An in-process walk with `du`'s rules (no symlink following, one device, hard links once),
  reading directories with `getattrlistbulk`, the bulk metadata read that Finder-class tools
  use. One pass builds the size tree, so drill-down, largest files and Rescan this folder are
  cheap, and progress and cancellation are native.

The plan assumes the in-process walk, validated against `du` for accuracy. The spike also
checks whether APFS exposes recursive folder sizes cheaply (directory stats).

Parallelism is a shared work-stealing queue of directories, not one task per top-level
folder, so a dominant home folder spreads across all workers. Each directory node carries a
pending-children counter; when it reaches zero the node's total is final and propagates up,
which is what publishes a finished top-level folder (or a child of the home folder) during
the scan. Workers: at most 4 on internal SSDs, 1 on rotational or USB 2 external disks, at
utility QoS. Metadata only: no file opens, no Spotlight.

Walker threads set `setiopolicy_np(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
IOPOL_SCOPE_THREAD, IOPOL_MATERIALIZE_DATALESS_FILES_OFF)` so File Provider dataless files
and directories (iCloud Drive, `~/Library/CloudStorage`) are never materialized, neither
contents nor directory listings. A dataless directory that cannot be listed counts as
unscanned and is part of Other. The spike confirms the policy works for an unsandboxed app.

### Measurement semantics (Rust owns them)

Size is allocated bytes (`ATTR_FILE_ALLOCSIZE`, or `st_blocks × 512`). Logical size is not
shown. Allocated size is what a file holds, not what deleting it frees: a local Time Machine
snapshot, an APFS clone or another hard link can keep the blocks in use, and the UI says so.

- Hard links count once: track `(dev, ino)` only for entries with `nlink > 1`.
- Symlinks are not followed; their own size counts. Mount points are not crossed.
- APFS clones share blocks, so summing allocations can exceed real usage. The spike decides
  between allocated size with a note when measured exceeds used, or `ATTR_CMNEXT_PRIVATESIZE`
  (unshared bytes) with shared bytes reported separately.

### Volume identity

Today's capacity collector cannot supply what Space needs. On APFS, `statfs` returns the
container's size and free space for every volume in it (`disk.rs` header), the Data volume is
mounted `nobrowse` and filtered out by `browsable()`, so it is not in `HostInfo.boot_mounts`
on real hardware (only the mock fixture lists it), and volume names are not in the schema
(`disk/_lib/disk.ts`).

Rust adds, for Space only (no change to the `disk.*` series):

- Finder names for listed volumes (`ATTR_VOL_NAME` or the URL volume name).
- The boot container's Data volume, resolved by APFS role rather than mount flags, and its
  firmlinked display paths ("Macintosh HD › Users", not "/System/Volumes/Data/Users").
- Per-volume used space from `getattrlist(ATTR_VOL_SPACEUSED)`, falling back to the APFS
  volume size in the IORegistry. This is the progress denominator, the hero for non-boot
  volumes, and the source for sibling-volume tiles (System, VM, Preboot, user-added volumes).
  Unmounted volumes such as Recovery are named in Other's tooltip.
- Purgeable as `NSURLVolumeAvailableCapacityForImportantUsageKey` minus available capacity.
  It overlaps both counted files (evictable caches and iCloud files) and snapshots, so it is
  a note in the summary, never a tile or part of the arithmetic.

Root accounting: container used = sibling volumes + files measured on the scanned volume +
Other, with Other = the remainder, clamped at zero. If measured exceeds used (clones), Other
is omitted and a note explains why.

### Placement

No new crate and no change to `crate-edges.json`. This follows the Docker storage split.

| Layer | Owns | Location |
|---|---|---|
| `kelvo-schema` | DTOs: `SpaceVolume`, `SpaceScanStatus`, `SpaceReportSummary`, `SpaceNode`, `SpaceSubtree`, `SpaceFile` | `src/disk_space.rs` |
| `kelvo-collect` | The walker (root and options in, retained tree and counters out, cancellation flag) and volume identity. macOS enumeration and APFS lookups behind `cfg(target_os = "macos")`, a portable `std::fs` walker for Linux CI | `src/disk_space.rs`, `src/macos/space_walk.rs` |
| `kelvo-engine` | Report lifecycle: single flight, cancel, retained report, subtree, largest-files and subtree-rescan queries, root accounting | `src/disk_space.rs` |
| `src-tauri` | Commands, local-host guard, cancel on dashboard hide, Reveal in Finder | `src/disk_space.rs`, `windows.rs` |
| `src/core` | Transport methods, query keys, treemap layout math (pure, no React) | `transport`, `space-treemap.ts` |
| `src/app` | Route split, Space view under `routes/dashboard/disk/_components/space-*`, `VolumeTable` action prop | |

The walker is not a `Collector`: it has no tick, no series and no catalog entry.

### Commands

Following the Docker storage commands and polling, no new channel type:

- `disk_space_volumes() -> Vec<SpaceVolume>`: name, mount, used, role, whether scannable.
- `disk_space_start(volume, include) -> SpaceScanStatus`: `include` lists opted-in protected
  locations. Refuses while a scan runs.
- `disk_space_status() -> SpaceScanStatus`: phase, counters, current path, error, and the
  root's (and home folder's) children with a final size or `None`. Polled every 500 ms by
  TanStack Query only while a scan runs.
- `disk_space_cancel()`.
- `disk_space_report() -> Option<SpaceReportSummary>`: the retained report's summary and root.
- `disk_space_children(report_id, node_id) -> SpaceSubtree`: one level, at most 120 children
  plus the Smaller items remainder, sorted. Allowed on a finished folder during a scan. Rust
  applies both merge rules; the client only lays out (per `rules/data-boundary.md`).
- `disk_space_largest_files(report_id, limit) -> Vec<SpaceFile>`.
- `disk_space_rescan(report_id, node_id)` (Should): re-walk one subtree, update ancestors.
- `reveal_in_finder(report_id, node_id)`: resolved by node ID, so the IPC boundary never
  accepts an arbitrary path.

The mock transport gets a synthetic tree with the edge cases (hard links, a dominant home
folder, unreadable and skipped folders, a clone-heavy folder, a very deep path, a non-UTF-8
name, a folder of many mid-sized files) in the same change.

### Lifecycle, cost and bounds

- The dashboard's close button hides it and destroys it 5 minutes later (`windows.rs`
  `hide_dashboard`). The scan is cancelled on the visible to hidden transition, so it never
  runs in the backgrounded app. Navigating between views or pages does not cancel it. Sleep
  cancels it.
- One retained report in total, dropped when the dashboard window is destroyed or a new scan
  completes. The 150 MB coalition `phys_footprint` budget in `architecture.md` is defined
  with the warm panel, not the dashboard, but a report that alone approaches it is too much;
  the spike measures bytes per node and sets a retained-memory target from that (aiming at a
  few tens of MB for a typical dev Mac), and the report is gone once the dashboard is.
- Retention is relative to the parent, matching the display rule: every folder is kept, and a
  file is kept if it is among its folder's 120 largest entries and at least 0.5% of that
  folder, or among the 50 largest files overall. Other files fold into their folder's
  "Smaller items" total. Drilling into a Downloads folder of 300 × 100 MB files still names
  them.
- If the retained node count reaches the cap, deeper levels stop being retained (folded into
  their parent) but measuring continues, so totals stay correct and only drill depth is lost.
  The affected folders say "Too many items to break down further; totals are complete."
- Paths are personal data. They are not logged above debug level, not included in exports or
  diagnostics, and not sent anywhere.

### Permissions

Kelvo is unsandboxed and ad-hoc signed (D-113). Without Full Disk Access, macOS privacy
controls (TCC) deny some locations (Mail, Messages, Safari, other users' homes) and prompt for
others (Desktop, Documents, Downloads, other apps' containers under app-data protection,
removable volumes). TCC binds grants to the code signature; an ad-hoc signature's requirement
is its cdhash, which changes every release, so grants are very likely lost on each in-app
update and prompts return.

So skip-by-default is the plan of record. Before a scan, Rust builds the list of protected
locations on the selected volume (resolved per user: `~/Desktop`, `~/Documents`,
`~/Downloads`, `~/Library/Containers`, `~/Library/Group Containers`, `~/Library/Mail` and the
other TCC-gated folders the spike finds). They are not entered unless the user ticks them in
the empty state's disclosure; ticking one says macOS may ask for access. A skipped location
appears as "Skipped: protected by macOS" (sized only if its size can be read without
entering it, otherwise part of Other). External volumes say before Scan that macOS may ask for
access to removable volumes.

Desktop, Documents and Downloads are often large, so the spike checks whether reading them
prompts at all for this app; if only some prompt, only those are skipped by default.

## Phase 0: collection spike (gates the contract)

Run on the development Mac with a throwaway binary, as earlier spikes did, and record results
in PROGRESS.md.

1. Speed: parallel `/usr/bin/du -x -k` against the in-process work-stealing
   `getattrlistbulk` walk (and `std::fs` as baseline), cold and warm cache, on the real Data
   volume with its real home folder. Measure time to first finished top-level folder and
   total. Target: 1 M items in under 60 s warm, first folder under 5 s.
2. Accuracy: per-folder totals within 1% of `du -skx` on readable trees.
3. Permissions, as an ad-hoc-signed bundle: which locations prompt or deny, whether a call
   blocks while a prompt is open, whether a denial suppresses later prompts, whether grants
   survive an update to a newly signed build, and whether one known file inside an app
   container (Docker.raw, OrbStack's data image) can be sized without the app-data prompt.
4. Dataless: the materialization I/O policy on iCloud Drive and a third-party File Provider.
5. Volume identity: Finder names, Data volume by role, `ATTR_VOL_SPACEUSED` for each volume in
   the boot container, the purgeable estimate.
6. Clones: `cp -c` fixtures, allocated versus private size.
7. Memory: retained bytes per node under the retention rule, on the real volume.
8. Tick lateness: the sampler's and store commit's p99 lateness during a scan.

Results that change the experience (for example, Desktop must be skipped) go back to the
owner before Phase 1.

## Phase 1: v1

1. Schema DTOs, volume identity, walker with fixture tests, engine lifecycle tests,
   commands, bindings, mock.
2. Route split and Space view: switcher, empty state on `VolumeTable`, progressive treemap
   and table, summary, drill-down, breadcrumb, largest files, unattributed tiles, Reveal and
   Copy, remote and edition states.
3. Verification (below), CE review, decision record.

## Success criteria

| Metric | Target | Measurement |
|---|---|---|
| Find the largest consumer | Under 60 s from opening Space, on the dev Mac's boot volume | Timed task, warm cache |
| Accuracy | Folder totals within 1% of `du -skx` for readable trees | Spike script and a Rust fixture test |
| Attribution | Sibling volumes + files + Other equal container used; Other never negative | Unit test on accounting; real-volume check |
| Responsiveness | Sampler p99 tick lateness during a scan within the existing tick tolerance; UI input under 100 ms | New lateness check (spike step 8 becomes a test), Playwright with mock latency |
| Background cost | No scan work after the dashboard hides | Engine test on the hide transition; `make perf` tray-only unchanged |
| Memory | Retained report within the target set by the spike | Spike measurement, then a test on the mock tree |
| No surprises | No privacy prompt during a default scan, on first run and after an in-app update | Manual runs on a fresh user account |
| Fit | Light and dark screenshots of Space match Disk Activity, Containers › Storage & cleanup and the dev gallery, with differences named | Per `rules/verification.md` |

## Verification

Rust fixtures in temp directories: nested folders, hard links across folders, symlink loops,
a `chmod 000` folder, sparse files, APFS clones (macOS only), names that are not UTF-8, paths
over 1024 bytes, many mid-sized files in one folder, and files created or removed mid-walk.
Engine tests: single flight, cancel at each phase, hide cancellation, completion counters
publishing finished subtrees in any order, node cap stops retaining but keeps totals,
subtree remainder sums, subtree rescan updating ancestors. Vitest for the treemap math in
`src/core` (areas sum to the frame, finished tiles keep their position as others finish,
no tile below a minimum area) and render-count tests showing the treemap does not re-render
on ticks. Playwright: scan, cancel, drill down and up by mouse and keyboard (Cmd+↑, Escape
with a menu open), linked hover, largest files, skipped locations, switching to Activity and
back restoring its params, remote host state, 360 px width without page overflow. Real
hardware: one full scan of the dev Mac compared with `du`, and the permission checks above.

## Assumptions and risks

| Assumption | Confidence | Validation |
|---|---|---|
| Users want to inspect, and will delete in Finder | Medium | Watch whether the owner reaches for a delete action in daily use; revisit Won't |
| A metadata walk is fast enough without a privileged helper | Medium | Spike step 1 |
| Treemap plus table beats a table alone | Medium | It is the pattern users know; the table works on its own if the treemap is unreadable on small folders |

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Skipping protected folders hides the biggest items (Docker images, Downloads) | High | Medium | Sized skipped tiles where possible; per-location include; spike step 3 |
| Privacy prompts despite skipping | Low | High | Spike step 3; "No surprises" test after an update |
| Clone double counting makes totals exceed used | Medium | Medium | Private size or an explicit note; never a negative remainder |
| Memory on huge trees (node_modules farms) | Medium | Medium | Parent-relative retention and a node cap that keeps totals |
| Scan slows foreground work or the sampler | Low | Medium | Utility QoS, bounded workers, lateness check, Cancel always visible |
| A system or VM tile is mistaken for something deletable | Low | Low | Not openable, explained in tooltip |

## Open questions

- Clone accounting choice, after spike step 6.
- Which protected locations actually prompt for this app, after spike step 3.

## Implementation record

Phase 0, measured on the development Mac (M-series, macOS 27, warm cache) with a throwaway
binary and then the real walker behind an `#[ignore]` test: the in-process
`getattrlistbulk` walk read 1.7 M files in 3.3 s on 4 workers, against 18.6 s for
`du -x -k -d 1`. Per-folder totals matched `du -skx` within 0.1%. With the retention floor
the full walk took 3.84 s and kept 28,220 nodes (336 K without the floor).
`ATTR_VOL_SPACEUSED` gives each APFS volume's own usage, and the System, Data, Preboot,
Recovery and VM volumes summed to the container within 0.2 GB, so sibling volumes are
sized that way rather than with `statfs`.

Not measured, and still owed before the success criteria can be called met: the privacy
checks of spike step 3 on a fresh account and after an in-app update, the dataless I/O
policy against iCloud Drive and a third-party File Provider (the policy is set on every
walker thread but not observed), `cp -c` clone fixtures, retained bytes per node, and
sampler tick lateness during a scan. Clones are counted once per copy (allocated size);
when files exceed used space, Other is omitted rather than negative.

Deviations from the plan above:

- The scan cancels when the dashboard hides; sleep does not cancel it. The report is
  dropped when the dashboard window is destroyed.
- The Data volume is found by its mount path (`/System/Volumes/Data`), not by APFS role.
- No purgeable figure is shown.
- Rescan this folder and the folder picker are deferred.
- Non-boot volumes scan with 2 workers and the boot volume with 4, instead of probing for
  rotational or USB 2 media.
- An extra `disk_space_path` command returns an item's path as Finder shows it (firmlinks
  mapped back to `/Users/...`), for Copy path.
- The Activity | Space switcher sits in the page header after the time window, not in
  its own row below the header as on Containers. A row there pushed Disk I/O's fifth
  attribution row below the 900 px fold that `tests/e2e/disk-io.spec.ts` guards.
- Treemap tiles are absolutely positioned elements laid out with `d3-hierarchy`'s
  squarify, not SVG, so labels, focus rings and the hatch use ordinary CSS.

## Review record

The CE devil's-advocate review (2026-10-19) found that `statfs` cannot size APFS sibling
volumes and the Data volume is not in `boot_mounts`; purgeable overlaps files and snapshots;
growing-root layouts and one-task-per-folder parallelism would mislead and serialize on the
home folder; an absolute file-folding threshold broke drill-down; the dashboard hides rather
than closes; three hour-long retained reports were too much memory; TCC grants are
unlikely to survive ad-hoc updates; File Provider directories can materialize; the tick gate
did not exist; and the treemap's ARIA, color-only folder cue and hatch meanings conflicted
with the design system. All are addressed above. The review also settled the earlier open
questions: Volumes stays on Activity with Analyze links, and the scan cancels on hide.

## Handoff

Technical planning: start with Phase 0; the DTOs are not one-way doors (session-only, local,
not on the wire), so they can change after the spike. Keep the walker out of the sampler and
the series model. Design: extend Disk with the Containers switcher pattern below the header;
the core interaction is the linked treemap and table with breadcrumb drill-down and a
progressive root measured against used space; reuse `SectionCard`, `StatStrip`,
`VolumeTable`, `InlineBar`, table and select components; the treemap is the one new chart and
follows the ramp, hatch, label and gap rules in `design-system.md`. The accepted design is
recorded as D-114.
