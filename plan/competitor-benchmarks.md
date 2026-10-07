# Competitor benchmarks

Measured comparisons against other macOS monitors, for the "at or below Stats" budget
(`architecture.md`, Performance budget) and as release material. Every number here is CPU
time summed over the app and every process macOS holds it responsible for (its WebKit or
XPC helpers), as percent of one core, from `crates/kelvo-collect/examples/coalition.rs`:
the tool `make bench` uses for Kelvo. Footprint is `phys_footprint` summed the same way.
Absolute numbers on the dev machine swing with load (D-077), so only numbers from the
same sitting compare.

## 2026-10-07, dev M3 Max, macOS 27, on AC

Same sitting, load average 4.8 to 7.8 (other builds running).

| App | State | CPU (% of one core) | Footprint | Run |
|---|---|---|---|---|
| Kelvo (5751e53, D-094) | menu bar only, shipped defaults (combined item: CPU, GPU, memory, temperature) | 0.672 | 104.9 MB | `make bench` tray, 120 s after 30 s warm-up |
| Stats 3.0.20 | menu bar only, no popup open, its default settings | 4.145 | 149.0 MB | `coalition --pid`, 120 s after 10 s warm-up |
| Activity Monitor | main window open, default "Normally" (5 s) updates | 3.881 | 137.8 MB | `coalition --pid`, 120 s after 5 s warm-up |

Read by eye in Activity Monitor's CPU column, Activity Monitor showed 4 to 5% and Stats
4.3%, which agree.

Where Kelvo's time went: main thread 0.29 (menu bar redraws, 14 frames a minute at 12 ms
of main thread each), engine 0.27, GCD threads 0.08, store writer 0.01; WebKit helpers
about 0. Earlier the same day, before D-094, Kelvo measured 1.21 and 1.38 tray-only.

Stats over 10 s of `sample`: about 3.6% of its main thread's samples were busy, and its
`eu.exelban.Stats.Repeater` collection queues took about 2.8% of a thread's samples
(wall-clock samples, not CPU time; the 4.1% above is the CPU figure).

### What these numbers support

- Kelvo backgrounded uses about a sixth of Stats' CPU (0.67 vs 4.1) and about 70% of its
  memory, measured the same way in the same sitting.
- Activity Monitor was measured with its window open. Kelvo's comparable state is a
  visible dashboard, which measured 6 to 10% on 2026-10-07 (Overview 10.2, Processes
  6.4), so "lower than Activity Monitor" is true only for Kelvo in the menu bar against
  Activity Monitor's window, not like for like.

### Caveats before using these publicly

- One run each. A claim needs several alternating runs (`make bench-vs-stats`, 10 minutes
  each) on a quiet machine.
- Stats ran its default module set, which was not checked against Kelvo's (CPU, GPU,
  memory, temperature). The budget's test is "with the same modules enabled": configure
  Stats to match before quoting a ratio.
- Kelvo's tray number is still over its own 0.5% target.
