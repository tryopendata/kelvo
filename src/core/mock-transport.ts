/**
 * The mock transport: the browser dev server, Vitest and Playwright run the
 * app against it instead of Rust. It speaks the same protocol as the app
 * transport (D-049 message order, `{ status, data | error }` results) and
 * generates data with a seeded walk that lands on fixed sample values.
 *
 * Tests drive it explicitly: `tick()` produces one frame, `push(msg)` injects
 * any message. The dev server passes `autoTick` so it streams at 1 Hz.
 *
 * Scenarios (combinable): sleep-gap (the live backfill and history have a
 * sleep hole), unknown-chip (Sensors unsupported, no thermal or fan series),
 * no-battery (Battery not present), no-fans (passive cooling), paused
 * (Status paused, no frames), stale (frames stop with no status change),
 * history-unavailable (the store failed to open; history queries fail),
 * low-disk (10 s history paused), history-trimmed (the size limit trimmed
 * history 20 days back), history-locked / history-corrupt / history-too-new
 * (unavailable with that reason; Reset history works except when locked),
 * appstore (no Quit), clock-step (the auto-tick steps the clock back five
 * minutes after ten ticks), cpu-power-uncalibrated / cpu-power-seeded /
 * cpu-power-calibrated (`power.cpu_source` 1, 3 or 2, and no E-cluster
 * power series, as on the M3 Max), wide-layout (170 series, the perf gate),
 * hidden-resume (after ten ticks the window misses half an hour, then
 * resumes; the perf gate), no-process-network (`process_network` false:
 * no per-process network rates; `appstore` implies it), no-process-gpu
 * (`process_gpu` false: no per-process GPU time; `appstore` implies it).
 *
 * `query_network_by_app` splits the ring's own `net.rx` / `net.tx` rows into
 * apps (mock/net-apps.ts), so a spike on the Network chart is attributable:
 * en0 carries a Docker Desktop download (80 MB/s for 30 s every 5 minutes,
 * the first ending 60 s before the mock starts) and a short `curl` burst
 * (`NET_BURSTS`). Per-app history starts 40 s after the ring's first row, so
 * longer ranges are partly measured; with `process_network` false nothing is
 * recorded, and turning Network history off stops recording from then.
 *
 * Process rows carry network rates (`NET_RATES`) only while the window's
 * view asks for them (`network`) and the capability is there, as Rust
 * samples NetworkStatistics only then (D-081); otherwise they are null.
 * GPU time (`GPU_PCT`) works the same way with `gpu` (D-085).
 *
 * The live channel follows D-066: `series` projects layouts, rows and
 * frames; the last two minutes arrive before `subscribe_live` returns and
 * the rest as `backfill_earlier` chunks of 600 rows, newest first, one per
 * task after the first frame (or after 1 s without one). Process rows are
 * shaped and paced by the window's `ProcessView`, tied to its stream. While
 * the window is hidden or the display idle no frames go out; on resume the
 * missed span arrives as `backfill` messages of at most 600 rows in time
 * order, one per task, ahead of the next frame. A clock step bumps the
 * `timeline` and the ring drops the rows at or after the stepped time.
 *
 * The tick follows the sampling interval: `intervalMs` sets the starting
 * setting, and an `update_settings` that changes it re-times the auto-tick
 * and the `Status` the windows see, as the engine does.
 *
 * `process_signal` follows the plan's rules: the refuse list, `pid_reused` on
 * a start-time mismatch, `permission_denied` for a process owned by another
 * user than `MOCK_USER`, and a signalled process leaves later process rows.
 */

import type { MockTransport, MockTransportOptions } from "./mock/contracts";
import { createMockDocker } from "./mock/docker";
import { createMockEmitters } from "./mock/emitters";
import { scenarioFlags } from "./mock/fixtures";

export type {
  MockCall,
  MockTransport,
  MockTransportOptions,
} from "./mock/contracts";
export {
  EARLIER_CHUNK_ROWS,
  MOCK_LOCAL_IPV4,
  MOCK_PUBLIC_IP,
  MOCK_USER,
  RECENT_MS,
  selectProcesses,
} from "./mock/protocol";
export { applyPatch } from "./mock/settings";

import { MockChannels } from "./mock/channels";
import { MockCommands } from "./mock/commands";
import { MockPower } from "./mock/power";
import { MockProcesses } from "./mock/processes";
import { MockQueries } from "./mock/queries";
import { MockRemotes } from "./mock/remote";
import { MockRing } from "./mock/ring";
import { MockSettings } from "./mock/settings";
import { MockShell } from "./mock/shell";
import { MockStore } from "./mock/store";
import { MockStream } from "./mock/stream";
import { MockUsageQueries } from "./mock/usage-queries";
export function createMockTransport(
  options: MockTransportOptions = {}
): MockTransport {
  const flags = scenarioFlags(options.scenarios ?? ["default"]);
  const ring = new MockRing(flags, options.now ?? (() => Date.now()), options);
  const commands = new MockCommands();
  const emitters = createMockEmitters();
  const settings = new MockSettings(options, flags, commands, emitters);
  const store = new MockStore(options, ring, settings, emitters);
  commands.unavailable = () => store.unavailable;
  const power = new MockPower(settings, emitters, flags, options);
  const remote = new MockRemotes(
    options,
    ring,
    commands,
    emitters,
    createMockTransport
  );
  const docker = createMockDocker(
    options.scenarios ?? [],
    options.now ?? Date.now,
    (next) => settings.setDocker(next)
  );
  const channels = new MockChannels(ring, power);
  const processes = new MockProcesses(
    ring,
    power,
    remote,
    commands,
    channels.push
  );
  const stream = new MockStream(
    options,
    ring,
    power,
    channels,
    processes,
    remote,
    commands,
    docker
  );
  const queries = new MockQueries(
    options,
    ring,
    power,
    store,
    remote,
    commands,
    docker
  );
  const usage = new MockUsageQueries(
    ring,
    power,
    store,
    settings,
    processes,
    queries,
    commands
  );
  const shell = new MockShell(ring, power, settings, commands, emitters);
  settings.onChanged = (before, after) => {
    store.recordNetworkSetting(before, after);
    power.updatePower();
  };
  power.onChanged = (retimed) => {
    channels.pushStatus();
    if (retimed) stream.retime();
  };
  remote.seedScenario();
  return {
    kind: "mock",
    flags,
    calls: commands.calls,
    windowLabel: () => options.windowLabel ?? "dashboard",
    push: channels.push,
    subscriberCount: () => channels.live.size,
    ...remote.methods,
    ...processes.methods,
    ...stream.methods,
    ...queries.methods,
    ...usage.methods,
    ...shell.methods,
    ...settings.methods,
    ...store.methods,
    ...ring.methods,
    ...docker.methods,
    onNavigateRequested: emitters.navigate.on,
    onSettingsChanged: emitters.settings.on,
    onHistoryHealthChanged: emitters.health.on,
    onEventRecorded: emitters.event.on,
    onCapabilitiesChanged: emitters.caps.on,
    onHostsChanged: emitters.hosts.on,
    onWindowAppearanceChanged: emitters.appearance.on,
    dispose() {
      docker.dispose();
      stream.dispose();
      remote.dispose();
      for (const e of Object.values(emitters)) e.clear();
    },
  };
}
