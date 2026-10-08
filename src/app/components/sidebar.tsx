import { Pause } from "lucide-react";
import {
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  useLayoutEffect,
  useRef,
} from "react";
import { KelvoMark } from "~/components/kelvo-mark";
import { cn } from "~/lib/utils";

/** Icon paths (24 x 24, 1.5 px stroke). */
const ICONS = {
  overview: "M3 3h7v7H3zM14 3h7v7h-7zM14 14h7v7h-7zM3 14h7v7H3z",
  timeline: "M22 12h-4l-3 9L9 3l-3 9H2",
  cpu: "M6 6h12v12H6zM9 9h6v6H9zM9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4",
  gpu: "M3 6h18v12H3zM7 10h3v4H7zM13 10h4M13 14h4M7 18v2M17 18v2",
  memory:
    "M6 19v-3M10 19v-3M14 19v-3M18 19v-3M2 15h20M3 5h18a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM7 9h10",
  power: "M13 2 3 14h9l-1 8 10-12h-9l1-8z",
  network: "M7 3v18M3 7l4-4 4 4M17 21V3M13 17l4 4 4-4",
  disk: "M22 12H2M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11zM6 16h.01M10 16h.01",
  battery:
    "M3 7h15a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V8a1 1 0 0 1 1-1zM22 11v2M6 10v4M10 10v4",
  processes: "M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01",
  widgets: "M3 3h7v9H3zM14 3h7v5h-7zM14 12h7v9h-7zM3 16h7v5H3z",
  settings:
    "M21 4h-7M10 4H3M21 12h-9M8 12H3M21 20h-5M12 20H3M14 2v4M8 10v4M16 18v4",
} as const;

export type SidebarIcon = keyof typeof ICONS;

export interface SidebarEntry {
  /** Stable id, also what `active` matches ("overview", "cpu"). */
  id: string;
  /** Route the entry opens, "/dashboard/cpu". */
  href: string;
  label: string;
  icon: SidebarIcon;
  /** Live value ("18%", "14.8W"); omitted for disabled modules. */
  value?: string;
  /** The user switched the module off: listed dimmed, without a value. */
  disabled?: boolean;
}

export interface SidebarStatus {
  /** Effective base tick, ms. */
  intervalMs: number;
  paused: boolean;
  onBattery: boolean;
  /** No frame for three intervals while not paused (plan 4.17). */
  stale?: boolean;
}

export interface SidebarProps {
  /** Entry groups, separated by a hairline: nav, modules, tools. */
  groups: readonly { id: string; entries: readonly SidebarEntry[] }[];
  /** Id of the current entry. */
  active: string;
  status: SidebarStatus;
  /** App version, "0.4.0". */
  version: string;
  /** Called instead of following the href, so the router can navigate. */
  onNavigate?: (href: string) => void;
  /** A second footer line under the sampling state (the Performance mode marker). */
  footerNote?: ReactNode;
}

function intervalLabel(ms: number): string {
  const s = ms / 1000;
  return `${Number.isInteger(s) ? s : s.toFixed(1)}s`;
}

function SidebarItem({
  entry,
  current,
  onNavigate,
}: {
  entry: SidebarEntry;
  current: boolean;
  onNavigate?: (href: string) => void;
}) {
  const onClick = (e: MouseEvent<HTMLAnchorElement>) => {
    if (!onNavigate) return;
    e.preventDefault();
    onNavigate(entry.href);
  };
  return (
    <a
      href={entry.href}
      aria-current={current ? "page" : undefined}
      data-sidebar-item
      onClick={onClick}
      className={cn(
        "relative flex h-[30px] items-center gap-2.5 rounded-control px-2 text-fg-subtle no-underline outline-none focus-visible:ring-2 focus-visible:ring-ring",
        current ? "text-foreground" : "hover:bg-selected",
        entry.disabled && "text-muted-foreground"
      )}
    >
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden
        className={cn("shrink-0", entry.disabled && "opacity-60")}
      >
        <path d={ICONS[entry.icon]} />
      </svg>
      <span className="flex-1 truncate text-[13px]">{entry.label}</span>
      {!entry.disabled && entry.value && (
        <span className="figures min-w-9 text-right font-normal text-[11px] text-muted-foreground">
          {entry.value}
        </span>
      )}
    </a>
  );
}

/**
 * Dashboard navigation: three groups, live values per module, the
 * active row, and a footer with the sampling state and version. Up/Down move
 * focus between entries, Home/End jump to the ends, Enter opens.
 */
