import {
  createMemoryRouter,
  Navigate,
  Outlet,
  type RouteObject,
} from "react-router";
import { useWindowAppearance } from "~/hooks/use-window-appearance";
import BatteryRoute from "~/routes/dashboard/battery/route";
import CpuRoute from "~/routes/dashboard/cpu/route";
import DiskRoute from "~/routes/dashboard/disk/route";
import GpuRoute from "~/routes/dashboard/gpu/route";
import MemoryRoute from "~/routes/dashboard/memory/route";
import NetworkRoute from "~/routes/dashboard/network/route";
import OverviewRoute from "~/routes/dashboard/overview/route";
import PendingPageRoute from "~/routes/dashboard/pending/route";
import PowerRoute from "~/routes/dashboard/power/route";
import ProcessesRoute from "~/routes/dashboard/processes/route";
import DashboardLayout from "~/routes/dashboard/route";
import SettingsRoute from "~/routes/dashboard/settings/route";
import TimelineRoute from "~/routes/dashboard/timeline/route";
import OnboardingRoute from "~/routes/onboarding/route";
import PopoverRoute from "~/routes/popover/route";
import { RouteError } from "~/routes/route-error";

/** Dashboard pages that still render the phase 5 placeholder. */
const PENDING_PAGES: string[] = [];

function Root({ theme }: { theme: "light" | "dark" | null }) {
  useWindowAppearance(theme);
  return <Outlet />;
}

export interface AppRouterOptions {
  initialEntry: string;
  /** Pin the theme (dev gallery and screenshots); null follows settings. */
  theme?: "light" | "dark" | null;
  /** Register dev-only routes (`/dev/gallery`). */
  dev?: boolean;
}

export function appRoutes({
  theme = null,
  dev = false,
}: Omit<AppRouterOptions, "initialEntry">): RouteObject[] {
  return [
    {
      element: <Root theme={theme} />,
      errorElement: <RouteError />,
      children: [
        { path: "/popover", element: <PopoverRoute /> },
        { path: "/onboarding", element: <OnboardingRoute /> },
        {
          path: "/dashboard",
          element: <DashboardLayout />,
          children: [
            { index: true, element: <Navigate to="overview" replace /> },
            { path: "overview", element: <OverviewRoute /> },
            { path: "timeline", element: <TimelineRoute /> },
            { path: "cpu", element: <CpuRoute /> },
            { path: "gpu", element: <GpuRoute /> },
            { path: "memory", element: <MemoryRoute /> },
            { path: "power", element: <PowerRoute /> },
            { path: "network", element: <NetworkRoute /> },
            { path: "disk", element: <DiskRoute /> },
            { path: "battery", element: <BatteryRoute /> },
            { path: "processes", element: <ProcessesRoute /> },
            { path: "settings", element: <SettingsRoute /> },
            ...PENDING_PAGES.map((page) => ({
              path: page,
              element: <PendingPageRoute />,
            })),
          ],
        },
        ...(dev
          ? [
              {
                path: "/dev/gallery",
                // Shown while the lazy module loads on first entry.
                HydrateFallback: () => null,
                lazy: async () => ({
                  Component: (await import("~/routes/dev/gallery/route"))
                    .default,
                }),
              },
            ]
          : []),
        { path: "*", element: <Navigate to="/dashboard/overview" replace /> },
      ],
    },
  ];
}

/**
 * Memory router for one window (architecture.md "Routing by window label").
 * There is no URL bar; the entry route comes from the window label.
 */
export function createAppRouter({ initialEntry, ...rest }: AppRouterOptions) {
  return createMemoryRouter(appRoutes(rest), {
    initialEntries: [initialEntry],
  });
}
