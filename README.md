# Kelvo

A menu bar system monitor for Apple Silicon Macs that keeps history.

Kelvo shows CPU, GPU, memory, power and sensors, network, disk and battery once a second, and stores 30 days of it on disk. When the fans spun up at 3pm, you can scroll back and see which process did it.

Kelvo is free and open source. Download the app below; no build tools are needed.

![The Kelvo dashboard switching between modules](https://raw.githubusercontent.com/tryopendata/kelvo/assets/media/dashboard.webp)

## Install

You need an Apple Silicon Mac (M1 or newer) on macOS 26 or newer. Intel Macs aren't supported because the power and sensor readings come from interfaces only Apple Silicon has.

1. [Download Kelvo for Apple Silicon](https://github.com/tryopendata/kelvo/releases/latest/download/Kelvo_aarch64.dmg).
2. Open the DMG and drag **Kelvo** into **Applications**.
3. Eject the DMG, then open **Kelvo** from Applications.
4. If macOS blocks the first launch, go to **System Settings → Privacy & Security → Open Anyway**, then confirm **Open**.

Kelvo is currently **ad-hoc signed, without Apple notarization**, so the first-launch
approval is expected. You do not need an Apple Developer account. Managed Macs may
restrict this approval; see [Apple’s instructions](https://support.apple.com/en-us/102445).

Kelvo appears in the menu bar and asks you to choose a menu bar style.

### Updates

Open **Settings → General → Check now**. When a new release is available, choose
**Install and restart**. Kelvo verifies the download’s updater signature before
installing it, restarts, and keeps your history and settings.

With **Check for updates automatically** enabled, Kelvo checks GitHub shortly after
launch and every 24 hours while running. It never installs an update without your
click. Turning this off stops background checks; **Check now** still works.
Checks contact GitHub; Kelvo sends no metrics, history or machine identifier.

If an update fails, use **Check now** to retry or download the latest DMG and replace
the app after quitting Kelvo. Run Kelvo from Applications, not from the mounted DMG.
Source-built older versions need this first DMG installation to gain the updater.
Quit the old copy and replace it in its existing Applications folder (the source
installer used `~/Applications`) so you do not keep two copies.

### Build it yourself

For contributors and users who prefer source builds, install the [Xcode Command Line Tools](https://developer.apple.com/xcode/resources/) (`xcode-select --install`), [Rust](https://rustup.rs) and [Bun](https://bun.sh), then:

```bash
git clone https://github.com/tryopendata/kelvo.git
cd kelvo
scripts/install.sh
```

Run from a checkout, the script builds that checkout as it is and doesn't pull.

During development, `bun run build:prod` (also available as `bun run build:local`)
builds the release app, quits Kelvo, replaces `~/Applications/Kelvo.app`, and opens it.

### Uninstall

Quit Kelvo from its menu bar item, then delete the app, its data and the source:

```bash
rm -rf ~/Applications/Kelvo.app
rm -rf ~/Library/Application\ Support/com.tryopendata.kelvo ~/Library/Logs/com.tryopendata.kelvo
rm -rf ~/.kelvo
```

## What it shows

| Module | What you get |
| --- | --- |
| CPU | Total and per-core load, efficiency and performance clusters, top processes |
| GPU | Utilization, frequency, GPU time per process |
| Memory | App, wired, compressed and cached memory, pressure, swap |
| Power & Sensors | Package, CPU, GPU and Neural Engine power, temperatures, fan speeds, thermal state |
| Network | Rates per interface, download and upload per process |
| Disk | Read and write rates, free space, top processes by disk activity |
| Battery | Charge, health, cycle count, power in or out, time remaining |
| Processes | Every process, with CPU, memory, disk, network and GPU columns |
| Containers (opt-in) | Local Docker CPU, memory and I/O, history, logs, storage inventory and reviewed cleanup |

Enable monitoring from Containers and choose one local Docker engine. Kelvo reads
the Unix socket directly and needs no Docker CLI. Up to 128 containers are
monitored at once; larger inventories show explicit partial coverage. Container
memory and macOS runtime footprint are separate measurements, and unavailable
runtime attribution is labelled. Logs stay in memory while viewed. OrbStack has
been exercised locally; Docker Desktop and Colima runtime validation remains open
in the [compatibility notes](plan/docker-container-monitoring-compatibility.md).

Storage & cleanup scans images, writable layers, volumes and daemon build cache
on demand, with a five-minute cache. Review stopped containers and unused resources
by size and project, then confirm selected actions. Volumes require a separate
data-loss acknowledgement; bind-mounted host files are never removed. AutoRemove
containers cannot be stopped here, and cache cleanup is enabled only for verified
engine builds. Docker-managed usage is not the VM's allocated Mac footprint or a
promise of immediately recovered Mac disk space. See the
[storage and cleanup plan](plan/docker-storage-cleanup.md) for limits and verification.

![The CPU page](https://raw.githubusercontent.com/tryopendata/kelvo/assets/media/cpu.webp)

![The Memory page](https://raw.githubusercontent.com/tryopendata/kelvo/assets/media/memory.webp)

![The Network page](https://raw.githubusercontent.com/tryopendata/kelvo/assets/media/network.webp)

The Timeline puts every module on one chart covering the last hour, 24 hours, 7 days or 30 days, and marks events on it: fans ramping, thermal state changes, a process pinning the CPU, power spikes. A heatmap shows CPU load or temperature by hour across 30 days. Any range exports to CSV.

![The Timeline](https://raw.githubusercontent.com/tryopendata/kelvo/assets/media/timeline.webp)

The menu bar can show one combined item or one item per module, each as a number, a graph or a per-core strip.

## Monitor another Mac

Open **Hosts → Add Mac** in a packaged Kelvo build. Both Macs need Apple Silicon
and macOS 26 or newer. Kelvo connects over SSH using the account signed in to the
other Mac's desktop.

1. On the other Mac, enable **System Settings → General → Sharing → Remote Login**
   and allow that account.
2. On this Mac, set up SSH key access. The in-app **SSH setup guide** explains how
   to create a key, authorize its public key on the other Mac, and unlock it in your
   SSH agent. Kelvo's connections cannot prompt for passwords or key passphrases.
3. Enter an SSH alias from `~/.ssh/config` or `user@address`. A config file is optional;
   Kelvo detects when it is missing and opens the guide. Leave Tailscale blank unless
   you use it; both addresses must reach the same Mac and account.
4. Choose **Check connection**. If the host is unknown, choose **Verify host** and
   compare its fingerprint with the other Mac before trusting it. Then connect.

Setup reuses or copies the packaged Kelvo app and enables remote monitoring and
launch at login on the other Mac. macOS approvals must be completed there. Select
the Mac in Hosts to see its live metrics and mirrored history; settings, alerts,
process control and menu-bar metrics remain local.

If connection fails, follow the recovery message and expand its details when needed.
For a saved Mac, open its **Settings → Show SSH setup guide** for verification and
access-test commands, save any address changes, then use **Retry** in Hosts.
**Recheck config** only checks for the local file; it does not test SSH access.

See the [setup and troubleshooting notes](plan/remote-macs-execution.md#setup-behavior)
for recovery behavior and the [remaining real-device checks](plan/remote-macs-execution.md#outstanding-real-device-gates).
Linux hosts are not supported yet.

## Resource use

With only the menu bar showing, Kelvo samples every 2 seconds and uses under 1% of one core. Open a window and it samples every second (adjustable from 0.5 to 60 seconds) and uses 6 to 10%, depending on the page.

Measured on an M3 Max running macOS 27, plugged in, on 2026-10-07:

| App | State | CPU (% of one core) | Memory |
| --- | --- | --- | --- |
| Kelvo | Menu bar only, default settings | 0.67% | 105 MB |
| [Stats](https://github.com/exelban/stats) 3.0.20 | Menu bar only, default settings | 4.1% | 149 MB |
| Activity Monitor | Window open, 5 s updates | 3.9% | 138 MB |

CPU is the kernel's CPU time for the app and its helper processes over 120 seconds; memory is their physical footprint. These default-setting measurements predate remote monitoring and have Docker monitoring off. Each figure is a single run on a machine busy with other builds, and Stats ran its own default modules rather than Kelvo's set. Activity Monitor had its window open, so compare it with Kelvo's 6 to 10% windowed figure, not the menu bar one.

To measure on your Mac, run `make bench`, or `make bench-vs-stats` to compare against Stats for 10 minutes each. Raw numbers are in [`plan/competitor-benchmarks.md`](plan/competitor-benchmarks.md).

## FAQ

### Does Kelvo need any permissions?

Local monitoring reads macOS data as your user, without a privileged helper. If
you turn on an alert in Settings, macOS asks once whether Kelvo may send notifications.
Monitoring another Mac requires Remote Login and SSH key access on that Mac;
setup uses a bundled SSH helper and may require native macOS approval there.

### Why does the network column only list some processes?

macOS only shows an app the network traffic of processes owned by the same user. System services run as other users, so their traffic counts toward the interface totals but isn't listed per process.

### How much disk does the history use?

About 150 MB for 30 days. History older than 7 days is stored in 15-minute buckets, and you can set a size limit in Settings.

### Where does Kelvo keep its data?

History and settings are in `~/Library/Application Support/com.tryopendata.kelvo/`,
logs in `~/Library/Logs/com.tryopendata.kelvo/`. Local monitoring keeps that data on
your Mac. If you enable remote monitoring, authorized SSH clients can read its
metrics and history; a controller keeps a separate local history mirror for each
remote Mac. This sharing uses SSH directly, with no Kelvo backend.

### Is there a Homebrew or Mac App Store version?

Use the [DMG download](https://github.com/tryopendata/kelvo/releases/latest) and in-app
updates for now. Homebrew and Mac App Store distribution are not part of this release.
Kelvo remains free; Apple notarization may be added later.

## Development

| Command | What it does |
| --- | --- |
| `bun run dev` | The full app with hot reload |
| `bun run dev:fast` | The interface only, in a browser with simulated data. Faster for UI work |
| `make check` | Lint, typecheck and every test. The pre-push hook runs it (`make hooks` installs the hooks) |

[`CLAUDE.md`](CLAUDE.md) covers the project layout and conventions, and [`plan/README.md`](plan/README.md) covers what's being built.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
