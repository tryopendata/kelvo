import { RING_SPAN_MS, type SeriesSelector } from "@core/generated/bindings";
import { scaledWindowMs, slowestIntervalMs } from "@core/live-window";
import { hostKeys } from "@core/query-keys";
import type { Transport } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { RouterProvider } from "react-router";
import { Toaster } from "~/components/ui/sonner";
import { TooltipProvider } from "~/components/ui/tooltip";
import { QueryProvider } from "~/lib/query";
import { TransportProvider, useTransport } from "~/lib/transport-context";
import { type AppRouterOptions, createAppRouter } from "~/router";
import { POPOVER_SERIES } from "~/routes/popover/_lib/series";
import { HostStoreProvider } from "~/stores/host-store";
import { SettingsProvider, useSettings } from "~/stores/settings-store";

/**
 * Ring history a window asks for on subscribe. The dashboard's module pages
 * draw up to an hour from the ring (CPU 1h window, 10-minute heatmap and
 * power stack, plan 4.7 to 4.10). The popover's charts hold 60 samples, so
 * it asks for 60 samples at the slowest interval the settings allow (60 s at
 * 1 s, 30 minutes at 30 s); other windows take the 60 s default.
 */
const BACKFILL_MS_BY_WINDOW: Record<string, number | undefined> = {
  dashboard: RING_SPAN_MS,
};

/**
 * The series a window's channel carries (D-066). The popover names what its
 * cards draw. The dashboard takes every series: its routes share one
 * subscription and one hour of ring, and a projection per route would
 * resubscribe and refill the ring on every navigation.
 */
const SERIES_BY_WINDOW: Record<string, readonly SeriesSelector[] | undefined> =
  {
    popover: POPOVER_SERIES,
  };

/**
 * Keep the host list and records current: `hosts-changed` carries every
 * host when one changes (the local host learning `chip_known`).
 */
function useHostsChanged() {
  const transport = useTransport();
  const queryClient = useQueryClient();
  useEffect(
    () =>
      transport.onHostsChanged(({ hosts }) => {
        queryClient.setQueryData(hostKeys.list(), hosts);
        for (const h of hosts)
          queryClient.setQueryData(hostKeys.detail(h.id), h);
      }),
    [transport, queryClient]
  );
}

/**
 * The popover's backfill, fixed at first settings read: a later interval
 * change must not resubscribe, since rows already held win over a backfill.
 * It waits for the window appearance too, since Low Power Mode doubles the
 * interval without being a setting (D-088).
 */
function usePopoverBackfill(enabled: boolean): number | null {
  const transport = useTransport();
  const sampling = useSettings((s) => s.sampling);
  const [lowPower, setLowPower] = useState<boolean | null>(null);
  const fixed = useRef<number | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    transport.getWindowAppearance().then(
      (a) => {
        if (!cancelled) setLowPower(a.performance === "low_power_mode");
      },
      (err: unknown) => {
        console.error("[app] get_window_appearance failed", err);
        if (!cancelled) setLowPower(false);
      }
    );
    return () => {
      cancelled = true;
    };
  }, [enabled, transport]);
  if (enabled && fixed.current === null && sampling && lowPower !== null) {
    fixed.current = scaledWindowMs(
      60_000,
      slowestIntervalMs(sampling, lowPower)
    );
  }
  return fixed.current;
}

/** Waits for the host list, then scopes a live store to the local host. */
function LocalHost({ router }: { router: ReturnType<typeof createAppRouter> }) {
  const transport = useTransport();
  const { data: hosts, error } = useQuery({
    queryKey: hostKeys.list(),
    queryFn: () => transport.listHosts(),
    staleTime: Infinity,
  });
  useHostsChanged();
  if (error) {
    console.error("[app] list_hosts failed", {
      window: transport.windowLabel(),
      error,
    });
  }
  const label = transport.windowLabel();
  const popoverBackfill = usePopoverBackfill(label === "popover");
  const local = hosts?.find((h) => h.is_local) ?? hosts?.[0];
  if (!local) return null;
  // The popover waits for settings so its first subscribe asks for enough.
  if (label === "popover" && popoverBackfill === null) return null;
  return (
    <HostStoreProvider
      key={local.id}
      hostId={local.id}
      backfillMs={popoverBackfill ?? BACKFILL_MS_BY_WINDOW[label]}
      series={SERIES_BY_WINDOW[label]}
    >
      <RouterProvider router={router} />
    </HostStoreProvider>
  );
}

/** Providers for one window: transport, query cache, settings, live store, router. */
export function App({
  transport,
  router: routerOptions,
}: {
  transport: Transport;
  router: AppRouterOptions;
}) {
  const [router] = useState(() => createAppRouter(routerOptions));
  return (
    <TransportProvider transport={transport}>
      <QueryProvider>
        <SettingsProvider>
          <TooltipProvider>
            <LocalHost router={router} />
            <Toaster />
          </TooltipProvider>
        </SettingsProvider>
      </QueryProvider>
    </TransportProvider>
  );
}
