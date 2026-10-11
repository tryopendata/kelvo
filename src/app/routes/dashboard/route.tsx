import { type RefObject, useEffect, useLayoutEffect, useRef } from "react";
import { Outlet, useLocation, useNavigate } from "react-router";
import { useRemoteHosts } from "~/hooks/use-remote-hosts";
import { pageEnter } from "~/lib/motion/enter";
import { useTransport } from "~/lib/transport-context";
import { cn } from "~/lib/utils";
import { useHostId } from "~/stores/host-store";
import { useSampling } from "~/stores/live-selectors";
import { LiveSidebar } from "./_components/live-sidebar";
import { connectionLabel } from "./hosts/route";

/**
 * An open dashboard window follows `navigate-requested`: Rust sends it when
 * the popover or tray asks for a route (Settings, Activity) while the window
 * already exists. Only `/dashboard/...` routes are accepted.
 */
function useNavigateRequests() {
  const transport = useTransport();
  const navigate = useNavigate();
  useEffect(
    () =>
      transport.onNavigateRequested(({ route }) => {
        if (route === "/dashboard" || route.startsWith("/dashboard/")) {
          navigate(route);
        } else {
          console.warn("[dashboard] ignored navigate-requested", { route });
        }
      }),
    [transport, navigate]
  );
}

/**
 * The page scrolls inside `<main>`, which outlives route changes, so a new
 * page would otherwise open at the previous page's offset. Reset before paint.
 */
function useScrollTopOnNavigate(ref: RefObject<HTMLElement | null>) {
  const { pathname } = useLocation();
  // biome-ignore lint/correctness/useExhaustiveDependencies: pathname is the trigger, not an input
  useLayoutEffect(() => {
    if (ref.current) ref.current.scrollTop = 0;
  }, [pathname, ref]);
}

/**
 * Dashboard window shell (plan 4.4): the 220 px sidebar under the
 * overlay title bar, and the page. A stale stream (no frame for three
 * intervals) dims the page's values to 50% until the next frame. Each page's
 * sections lift in on navigation (`pageEnter`); the sidebar stays put.
 */
export default function DashboardLayout() {
  useNavigateRequests();
  const mainRef = useRef<HTMLElement>(null);
  useScrollTopOnNavigate(mainRef);
  const sampling = useSampling();
  const hostId = useHostId();
  const remote = useRemoteHosts().data?.find(
    (host) => host.config.id === hostId
  );
  const { pathname } = useLocation();
  const metricPage =
    pathname !== "/dashboard/hosts" && pathname !== "/dashboard/settings";
  const stale =
    metricPage &&
    ((sampling.stale && !sampling.paused) ||
      (remote !== undefined && remote.status.state !== "connected"));
  return (
    <div className="flex h-svh bg-background text-foreground">
      <LiveSidebar />
      <main
        ref={mainRef}
        data-stale={stale || undefined}
        className={cn(
          "min-w-0 flex-1 overflow-y-auto px-6 pt-5 pb-6 transition-opacity duration-(--motion-crossfade)",
          pageEnter,
          stale && "opacity-50"
        )}
      >
        {remote && metricPage && (
          <div
            role="status"
            className="mb-4 border-border-subtle border-b pb-3 text-[12px] text-muted-foreground"
          >
            {remote.config.name} · {connectionLabel(remote)}
            {remote.status.state !== "connected" &&
              " · live readings are stale; saved history remains available"}
            {Math.abs(remote.status.clock_offset_ms ?? 0) >= 30000 &&
              " · chart times follow this Mac’s clock"}
            {remote.status.error && (
              <p className="mt-1 text-destructive">{remote.status.error}</p>
            )}
          </div>
        )}
        <Outlet />
      </main>
    </div>
  );
}
