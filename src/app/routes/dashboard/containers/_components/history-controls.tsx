import { HistoryRangeControl } from "~/components/history-range-control";
import { DOCKER_RANGES, type DockerRange } from "../_lib/history-params";

export { DOCKER_RANGES, type DockerRange } from "../_lib/history-params";

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
