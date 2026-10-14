import { ChevronLeft, ChevronRight } from "lucide-react";
import {
  SegmentedControl,
  type SegmentedOption,
} from "~/components/segmented-control";
import { Button } from "~/components/ui/button";

/** The common retained-history toolbar for Timeline and Containers. */
export function HistoryRangeControl<V extends string>({
  options,
  value,
  onChange,
  live,
  onPrevious,
  onNext,
  onLive,
  ariaLabel = "Range",
  previousLabel = "Previous range",
  nextLabel = "Next range",
}: {
  options: readonly SegmentedOption<V>[];
  value: V;
  onChange: (value: V) => void;
  live: boolean;
  onPrevious: () => void;
  onNext: () => void;
  onLive: () => void;
  ariaLabel?: string;
  previousLabel?: string;
  nextLabel?: string;
}) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <SegmentedControl
        options={options}
        value={value}
        onChange={onChange}
        ariaLabel={ariaLabel}
      />
      <div className="flex items-center gap-1">
        <Button
          variant="outline"
          size="icon"
          aria-label={previousLabel}
          onClick={onPrevious}
        >
          <ChevronLeft />
        </Button>
        {!live && (
          <Button
            variant="outline"
            size="icon"
            aria-label={nextLabel}
            onClick={onNext}
          >
            <ChevronRight />
          </Button>
        )}
      </div>
      <Button variant="outline" size="sm" aria-pressed={live} onClick={onLive}>
        Live
      </Button>
    </div>
  );
}
