# Kelvo

A menu bar system monitor for Apple Silicon Macs that keeps history.

Kelvo shows CPU, GPU, memory, power and sensors, network, disk and battery once a second, and stores 30 days of it on disk. When the fans spun up at 3pm, you can scroll back and see which process did it.

Kelvo is pre-release. There's no signed download yet, so the installer builds it from source on your Mac.

## Install

You need an Apple Silicon Mac (M1 or newer) on macOS 26 or newer. Intel Macs aren't supported because the power and sensor readings come from interfaces only Apple Silicon has.

```bash
curl -fsSL https://raw.githubusercontent.com/tryopendata/kelvo/main/scripts/install.sh | bash
```

The script installs whichever build tools are missing (Xcode Command Line Tools, Rust, Bun), downloads the source to `~/.kelvo/src`, builds the app, copies it to `/Applications`, and opens it. The first run takes about five minutes, most of it compiling. Kelvo then appears in the menu bar and asks you to pick a menu bar style.

Because the app is built on your Mac, Gatekeeper opens it without a warning.

To update, run the same command again. It pulls the latest source, rebuilds, and replaces the app. Your history and settings are kept.

### Build it yourself

If you'd rather not pipe a script to bash, install the [Xcode Command Line Tools](https://developer.apple.com/xcode/resources/) (`xcode-select --install`), [Rust](https://rustup.rs) and [Bun](https://bun.sh), then:

```bash
git clone https://github.com/tryopendata/kelvo.git
cd kelvo
scripts/install.sh
```

Run from a checkout, the script builds that checkout as it is and doesn't pull.

### Uninstall

Quit Kelvo from its menu bar item, then delete the app, its data and the source:

```bash
rm -rf /Applications/Kelvo.app
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

The Timeline puts every module on one chart covering the last hour, 24 hours, 7 days or 30 days, and marks events on it: fans ramping, thermal state changes, a process pinning the CPU, power spikes. A heatmap shows CPU load or temperature by hour across 30 days. Any range exports to CSV.

The menu bar can show one combined item or one item per module, each as a number, a graph or a per-core strip.

## Resource use

With only the menu bar showing, Kelvo samples every 2 seconds and uses under 1% of one core. Open a window and it samples every second (adjustable from 0.5 to 60 seconds) and uses 6 to 10%, depending on the page.

Measured on an M3 Max running macOS 27, plugged in, on 2026-10-07:

| App | State | CPU (% of one core) | Memory |
| --- | --- | --- | --- |
| Kelvo | Menu bar only, default settings | 0.67% | 105 MB |
| [Stats](https://github.com/exelban/stats) 3.0.20 | Menu bar only, default settings | 4.1% | 149 MB |
| Activity Monitor | Window open, 5 s updates | 3.9% | 138 MB |

CPU is the kernel's CPU time for the app and its helper processes over 120 seconds; memory is their physical footprint. Each figure is a single run on a machine busy with other builds, and Stats ran its own default modules rather than Kelvo's set. Activity Monitor had its window open, so compare it with Kelvo's 6 to 10% windowed figure, not the menu bar one.

To measure on your Mac, run `make bench`, or `make bench-vs-stats` to compare against Stats for 10 minutes each. Raw numbers are in [`plan/competitor-benchmarks.md`](plan/competitor-benchmarks.md).

## FAQ

### Does Kelvo need any permissions?

No. It reads everything as your user, with no admin password or helper tool. If you turn on an alert in Settings, macOS asks once whether Kelvo may send notifications.

### Why does the network column only list some processes?

macOS only shows an app the network traffic of processes owned by the same user. System services run as other users, so their traffic counts toward the interface totals but isn't listed per process.

### How much disk does the history use?

About 150 MB for 30 days. History older than 7 days is stored in 15-minute buckets, and you can set a size limit in Settings.

### Where does Kelvo keep its data?

History and settings are in `~/Library/Application Support/com.tryopendata.kelvo/`, logs in `~/Library/Logs/com.tryopendata.kelvo/`. Nothing leaves your Mac.

### Will there be a DMG or Homebrew install?

Yes, once testing wraps up: a signed DMG on GitHub Releases, a Homebrew cask, and in-app updates.

## Development

| Command | What it does |
| --- | --- |
| `bun run dev` | The full app with hot reload |
| `bun run dev:fast` | The interface only, in a browser with simulated data. Faster for UI work |
| `make check` | Lint, typecheck and every test. The pre-push hook runs it (`make hooks` installs the hooks) |

[`CLAUDE.md`](CLAUDE.md) covers the project layout and conventions, and [`plan/README.md`](plan/README.md) covers what's being built.

## License

Apache-2.0. See [`LICENSE`](LICENSE).
