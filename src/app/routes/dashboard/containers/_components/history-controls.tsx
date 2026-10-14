import { HistoryRangeControl } from "~/components/history-range-control";
export const DOCKER_RANGES = {
  "15m": 900_000,
  "1h": 3_600_000,
  "24h": 86_400_000,
  "7d": 604_800_000,
  "30d": 2_592_000_000,
} as const;
export type DockerRange = keyof typeof DOCKER_RANGES;
const RANGES = (Object.keys(DOCKER_RANGES) as DockerRange[]).map((value) => ({
  value,
  label: value,
}));
export function HistoryControls({
  range,
  end,
  onRange,
  onEnd,
  now,
}: {
  range: DockerRange;
  end: number | null;
  onRange: (r: DockerRange) => void;
  onEnd: (end: number | null) => void;
  now: number;
}) {
  return (
    <HistoryRangeControl
      options={RANGES}
      value={range}
      onChange={onRange}
      live={end === null}
      ariaLabel="Container history range"
      previousLabel="Previous container range"
      nextLabel="Next container range"
      onPrevious={() => onEnd((end ?? now) - DOCKER_RANGES[range])}
      onNext={() =>
        onEnd(
          end === null || end + DOCKER_RANGES[range] >= now
            ? null
            : end + DOCKER_RANGES[range]
        )
      }
      onLive={() => onEnd(null)}
    />
  );
}
