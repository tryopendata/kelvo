# Kelvo 0.3.0

Kelvo is a free, open-source system monitor for Apple Silicon Macs running macOS
26 or later.

- Select a Timeline interval to zoom in, inspect event annotations and carry the
  same range into module charts. Use Back to return to the previous range.
- See recorded app usage for that interval, expand app groups into processes and
  compare usage with the previous interval. Sort the top ten app groups by CPU,
  GPU, RAM, estimated CPU power or network usage.
- Collapse individual popover cards or all of them. Kelvo saves your choices and
  fits the popover to its content.
- Keep app attribution and coverage in history, with progressively coarser older
  intervals. The default size limit is now 750 MB; existing saved limits stay in
  place. The cap and recorded activity can shorten retention.
- Temperature defaults follow macOS regional preferences. Battery visibility,
  Memory layout and pause captions are corrected.

Durable app attribution starts recording after this upgrade. Earlier unrecorded
app usage is unavailable; it cannot be reconstructed from older history.

## Updates

For this first upgrade, open **Settings → General → Check now**, then choose
**Install and restart**. Existing Linux hosts need one **Update agent** action
in Hosts to install the new updater. History and settings are preserved.

After this upgrade, automatic installation defaults on when automatic checks are
enabled. Existing check opt-outs stay disabled. Macs verify signed downloads and
wait until the dashboard, popover and other interactive windows are closed and
maintenance has finished before installing and restarting. Automatic installation
never requests administrator privileges; use Settings for a manual installation
if authorization is needed. Turn off **Install updates automatically** to review
and install releases yourself.

Linux agents check and update independently through a daily systemd timer, with
startup checking and jitter. Each host has its own update preferences and results.
Updates that drop a protocol supported by the installed release wait for a
coordinated manual upgrade.

## Install

Download **Kelvo_aarch64.dmg**, open it, and drag **Kelvo** into **Applications**.
Eject the disk image, then open Kelvo from Applications.

This release is ad-hoc signed, without Apple notarization. If macOS blocks the first
launch, open **System Settings → Privacy & Security → Open Anyway**, then confirm
**Open**. No build tools or Apple Developer membership are needed by users.

Older source builds without updater support need this first DMG installation.
Quit the old copy and replace it in its existing Applications folder (the source
installer used `~/Applications`) so you do not keep two copies.

Kelvo remains free. Update downloads are verified with Kelvo's updater signing key.
History and settings stay on your Mac across updates. For development, the source
installer remains available in the repository README.
