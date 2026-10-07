import { describe, expect, it } from "vitest";
import { MOCK_HOST_ID, PROCESSES } from "./mock/fixtures";
import { createMockTransport } from "./mock-transport";
import {
  type ProcessSignalError,
  refusalReason,
  signalOutcome,
  signalTarget,
} from "./process-signal";

const target = { pid: 2214, startTimeUs: 5, name: "Xcode" };

describe("refusalReason", () => {
  it.each([
    "kernel_task",
    "launchd",
    "window_server",
    "login_window",
    "kelvo",
  ] as const)("says why %s is refused", (refusal) => {
    expect(refusalReason({ refusal })).toMatch(/\w/);
  });

  it("allows an ordinary app", () => {
    expect(refusalReason({ refusal: null })).toBeNull();
  });

  it("follows the refusal Rust put on the mock's rows", () => {
    const byPid = new Map(PROCESSES.map((p) => [p.pid, p.refusal]));
    expect(byPid.get(0)).toBe("kernel_task");
    expect(byPid.get(2214)).toBeNull();
  });
});

describe("signalOutcome", () => {
  it("says the EPERM sentence from the plan and nothing about escalating", () => {
    const out = signalOutcome(
      { status: "error", error: { kind: "permission_denied" } },
      target,
      "quit"
    );
    expect(out).toEqual({
      tone: "error",
      message: "Kelvo can't quit processes owned by another user",
    });
  });

  it("names the process on success and on an exited process", () => {
    expect(
      signalOutcome({ status: "ok", data: null }, target, "force_quit").message
    ).toBe("Force quit Xcode (2214)");
    expect(
      signalOutcome(
        { status: "error", error: { kind: "not_found" } },
        target,
        "quit"
      ).message
    ).toBe("Xcode (2214) has already exited");
  });

  it("explains a reused pid", () => {
    expect(
      signalOutcome(
        { status: "error", error: { kind: "pid_reused" } },
        target,
        "quit"
      ).message
    ).toMatch(/PID 2214 now belongs to a different process/);
  });

  it("maps the host and OS failure variants", () => {
    const msg = (error: ProcessSignalError) =>
      signalOutcome({ status: "error", error }, target, "quit");
    expect(msg({ kind: "remote_host", host: "h" })).toEqual({
      tone: "error",
      message: "Kelvo can only quit processes on this Mac",
    });
    expect(msg({ kind: "failed", message: "EINVAL" }).message).toBe(
      "Couldn't quit Xcode (2214): EINVAL"
    );
    expect(msg({ kind: "unknown_host", host: "h" }).tone).toBe("error");
    expect(msg({ kind: "unavailable" })).toEqual({
      tone: "error",
      message: "Quitting processes isn't available in this edition of Kelvo",
    });
  });
});

describe("mock process_signal", () => {
  const xcode = PROCESSES.find((p) => p.name === "Xcode");
  const mds = PROCESSES.find((p) => p.name === "mds_stores");
  if (!xcode || !mds) throw new Error("fixture changed");

  it("quits an own process, which then leaves the process rows", async () => {
    const t = createMockTransport();
    const rows: string[][] = [];
    await t.subscribeLive(MOCK_HOST_ID, (m) => {
      if (m.kind === "processes") rows.push(m.rows.map((p) => p.name));
    });
    await t.setProcessInterest(MOCK_HOST_ID, true, null, null);
    const { pid, startTimeUs } = signalTarget(xcode);
    const r = await t.processSignal(MOCK_HOST_ID, pid, startTimeUs, "quit");
    expect(r).toEqual({ status: "ok", data: null });
    t.tick();
    expect(rows.at(-1)).not.toContain("Xcode");
    t.dispose();
  });

  it("answers pid_reused for a stale start time", async () => {
    const t = createMockTransport();
    const r = await t.processSignal(
      MOCK_HOST_ID,
      xcode.pid,
      xcode.start_time_us + 1,
      "quit"
    );
    expect(r).toEqual({ status: "error", error: { kind: "pid_reused" } });
  });

  it("refuses kernel_task and answers EPERM for another user's process", async () => {
    const t = createMockTransport();
    const kernel = PROCESSES.find((p) => p.name === "kernel_task");
    if (!kernel) throw new Error("fixture changed");
    const refused = await t.processSignal(
      MOCK_HOST_ID,
      kernel.pid,
      kernel.start_time_us,
      "force_quit"
    );
    expect(refused.status === "error" && refused.error.kind).toBe("refused");
    const eperm = await t.processSignal(
      MOCK_HOST_ID,
      mds.pid,
      mds.start_time_us,
      "quit"
    );
    expect(eperm).toEqual({
      status: "error",
      error: { kind: "permission_denied" },
    });
  });
});
