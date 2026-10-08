import { Info } from "lucide-react";

export interface UnsupportedNoticeProps {
  /** Model identifier, "Mac17,4". */
  modelId: string;
  onShareDump: () => void;
}

/**
 * Unknown-chip explanation: info icon, one
 * sentence on what is hidden and what still works, and the one cyan text
 * action that design-system.md allows ("Share sensor dump").
 */
export function UnsupportedNotice({
  modelId,
  onShareDump,
}: UnsupportedNoticeProps) {
  return (
    <div className="flex items-start gap-2 pt-1">
      <Info
        aria-hidden
        className="mt-px size-3.5 shrink-0 text-muted-foreground"
        strokeWidth={1.5}
      />
      <p className="m-0 font-normal text-[12px] text-muted-foreground leading-normal">
        Temperature and fan sensors aren't mapped for this chip{" "}
        <span className="figures">({modelId})</span> yet, so Power &amp; Sensors
        is hidden. CPU and GPU watts still show on their own pages.{" "}
        <button
          type="button"
          onClick={onShareDump}
          className="rounded-chip text-link outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          Share sensor dump
        </button>
      </p>
    </div>
  );
}
