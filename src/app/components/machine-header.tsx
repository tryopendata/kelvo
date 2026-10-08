import { Card } from "~/widgets/card";

export interface MachineSpec {
  /** Field label: "Chip", "Memory", "Storage", "Battery", "Model", "Uptime". */
  label: string;
  value: string;
}

export interface MachineHeaderProps {
  /** "MacBook Pro 14-inch (M4 Pro, 2024)". */
  title: string;
  /** "macOS 27.0.1". */
  osVersion: string;
  /** "27A5320", shown after the version. */
  osBuild?: string;
  /** Two-column spec grid, filled row by row. */
  specs: readonly MachineSpec[];
}

/** Laptop line drawing with a live-looking trace in the CPU ink. */
function Illustration() {
  return (
    <svg
      width="150"
      height="96"
      viewBox="0 0 150 96"
      role="img"
      aria-label="Mac illustration"
      className="shrink-0"
    >
      <rect
        x="22"
        y="6"
        width="106"
        height="72"
        rx="5"
        fill="var(--color-raised)"
        stroke="var(--color-border-strong)"
      />
      <rect
        x="27"
        y="11"
        width="96"
        height="62"
        rx="2"
        fill="var(--color-deep)"
      />
      <path
        d="M33 60 L48 46 L60 52 L76 34 L92 44 L104 30 L117 38"
        fill="none"
        stroke="var(--color-cpu-ink)"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path d="M33 66 H117" stroke="var(--color-grid)" />
      <rect
        x="67"
        y="6"
        width="16"
        height="3"
        rx="1.5"
        fill="var(--color-deep)"
      />
      <path
        d="M6 80 H144 L138 88 H12 Z"
        fill="var(--color-raised)"
        stroke="var(--color-border-strong)"
      />
      <rect
        x="62"
        y="80"
        width="26"
        height="3"
        rx="1.5"
        fill="var(--color-deep)"
      />
    </svg>
  );
}

/**
 * Overview machine header: illustration, model name with macOS
 * version, and a two-column spec grid. Takes display strings because the
 * marketing name, GPU core count and memory type are not in `HostInfo`; the
 * route builds them.
 */
export function MachineHeader({
  title,
  osVersion,
  osBuild,
  specs,
}: MachineHeaderProps) {
  return (
    <Card
      accent="cpu"
      origin="tr"
      variant="chart"
      ariaLabel="This Mac"
      className="grid grid-cols-[150px_minmax(0,1fr)] items-center gap-6 px-5 py-4"
    >
      <Illustration />
      <div className="flex min-w-0 flex-col gap-3">
        <div className="flex flex-wrap items-baseline gap-3">
          <h2 className="m-0 font-[590] text-[20px] tracking-[-0.022em]">
            {title}
          </h2>
          <span className="font-normal text-[13px] text-muted-foreground">
            {osVersion}
            {osBuild && (
              <span className="figures text-[11px]"> ({osBuild})</span>
            )}
          </span>
        </div>
        <dl className="m-0 grid grid-cols-2 gap-x-8 gap-y-1.5">
          {specs.map((s) => (
            <div
              key={s.label}
              className="flex items-baseline gap-3 border-border-subtle border-b pb-[5px]"
            >
              <dt className="w-[72px] shrink-0 font-normal text-[11px] text-muted-foreground">
                {s.label}
              </dt>
              <dd className="m-0 truncate font-normal text-[12px] text-fg-subtle">
                {s.value}
              </dd>
            </div>
          ))}
        </dl>
      </div>
    </Card>
  );
}
