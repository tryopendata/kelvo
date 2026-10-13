import type { TimeRange } from "@core/brush";
import { formatBytes, formatClockSeconds, formatSpan } from "@core/format";
import type { DiskIoPage } from "@core/generated/bindings";

/** Totals describe the server's source buckets, which can outsize the brush. */
export function DiskRangeTotals({
  page,
  requested,
  selected,
}: {
  page: DiskIoPage | undefined;
  requested: TimeRange | null;
  selected: boolean;
}) {
  const span = page
    ? page.to_ms - page.from_ms
    : requested
      ? requested.toMs - requested.fromMs
      : null;
  const scope = span === null ? "—" : formatSpan(span);
  const differs =
    page &&
    requested &&
    (page.from_ms !== requested.fromMs || page.to_ms !== requested.toMs);
  const partial =
    page &&
    span !== null &&
    (page.read_covered_ms < span || page.write_covered_ms < span);
  return (
    <section
      aria-label="Recorded device byte totals"
      className="flex min-w-0 flex-col gap-1"
      data-testid="disk-range-totals"
    >
      <div className="figures flex gap-4 text-[12px] text-fg-subtle">
        <span>
          Read · {scope}{" "}
          <strong className="font-[590] text-foreground">
            {formatBytes(page?.read_bytes ?? null)}
          </strong>
        </span>
        <span>
          Written · {scope}{" "}
          <strong className="font-[590] text-foreground">
            {formatBytes(page?.write_bytes ?? null)}
          </strong>
        </span>
      </div>
      {differs && (
        <p className="figures font-normal text-[11px] text-muted-foreground">
          Recorded totals cover {formatClockSeconds(page.from_ms)} to{" "}
          {formatClockSeconds(page.to_ms)}.{" "}
          {selected ? "Selected interval" : "Chart interval"}:{" "}
          {formatClockSeconds(requested.fromMs)} to{" "}
          {formatClockSeconds(requested.toMs)}.
        </p>
      )}
      {page && (differs || partial) && (
        <p className="figures font-normal text-[11px] text-muted-foreground">
          Read measured for {formatSpan(page.read_covered_ms)} of {scope}
          {"; "}
          written measured for {formatSpan(page.write_covered_ms)} of {scope}.
        </p>
      )}
    </section>
  );
}
