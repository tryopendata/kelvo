# Changelog

## [0.3.0](https://github.com/tryopendata/kelvo/compare/v0.2.0...v0.3.0) - 2026-10-26

### Added

- Automatic installation of signed releases on Macs and Linux hosts, with
  per-host controls and update results. Macs wait until interactive windows are
  closed; Linux agents update independently through systemd.
- Timeline range selection, animated zoom and Back navigation, shared with module
  charts, plus focused event annotations.
- Recorded app usage for historical ranges, with top-ten app groups, expandable
  process details and comparison with the previous interval. Groups can be sorted
  by CPU, GPU, RAM, estimated CPU power or network usage.
- Saved compact popover cards, individual collapse controls and Collapse all /
  Expand all. The popover resizes to fit its content.

### Changed

- Stored app attribution and coverage alongside resource history, with older
  intervals kept at progressively coarser resolution. App attribution starts
  recording after the upgrade; earlier unrecorded app usage is unavailable.
- Raised the default history size limit to 750 MB. Existing valid saved limits
  are preserved; retention still depends on recorded activity and the size cap.

### Fixed

- Temperature defaults now follow macOS regional preferences while preserving
  saved choices. Battery history respects the module toggle, and unavailable
  Battery routes return to a supported page. ([#1](https://github.com/tryopendata/kelvo/issues/1))
- Memory pressure, usage and swap sections now use the full content width.
- Short and ongoing pauses no longer cover charts with prominent captions;
  pause details remain available on hover and keyboard focus.
- Historical selections retain their dates as live data arrives, recover after
  failed reads, and survive returning from container details.

## [0.2.0](https://github.com/tryopendata/kelvo/compare/v0.1.0...v0.2.0) - 2026-10-25

### Added

- Linux host monitoring over SSH, with bundled x86_64 and aarch64 agents, systemd
  installation, updates, removal and identity re-pairing.
- Linux CPU, memory, disk, network, hardware sensor, battery and power collectors
  where available, plus cgroup service and container monitoring.
- Remote per-app history and disk space reports.
- Rolling and fixed date ranges across module charts, with selection summaries.
- On-demand disk space reports with a drillable treemap.
- Per-app power draw lines and thermal zone history.

### Fixed

- Updated TLS certificate verification, Tokio, Vite and the test DOM dependency
  to resolve dependency audit findings.

- Remote feed replacement, reconnects after sleep, backoff and agent restarts.
- Loading stored remote history into live charts and preserving disk selections
  through process navigation.
- Linux agent memory accounting, stable sensor naming and counter reset handling.
- Repeated rendering in power, container and app usage views.
- Hidden volume heading layout and accessibility coverage.

## [0.1.0](https://github.com/tryopendata/kelvo/releases/tag/v0.1.0) - 2026-10-16

- First downloadable Apple Silicon release for macOS 26 or later.
- CPU, GPU, memory, power, sensors, network, disk, battery and process monitoring.
- Local history, Timeline, heatmaps, CSV export and alerts.
- Optional local Docker and remote Mac monitoring.
- Ad-hoc-signed DMG and signature-verified in-app updates.