export function Sidebar({
  groups,
  active,
  status,
  version,
  onNavigate,
  footerNote,
}: SidebarProps) {
  const navRef = useRef<HTMLElement>(null);
  const pillRef = useRef<HTMLDivElement>(null);
  const placed = useRef(false);
  const prevActive = useRef(active);
  const layout = groups.map((g) => g.entries.length).join(",");

  // The selection is one pill that slides to the current row (framer's
  // `layoutId` move) instead of a fill on each row. Measured only when the
  // route or the row count changes, never on the 1 Hz value updates. It slides
  // only when the route changes under a visible pill; the first placement and
  // a row-count change snap.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `active` and `layout` stand in for the rows' positions
  useLayoutEffect(() => {
    const pill = pillRef.current;
    if (!pill) return;
    const moved = prevActive.current !== active;
    prevActive.current = active;
    const row = navRef.current?.querySelector<HTMLElement>(
      "[data-sidebar-item][aria-current]"
    );
    if (!row) {
      pill.style.opacity = "0";
      placed.current = false;
      return;
    }
    const snap = !(placed.current && moved);
    if (snap) pill.style.transition = "none";
    pill.style.opacity = "1";
    pill.style.transform = `translateY(${row.offsetTop}px)`;
    if (snap) {
      // Commit the position before the transition comes back.
      pill.getBoundingClientRect();
      pill.style.transition = "";
    }
    placed.current = true;
  }, [active, layout]);

  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    const keys = ["ArrowDown", "ArrowUp", "Home", "End"];
    if (!keys.includes(e.key) || !navRef.current) return;
    const items = Array.from(
      navRef.current.querySelectorAll<HTMLAnchorElement>("[data-sidebar-item]")
    );
    if (items.length === 0) return;
    const i = items.indexOf(document.activeElement as HTMLAnchorElement);
    let next = 0;
    if (e.key === "Home") next = 0;
    else if (e.key === "End") next = items.length - 1;
    else if (e.key === "ArrowDown")
      next = i < 0 ? 0 : Math.min(items.length - 1, i + 1);
    else next = i < 0 ? items.length - 1 : Math.max(0, i - 1);
    e.preventDefault();
    items[next]?.focus();
  };

  return (
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: arrow keys move focus between the links inside; the handler delegates to them
    <nav
      ref={navRef}
      aria-label="Dashboard sections"
      onKeyDown={onKeyDown}
      className="relative flex h-full w-[220px] flex-col gap-0.5 border-border-subtle border-r bg-deep px-2.5 pt-3.5 pb-3 text-foreground"
    >
      <div
        ref={pillRef}
        aria-hidden
        data-sidebar-pill
        className="pointer-events-none absolute inset-x-2.5 top-0 h-[30px] rounded-control bg-selected opacity-0 shadow-[inset_0_0_0_1px_var(--color-border)] transition-transform duration-(--motion-fast) ease-out"
      />
      {/* Room for the traffic lights over the overlay title bar, then the app's
          name; dragging either moves the window. */}
      <div aria-hidden data-tauri-drag-region className="h-[30px] shrink-0" />
      <div
        data-tauri-drag-region
        className="mb-2 flex shrink-0 items-center gap-2.5 border-border-subtle border-b px-2 pt-0.5 pb-3"
      >
        <KelvoMark className="pointer-events-none size-5" />
        <span className="pointer-events-none font-[620] text-[18px] leading-none tracking-[-0.02em]">
          Kelvo
        </span>
      </div>
      {groups.map((g, gi) => (
        <ul
          key={g.id}
          className={cn(
            "m-0 flex list-none flex-col gap-0.5 p-0",
            gi === 0 ? "pb-2" : "border-border-subtle border-t py-2"
          )}
        >
          {g.entries.map((entry) => (
            <li key={entry.id}>
              <SidebarItem
                entry={entry}
                current={entry.id === active}
                onNavigate={onNavigate}
              />
            </li>
          ))}
        </ul>
      ))}
      <div className="flex-1" />
      <div className="flex flex-col gap-1 border-border-subtle border-t px-2 pt-2.5">
        <div className="flex items-center gap-2">
          {status.paused ? (
            <>
              <Pause
                aria-hidden
                className="size-2.5 text-muted-foreground"
                strokeWidth={2.5}
              />
              <span className="font-normal text-[11px] text-muted-foreground">
                Paused
              </span>
            </>
          ) : status.stale ? (
            <>
              <span
                aria-hidden
                className="size-1.5 rounded-full border border-muted-foreground"
              />
              <span className="min-w-0 truncate font-normal text-[11px] text-muted-foreground">
                Stale · no new data
              </span>
            </>
          ) : (
            <>
              <span aria-hidden className="size-1.5 rounded-full bg-live" />
              <span className="min-w-0 truncate font-normal text-[11px] text-muted-foreground">
                Sampling every{" "}
                <span className="figures">
                  {intervalLabel(status.intervalMs)}
                </span>
                {status.onBattery && " · on battery"}
              </span>
            </>
          )}
          <span className="flex-1" />
          <span className="figures text-[10px] text-fg-faint">v{version}</span>
        </div>
        {footerNote}
      </div>
    </nav>
  );
}
