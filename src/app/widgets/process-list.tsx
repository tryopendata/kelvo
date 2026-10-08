import { InitialChip } from "./initial-chip";

export interface ProcessListRow {
  /** Stable identity ("pid:start_time" for processes, the interface name). */
  id: string;
  initial: string;
  name: string;
  value: string;
}

export interface ProcessListProps {
  rows: ProcessListRow[];
  /** Accessible name for the list ("Top processes by CPU"). */
  ariaLabel: string;
}

/** Compact top-5: initial chip, name, value. */
export function ProcessList({ rows, ariaLabel }: ProcessListProps) {
  return (
    <ul aria-label={ariaLabel} className="flex flex-col gap-1">
      {rows.map((row) => (
        <li
          key={row.id}
          className="flex h-[18px] items-center gap-2 text-[12px]"
        >
          <InitialChip text={row.initial} />
          <span className="min-w-0 flex-1 truncate font-normal text-fg-subtle">
            {row.name}
          </span>
          <span className="figures text-[11px]">{row.value}</span>
        </li>
      ))}
    </ul>
  );
}
