import { TriangleAlert } from "lucide-react";

export interface CardNoticeProps {
  /** "Sensor read failed · last value 11:02". */
  text: string;
}

/**
 * One state line inside a card (plan 4.17): a warning glyph and text, so the
 * state never rests on color and stays apart from the amber Power accent.
 */
export function CardNotice({ text }: CardNoticeProps) {
  return (
    <p
      role="status"
      className="m-0 flex items-center gap-1.5 font-normal text-[11px] text-muted-foreground"
    >
      <TriangleAlert aria-hidden className="size-3 shrink-0" strokeWidth={2} />
      <span className="min-w-0 truncate">{text}</span>
    </p>
  );
}
