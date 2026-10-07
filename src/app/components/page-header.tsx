import type { ReactNode } from "react";

export interface PageHeaderProps {
  title: string;
  /** One line of facts ("Apple M4 Pro · 10 performance + 4 efficiency cores"). */
  subtitle?: ReactNode;
  /** Right-aligned controls (the window segmented control on CPU). */
  actions?: ReactNode;
}

/**
 * Dashboard page header: 22 px title, a muted fact line, and
 * an optional control on the right. No kicker; the subtitle must carry facts.
 */
export function PageHeader({ title, subtitle, actions }: PageHeaderProps) {
  return (
    <header className="flex flex-wrap items-center gap-4">
      <div className="flex min-w-[220px] flex-1 flex-col gap-0.5">
        <h1 className="m-0 font-[590] text-[22px] tracking-[-0.022em]">
          {title}
        </h1>
        {subtitle && (
          <span className="font-normal text-[12px] text-muted-foreground">
            {subtitle}
          </span>
        )}
      </div>
      {actions}
    </header>
  );
}
