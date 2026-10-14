import type { LiveMsg, ProcessView } from "@core/generated/bindings";
import type { MockCommands } from "./commands";
import type { MockTransport } from "./contracts";
import { MOCK_HOST_ID, withGpuPct, withNetRates, withPorts } from "./fixtures";
import type { MockPower } from "./power";
import {
  effectiveMinPeriodMs,
  MOCK_USER,
  ok,
  selectProcesses,
  signalErr,
} from "./protocol";
import type { MockRemotes } from "./remote";
import type { MockRing } from "./ring";
export class MockProcesses {
  interest: { view: ProcessView; stream: number | null } | null = null;
  lastBatchMs: number | null = null;
  currentStream: number | null = null;
  private signalled = new Set<number>();
  constructor(
    private readonly ring: MockRing,
    private readonly power: MockPower,
    private readonly remote: MockRemotes,
    private readonly commands: MockCommands,
    private readonly push: (msg: LiveMsg) => void
  ) {}
  readonly processRows = () =>
    this.ring.gen.processes().filter((p) => !this.signalled.has(p.pid));
  readonly interestActive = () =>
    this.interest !== null &&
    (this.interest.stream === null ||
      this.interest.stream === this.currentStream);
  readonly sendProcesses = (ts: number) => {
    if (!this.interest || !this.interestActive()) return;
    const period = effectiveMinPeriodMs(
      this.interest.view.period_ms,
      this.power.status.performance
    );
    if (
      period !== null &&
      this.lastBatchMs !== null &&
      ts - this.lastBatchMs < period * 0.9
    ) {
      return;
    }
    this.lastBatchMs = ts;
    const net =
      this.interest.view.network === true &&
      this.ring.caps.process_network === true
        ? withNetRates(this.processRows())
        : this.processRows();
    const gpu =
      this.interest.view.gpu === true && this.ring.caps.process_gpu === true
        ? withGpuPct(net)
        : net;
    const all = this.interest.view.ports === true ? withPorts(gpu) : gpu;
    const batch = selectProcesses(all, this.interest.view);
    this.push({ kind: "processes", ts_ms: ts, rows: batch });
  };
  readonly setProcessInterest: MockTransport["setProcessInterest"] = async (
    host,
    interested,
    view,
    stream
  ) => {
    const remote = this.remote.remotes.get(host);
    if (remote)
      return remote.peer.setProcessInterest(
        MOCK_HOST_ID,
        interested,
        view,
        stream
      );
    const args = [host, interested, view, stream];
    return this.commands.hostCommand(
      "set_process_interest",
      host,
      args,
      {},
      () => {
        const wasActive = this.interestActive();
        const wasNetwork = this.interest?.view.network === true;
        const wasGpu = this.interest?.view.gpu === true;
        // A new call replaces the window's view (D-066).
        this.interest = interested
          ? { view: view ?? { limit: null, sort: [], period_ms: null }, stream }
          : null;
        // One batch right away, so a page opened with `?ticks=0` (screenshots)
        // still has rows; Rust sends its next batch on the next sample. The
        // same when a view starts asking for network rates or GPU time (a
        // column set switched), so the values show without a tick.
        const startsNetwork =
          this.interest?.view.network === true && !wasNetwork;
        const startsGpu = this.interest?.view.gpu === true && !wasGpu;
        if (
          this.interestActive() &&
          (!wasActive || startsNetwork || startsGpu) &&
          !this.power.status.paused &&
          !this.ring.flags.stale
        ) {
          this.lastBatchMs = null;
          queueMicrotask(() =>
            this.sendProcesses(this.ring.now() + this.ring.clockOffset)
          );
        }
        return ok(null);
      }
    );
  };
  readonly queryProcessesAt: MockTransport["queryProcessesAt"] = async (
    host,
    tMs
  ) => {
    return this.commands.hostCommand(
      "query_processes_at",
      host,
      [host, tMs],
      {},
      () =>
        ok({
          ts_ms: Math.floor(tMs / 10_000) * 10_000,
          resolution: "snapshot",
          rows: this.processRows().map((p) => ({
            name: p.name,
            pid: p.pid,
            cpu_pct: p.cpu_pct,
            mem_bytes: p.mem_bytes,
            threads: p.threads,
            idle_wakeups_per_s: p.idle_wakeups_per_s,
            energy: p.energy,
          })),
        })
    );
  };
  readonly processSignal: MockTransport["processSignal"] = async (
    host,
    pid,
    startTimeUs,
    kind
  ) => {
    this.commands.record("process_signal", host, pid, startTimeUs, kind);
    if (host !== MOCK_HOST_ID) return signalErr({ kind: "unknown_host", host });
    if (this.ring.flags.appstore) return signalErr({ kind: "unavailable" });
    const p = this.processRows().find((row) => row.pid === pid);
    if (!p) return signalErr({ kind: "not_found" });
    if (p.start_time_us !== startTimeUs)
      return signalErr({ kind: "pid_reused" });
    if (p.refusal) return signalErr({ kind: "refused", refusal: p.refusal });
    if (p.user !== MOCK_USER) return signalErr({ kind: "permission_denied" });
    this.signalled.add(pid);
    return { status: "ok", data: null };
  };
  readonly processInterest = () =>
    this.interestActive() ? this.interest : null;

  get methods() {
    return {
      setProcessInterest: this.setProcessInterest,
      queryProcessesAt: this.queryProcessesAt,
      processSignal: this.processSignal,
      processInterest: this.processInterest,
    };
  }
}
