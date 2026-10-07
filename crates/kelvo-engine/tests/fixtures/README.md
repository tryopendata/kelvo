# Detector fixtures

Real series replayed through the v1.2 detectors (`src/detect/tests.rs`, `replay`).

Format: `{"interval_ms": n, "ticks": [[fan0, fan1, thermal, package, ane], ...]}`, one
row per tick, `null` where the series had no value. `thermal` is the `thermal.state` level
(0 nominal to 3 critical); fans in rpm, power in watts.

## busy-25min.json

25 minutes (1,499 ticks at 1 s) on an Apple Silicon MacBook Pro running macOS 27,
recorded on 2026-10-05 with:

```
cargo build --release -p kelvo-engine --example dump
target/release/examples/dump --json --interest detail --seconds 1500 > rec.jsonl
```

then reduced to the five columns (`sensors.fans[fan=0|1].rpm`, `sensors.thermal_state`,
`power.package`, `power.ane`). Other builds ran on the machine throughout, so it is not
an idle recording: the fans ramp from about 1,450 to 3,750 rpm five times, and stop
(0 rpm) for a little over a minute around 210 s. Thermal state stayed nominal. Package
and ANE power read as gaps on all but three ticks (D-043), so this file says nothing
about `power_spike`; the scripted fixtures cover that.
