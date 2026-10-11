import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "~/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
export const DOCKER_RANGES = {
  "15m": 900_000,
  "1h": 3_600_000,
  "24h": 86_400_000,
  "7d": 604_800_000,
  "30d": 2_592_000_000,
} as const;
export type DockerRange = keyof typeof DOCKER_RANGES;
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
    <div className="flex items-center gap-2">
      <Select
        value={range}
        onValueChange={(value) => onRange(value as DockerRange)}
      >
        <SelectTrigger aria-label="Container history range" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {Object.keys(DOCKER_RANGES).map((value) => (
            <SelectItem key={value} value={value}>
              {value}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button
        variant="outline"
        size="icon"
        aria-label="Previous container range"
        onClick={() => onEnd((end ?? now) - DOCKER_RANGES[range])}
      >
        <ChevronLeft />
      </Button>
      {end !== null && (
        <Button
          variant="outline"
          size="icon"
          aria-label="Next container range"
          onClick={() =>
            onEnd(
              end + DOCKER_RANGES[range] >= now
                ? null
                : end + DOCKER_RANGES[range]
            )
          }
        >
          <ChevronRight />
        </Button>
      )}
      <Button
        variant="outline"
        size="sm"
        aria-pressed={end === null}
        onClick={() => onEnd(null)}
      >
        Live
      </Button>
    </div>
  );
}
