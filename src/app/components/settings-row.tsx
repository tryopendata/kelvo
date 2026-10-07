import { type ReactNode, useId } from "react";

export interface SettingsRowProps {
  label: string;
  /** A fact the label lacks ("Kelvo uses about 0.4% CPU at 1s"), or nothing. */
  sub?: ReactNode;
  /** Id of the control the label names, when it is a form element. */
  htmlFor?: string;
  /** The control: Switch, Select, SegmentedControl, a value and a button. */
  children?: ReactNode;
}

/**
 * One settings row: label and optional sub on the left,
 * the control on the right, 48 px tall, a hairline between rows. Rows sit in a
 * bordered panel; the last row drops its divider.
 */
export function SettingsRow({
  label,
  sub,
  htmlFor,
  children,
}: SettingsRowProps) {
  const Label = htmlFor ? "label" : "span";
  return (
    <div className="flex min-h-12 flex-wrap items-center gap-3 border-border-subtle border-b px-4 text-[13px] last:border-b-0">
      <div className="flex min-w-0 flex-1 flex-col gap-0.5 py-2">
        <Label htmlFor={htmlFor}>{label}</Label>
        {sub && (
          <span className="font-normal text-[12px] text-muted-foreground">
            {sub}
          </span>
        )}
      </div>
      {children}
    </div>
  );
}

/**
 * A settings section: a sentence-case title over a bordered panel of
 * `SettingsRow`s. `after` sits under the panel (a note, history notices).
 */
export function SettingsPanel({
  title,
  after,
  children,
}: {
  title: string;
  after?: ReactNode;
  children?: ReactNode;
}) {
  const titleId = useId();
  return (
    <section aria-labelledby={titleId} className="flex flex-col gap-2.5">
      <h2 id={titleId} className="font-[590] text-[14px]">
        {title}
      </h2>
      <div className="overflow-hidden rounded-card border border-border bg-card">
        {children}
      </div>
      {after}
    </section>
  );
}
