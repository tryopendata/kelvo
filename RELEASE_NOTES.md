# Kelvo 0.2.0

Kelvo is a free, open-source system monitor for Apple Silicon Macs running macOS
26 or later.

- Monitor Linux hosts over SSH with a bundled headless agent, including CPU,
  memory, disks, network, sensors and containers where supported.
- Install, update and remove remote Linux agents from host settings.
- Explore rolling and fixed history ranges, with remote per-app history and disk
  space reports.
- Drill into disk usage with an on-demand space report and treemap.
- View per-app power draw and thermal zone history.
- Improved remote reconnection, stored history loading, chart navigation and
  rendering performance.

Existing users can update from Settings → General. History and settings are
preserved across updates.

## Install

Download **Kelvo_aarch64.dmg**, open it, and drag **Kelvo** into **Applications**.
Eject the disk image, then open Kelvo from Applications.

This release is ad-hoc signed, without Apple notarization. If macOS blocks the first
launch, open **System Settings → Privacy & Security → Open Anyway**, then confirm
**Open**. No build tools or Apple Developer membership are needed by users.

Kelvo remains free. Update downloads are verified with Kelvo's updater signing key.
Automatic checks can be disabled; installing an update always requires your click.
History and settings stay on your Mac across updates.

For development, the source installer remains available in the repository README.
