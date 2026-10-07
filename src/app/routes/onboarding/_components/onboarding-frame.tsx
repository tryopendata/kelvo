import type { ReactNode } from "react";
import { KelvoMark } from "~/components/kelvo-mark";
import { enter } from "~/lib/motion/enter";
import { cn } from "~/lib/utils";

/**
 * The onboarding window's frame: a top strip under the native
 * traffic lights (overlay title bar) with the step counter, the title and
 * one line under it, the body, and a footer. The whole window is the card,
 * with the CPU-cyan corner glow. Each step mounts its own frame, so the
 * title, body and footer lift in again when the step changes.
 */
export function OnboardingFrame({
  step,
  title,
  sub,
  footer,
  children,
}: {
  step: 1 | 2;
  title: string;
  sub: string;
  footer: ReactNode;
  children: ReactNode;
}) {
  const head = enter(0);
  const body = enter(1);
  const foot = enter(2, "fade");
  return (
    <main className="vt-card flex h-svh flex-col overflow-hidden rounded-none border-0 text-foreground">
      <div
        data-tauri-drag-region
        className="flex h-10 shrink-0 items-center px-4"
      >
        <span className="flex-1" />
        <span className="data-mono text-[11px] text-muted-foreground">
          {step} of 2
        </span>
      </div>
      <div
        className={cn("flex flex-col gap-1.5 px-8 pt-2", head.className)}
        style={head.style}
      >
        <KelvoMark className="mb-3 size-10" />
        <h1 className="font-[590] text-[28px] tracking-[-0.022em]">{title}</h1>
        <p className="font-normal text-[14px] text-fg-subtle">{sub}</p>
      </div>
      <div
        className={cn("min-h-0 flex-1 px-8 py-6", body.className)}
        style={body.style}
      >
        {children}
      </div>
      <footer
        className={cn(
          "flex shrink-0 items-center gap-2.5 border-border-subtle border-t px-8 pt-4 pb-6",
          foot.className
        )}
        style={foot.style}
      >
        {footer}
      </footer>
    </main>
  );
}
