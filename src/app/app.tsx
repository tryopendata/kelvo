import {
  type RemoteHostRecord,
  RING_SPAN_MS,
  type SeriesSelector,
} from "@core/generated/bindings";
import { hostClock } from "@core/host-clock";
import { scaledWindowMs, slowestIntervalMs } from "@core/live-window";
import { historyKeys, hostKeys } from "@core/query-keys";
import type { Transport } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { RouterProvider } from "react-router";
import { Toaster } from "~/components/ui/sonner";
import { TooltipProvider } from "~/components/ui/tooltip";
import { useRemoteHosts } from "~/hooks/use-remote-hosts";
import { HostClockContext } from "~/lib/host-clock";
import { HostSelectionContext } from "~/lib/host-selection";
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

function selectClockOffsets(hosts: RemoteHostRecord[]) {
  return hosts.map((host) => ({
    id: host.config.id,
    offset: host.status.clock_offset_ms ?? 0,
  }));
}

/** Waits for the host list, then scopes a live store to the local host. */
export function LocalHost({
  router,
}: {
  router: ReturnType<typeof createAppRouter>;
}) {
  const transport = useTransport();
  const queryClient = useQueryClient();
  const [selected, setSelected] = useState<string | null>(null);
  const remotes = useRemoteHosts(selectClockOffsets);
  const routerView = useMemo(
    () => <RouterProvider router={router} />,
    [router]
  );
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
  const active =
    label === "dashboard"
      ? (hosts?.find((host) => host.id === selected) ?? local)
      : local;
  const offset =
    remotes.data?.find((remote) => remote.id === active?.id)?.offset ?? 0;
  const selection = useMemo(
    () =>
      active && hosts
        ? { hosts, selected: active.id, select: setSelected }
        : null,
    [hosts, active]
  );
  const now = useMemo(
    () => (active?.is_local ? () => Date.now() : hostClock(offset)),
    [active?.is_local, offset]
  );
  const previousClock = useRef<{ host: string; offset: number } | null>(null);
  useEffect(() => {
    const previous = previousClock.current;
    previousClock.current = active ? { host: active.id, offset } : null;
    if (
      active &&
      !active.is_local &&
      previous?.host === active.id &&
      Math.abs(previous.offset - offset) >= 1000
    ) {
      void queryClient.invalidateQueries({
        queryKey: historyKeys.host(active.id),
      });
    }
  }, [active, offset, queryClient]);
  if (!local || !active) return null;
  // The popover waits for settings so its first subscribe asks for enough.
  if (label === "popover" && popoverBackfill === null) return null;
  return (
    <HostSelectionContext.Provider value={selection}>
      <HostClockContext.Provider value={now}>
        <HostStoreProvider
          key={active.id}
          hostId={active.id}
          backfillMs={popoverBackfill ?? BACKFILL_MS_BY_WINDOW[label]}
          series={SERIES_BY_WINDOW[label]}
        >
          {routerView}
        </HostStoreProvider>
      </HostClockContext.Provider>
    </HostSelectionContext.Provider>
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
