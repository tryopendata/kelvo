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
  windowMs = DOCKER_RANGES[range],
}: {
  range: DockerRange;
  end: number | null;
  onRange: (r: DockerRange) => void;
  onEnd: (end: number | null) => void;
  now: number;
  windowMs?: number | undefined;
}) {
  return (
    <HistoryRangeControl
      options={RANGES}
      value={windowMs === DOCKER_RANGES[range] ? range : ""}
      onChange={(value) => onRange(value as DockerRange)}
      live={end === null}
      ariaLabel="Container history range"
      previousLabel="Previous container range"
      nextLabel="Next container range"
      onPrevious={() => onEnd((end ?? now) - windowMs)}
      onNext={() =>
        onEnd(end === null || end + windowMs >= now ? null : end + windowMs)
      }
      onLive={() => onEnd(null)}
    />
  );
}
