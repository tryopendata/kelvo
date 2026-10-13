/** Typed command fixture: models server-side weighted request aggregation. */
import type {
  DiskIoDevices,
  DiskIoPage,
  DiskIoPoint,
  DiskIoRequest,
  Gap,
} from "@core/generated/bindings";
import type { ScenarioName } from "./fixtures";

export function mockDiskDevices(
  scenarios: readonly ScenarioName[]
): DiskIoDevices {
  const comparison = scenarios.includes("disk-io-comparison");
  return {
    default_device: "disk3",
    devices: [
      {
        dev: "disk3",
        name: comparison
          ? "Internal SSD · High throughput, short requests"
          : "Apple SSD",
        internal: true,
        request_time_supported: !scenarios.includes("disk-io-unavailable"),
      },
      ...(comparison
        ? [
            {
              dev: "disk7",
              name: "External drive · Low throughput, slow requests",
              internal: false,
              request_time_supported: true,
            },
          ]
        : []),
    ],
  };
}

export function mockDiskIo(
  request: DiskIoRequest,
  scenarios: readonly ScenarioName[],
  sharedGaps: readonly Gap[] = []
): DiskIoPage {
  const bucket = Math.max(
    10_000,
    Math.ceil(
      (request.to_ms - request.from_ms) /
        Math.max(1, request.max_points) /
        10_000
    ) * 10_000
  );
  const first = Math.floor(request.from_ms / bucket) * bucket;
  const points: DiskIoPoint[] = [];
  const idle = scenarios.includes("disk-io-no-ops");
  const unsupported = scenarios.includes("disk-io-unavailable");
  let readBytes = 0;
  let writeBytes = 0;
  const gapFrom = request.to_ms - 90_000;
  const gapTo = request.to_ms - 60_000;
  const gaps = [
    ...sharedGaps.filter((g) => g.module === null || g.module === "disk"),
    ...(scenarios.includes("disk-io-gap")
      ? [
          {
            start_ms: gapFrom,
            end_ms: gapTo,
            module: "disk" as const,
            reason: "sleep" as const,
          },
        ]
      : []),
  ].filter(
    (g) =>
      g.start_ms < request.to_ms && (g.end_ms ?? Infinity) > request.from_ms
  );
  let coveredMs = 0;
  for (let t = first; t < request.to_ms; t += bucket) {
    const missing = gaps.some(
      (g) => t + bucket > g.start_ms && t < (g.end_ms ?? Infinity)
    );
    const slow = request.device === "disk7";
    const all =
      request.device === null && scenarios.includes("disk-io-comparison");
    const wave = 1 + 0.15 * Math.sin(t / 17_000);
    const read = idle
      ? 0
      : (slow
          ? 4e6
          : scenarios.includes("disk-io-comparison")
            ? 420e6
            : 220e6) * wave;
    const write = idle
      ? 0
      : (slow ? 1e6 : scenarios.includes("disk-io-comparison") ? 90e6 : 48e6) *
        wave;
    const readOps = idle ? 0 : slow ? 80 : 5000;
    const writeOps = idle ? 0 : slow ? 30 : 1200;
    const p: DiskIoPoint = {
      t,
      read_bps: missing ? null : read + (all && !idle ? 4e6 * wave : 0),
      write_bps: missing ? null : write + (all && !idle ? 1e6 * wave : 0),
      read_ops_per_sec:
        missing || unsupported ? null : readOps + (all && !idle ? 80 : 0),
      write_ops_per_sec:
        missing || unsupported ? null : writeOps + (all && !idle ? 30 : 0),
      read_request_ms:
        missing || idle || unsupported
          ? null
          : all
            ? (5000 * 0.15 + 80 * 42) / 5080
            : slow
              ? 42
              : 0.15,
      write_request_ms:
        missing || idle || unsupported
          ? null
          : all
            ? (1200 * 0.3 + 30 * 65) / 1230
            : slow
              ? 65
              : 0.3,
    };
    const seconds =
      (Math.min(t + bucket, request.to_ms) - Math.max(t, request.from_ms)) /
      1000;
    if (!missing) coveredMs += seconds * 1000;
    if (p.read_bps !== null) readBytes += p.read_bps * seconds;
    if (p.write_bps !== null) writeBytes += p.write_bps * seconds;
    points.push(p);
  }
  return {
    tier: "s10",
    bucket_ms: bucket,
    from_ms: request.from_ms,
    to_ms: request.to_ms,
    read_covered_ms: coveredMs,
    write_covered_ms: coveredMs,
    points,
    read_bytes: points.some((p) => p.read_bps !== null) ? readBytes : null,
    write_bytes: points.some((p) => p.write_bps !== null) ? writeBytes : null,
    gaps,
  };
}
