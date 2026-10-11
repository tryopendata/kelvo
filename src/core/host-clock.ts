/** Source wall time anchored to monotonic elapsed time, immune to controller clock steps. */
export function hostClock(
  offsetMs: number,
  wallMs = Date.now(),
  monotonicMs = performance.now()
) {
  return () => wallMs + offsetMs + performance.now() - monotonicMs;
}
