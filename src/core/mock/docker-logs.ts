import type { CommandResult, Transport } from "@core/transport";

const ok = <T>(data: T): CommandResult<T> => ({ status: "ok", data });
export class MockDockerLogs {
  private logTimers = new Set<ReturnType<typeof setInterval>>();
  constructor(
    private readonly now: () => number,
    private readonly generation: () => number
  ) {}
  readonly subscribe: Transport["dockerLogs"] = async (request, cb) => {
    cb({
      generation: this.generation(),
      lines: Array.from(
        { length: 200 },
        (_, i) =>
          `${i === 0 ? "<script>literal text</script>" : "worker"} · processed job ${i}`
      ),
      truncated: false,
      complete: !request.follow,
      error: null,
    });
    const timer = request.follow
      ? setInterval(
          () =>
            cb({
              generation: this.generation(),
              lines: [`worker · processed job at ${this.now()}`],
              truncated: false,
              complete: false,
              error: null,
            }),
          1000
        )
      : null;
    if (timer) this.logTimers.add(timer);
    return {
      info: ok(1),
      unsubscribe: () => {
        if (timer) {
          clearInterval(timer);
          this.logTimers.delete(timer);
        }
      },
    };
  };
  clear() {
    for (const timer of this.logTimers) clearInterval(timer);
    this.logTimers.clear();
  }
}
