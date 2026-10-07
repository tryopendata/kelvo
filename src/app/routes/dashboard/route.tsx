import { useEffect } from "react";
import { Outlet, useNavigate } from "react-router";
import { pageEnter } from "~/lib/motion/enter";
import { useTransport } from "~/lib/transport-context";
import { cn } from "~/lib/utils";
import { useSampling } from "~/stores/live-selectors";
import { LiveSidebar } from "./_components/live-sidebar";

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
 * Dashboard window shell (plan 4.4): the 220 px sidebar under the
 * overlay title bar, and the page. A stale stream (no frame for three
 * intervals) dims the page's values to 50% until the next frame. Each page's
 * sections lift in on navigation (`pageEnter`); the sidebar stays put.
 */
export default function DashboardLayout() {
  useNavigateRequests();
  const sampling = useSampling();
  const stale = sampling.stale && !sampling.paused;
  return (
    <div className="flex h-svh bg-background text-foreground">
      <LiveSidebar />
      <main
        data-stale={stale || undefined}
        className={cn(
          "min-w-0 flex-1 overflow-y-auto px-6 pt-5 pb-6 transition-opacity duration-(--motion-crossfade)",
          pageEnter,
          stale && "opacity-50"
        )}
      >
        <Outlet />
      </main>
    </div>
  );
}
