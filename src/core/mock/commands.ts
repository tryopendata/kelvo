import type { CommandError, HostId } from "@core/generated/bindings";
import type { CommandResult } from "../transport";
import type { MockCall } from "./contracts";
import { MOCK_HOST_ID } from "./fixtures";
import { err } from "./protocol";
export class MockCommands {
  readonly calls: MockCall[] = [];
  unavailable: () => CommandError | null = () => null;
  readonly record = (command: string, ...args: unknown[]) => {
    this.calls.push({ command, args });
  };
  readonly unknownHost = (host: HostId) =>
    host === MOCK_HOST_ID ? null : err({ kind: "unknown_host", host });
  readonly hostCommand = <T>(
    name: string,
    host: HostId,
    args: unknown[],
    checks: {
      range?: readonly [fromMs: number, toMs: number];
      maxSpanMs?: number;
      needsHistory?: boolean;
    },
    body: () => CommandResult<T>
  ): CommandResult<T> => {
    this.record(name, ...args);
    const bad = this.unknownHost(host);
    if (bad) return bad;
    const unavailable = this.unavailable();
    if (checks.needsHistory && unavailable) return err(unavailable);
    if (checks.range) {
      const [fromMs, toMs] = checks.range;
      if (toMs < fromMs) {
        return err({
          kind: "invalid_argument",
          message: "the range ends before it starts",
        });
      }
      if (checks.maxSpanMs !== undefined && toMs - fromMs > checks.maxSpanMs) {
        return err({
          kind: "invalid_argument",
          message: "the range is longer than history keeps",
        });
      }
    }
    return body();
  };
}
