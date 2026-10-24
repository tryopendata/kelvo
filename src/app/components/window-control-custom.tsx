import { chartRange, customRange, dateInput } from "@core/chart-range";
import type { ChartWindow } from "@core/generated/bindings";
import { useId, useState } from "react";
import { SegmentedControl } from "~/components/segmented-control";
import { Button } from "~/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { useHostClock } from "~/lib/host-clock";

const INPUT =
  "figures h-8 min-w-0 rounded-md border border-input bg-transparent px-2 text-[12px] outline-none focus-visible:ring-2 focus-visible:ring-ring aria-invalid:border-destructive";
const UNITS = { minutes: 60_000, hours: 3_600_000, days: 86_400_000 };

export function CustomRangeForm({
  value,
  intervalMs,
  onApply,
}: {
  value: ChartWindow;
  intervalMs: number;
  onApply: (value: ChartWindow) => void;
}) {
  const clock = useHostClock();
  const range = chartRange(value);
  const id = useId();
  const [mode, setMode] = useState(
    range.endMs === null ? "relative" : "absolute"
  );
  const [unit, setUnit] = useState<keyof typeof UNITS>(
    range.windowMs % UNITS.hours === 0 ? "hours" : "minutes"
  );
  const [amount, setAmount] = useState(String(range.windowMs / UNITS[unit]));
  const [from, setFrom] = useState(
    dateInput((range.endMs ?? clock()) - range.windowMs)
  );
  const [to, setTo] = useState(dateInput(range.endMs ?? clock()));
  const [error, setError] = useState<string | null>(null);
  const submit = () => {
    const result = customRange({
      mode,
      amount,
      unitMs: UNITS[unit],
      from,
      to,
      now: clock(),
      intervalMs,
    });
    if ("error" in result) setError(result.error);
    else onApply(result.value);
  };
  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <SegmentedControl
        options={[
          { value: "relative", label: "Relative" },
          { value: "absolute", label: "Date range" },
        ]}
        value={mode}
        onChange={(next) => {
          setMode(next);
          setError(null);
        }}
        ariaLabel="Custom range type"
      />
      {mode === "relative" ? (
        <div className="flex items-center gap-2">
          <label htmlFor={`${id}-amount`}>Last</label>
          <input
            id={`${id}-amount`}
            aria-label="Duration"
            className={`${INPUT} w-20`}
            inputMode="decimal"
            value={amount}
            onChange={(event) => {
              setAmount(event.target.value);
              setError(null);
            }}
            aria-invalid={error !== null}
            aria-describedby={error ? `${id}-error` : undefined}
          />
          <Select
            value={unit}
            onValueChange={(next) => setUnit(next as keyof typeof UNITS)}
          >
            <SelectTrigger aria-label="Duration unit" size="sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {Object.keys(UNITS).map((key) => (
                <SelectItem key={key} value={key}>
                  {key}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      ) : (
        <div className="grid grid-cols-[auto_1fr] items-center gap-x-3 gap-y-2">
          <label htmlFor={`${id}-from`}>From</label>
          <input
            id={`${id}-from`}
            className={INPUT}
            placeholder="YYYY-MM-DD HH:mm"
            value={from}
            onChange={(event) => {
              setFrom(event.target.value);
              setError(null);
            }}
            aria-invalid={error !== null}
            aria-describedby={`${id}-zone`}
          />
          <label htmlFor={`${id}-to`}>To</label>
          <input
            id={`${id}-to`}
            className={INPUT}
            placeholder="YYYY-MM-DD HH:mm"
            value={to}
            onChange={(event) => {
              setTo(event.target.value);
              setError(null);
            }}
            aria-invalid={error !== null}
            aria-describedby={`${id}-zone`}
          />
          <p
            id={`${id}-zone`}
            className="col-span-2 text-[11px] text-muted-foreground"
          >
            YYYY-MM-DD HH:mm ·{" "}
            {Intl.DateTimeFormat().resolvedOptions().timeZone}
          </p>
        </div>
      )}
      {error && (
        <p
          id={`${id}-error`}
          role="alert"
          className="text-[11px] text-destructive"
        >
          {error}
        </p>
      )}
      <div className="flex items-center justify-between gap-3">
        <span className="text-[11px] text-muted-foreground">
          {mode === "relative" ? "Follows now" : "Stays at these dates"}
        </span>
        <Button size="sm" type="submit">
          Apply
        </Button>
      </div>
    </form>
  );
}
