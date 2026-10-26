# Changelog

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
