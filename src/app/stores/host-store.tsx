/**
 * The scoped live store, one per host (architecture.md "Live state: one store
 * per host"). Adapted from opendata's `/ask` conversation store: the same
 * `createStore` + context provider + `useState` initializer, so the store is
 * a per-provider instance that dies with its subtree, never a module-level
 * singleton, and consumers read it through a selector so a 1 Hz frame only
 * re-renders what changed.
 *
 * Providers nest into a registry keyed `hosts[hostId]` (v4 shows several
 * hosts in one window); `useHost` reads the nearest provider's host unless a
 * host id is passed.
 */
import type { HostId, LiveMsg, SeriesSelector } from "@core/generated/bindings";
import {
  type Connection,
  clearRows,
  type HostLive,
  initialHostLive,
  reduceLive,
} from "@core/live-state";
import { ProcessInterest } from "@core/process-interest";
import type { Transport } from "@core/transport";
import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createStore, type StoreApi, useStore } from "zustand";
import { useTransport } from "~/lib/transport-context";

export interface HostLiveState extends HostLive {
  apply: (msg: LiveMsg) => void;
  setStale: (stale: boolean) => void;
  setConnection: (connection: Connection) => void;
  /** Drop every row (a resubscribe that refills the ring from Rust). */
  clearRows: () => void;
  /**
   * This window's process interest for the host (D-066): consumers add
   * their views, the union goes to Rust with the live stream's id. The same
   * object for the store's life.
   */
  processInterest: ProcessInterest;
}

export type HostStore = StoreApi<HostLiveState>;

export function createHostStore(
  hostId: HostId,
  transport?: Transport
): HostStore {
  const interest = new ProcessInterest((view, stream) => {
    if (!transport) return;
    void transport
      .setProcessInterest(hostId, view !== null, view, stream)
      .then((r) => {
        if (r.status === "error") {
          console.error("[processes] set_process_interest failed", {
            hostId,
            window: transport.windowLabel(),
            view,
            error: r.error,
          });
        }
      });
  });
  return createStore<HostLiveState>()((set) => ({
    ...initialHostLive(hostId),
    apply: (msg) => set((s) => reduceLive(s, msg)),
    setStale: (stale) => set((s) => (s.stale === stale ? s : { stale })),
    setConnection: (connection) =>
      set((s) => (s.connection === connection ? s : { connection })),
    clearRows: () => set((s) => clearRows(s)),
    processInterest: interest,
  }));
}

const HostsContext = createContext<Readonly<Record<HostId, HostStore>>>({});
const CurrentHostContext = createContext<HostId | null>(null);

/** Retry delays after a failed subscribe, capped at the last one. */
const RETRY_MS = [500, 1000, 2000, 5000];
/** Intervals without a frame before the window reads stale (plan 4.17). */
const STALE_INTERVALS = 3;
/**
 * Intervals a visible, unpaused window stays stale before it resubscribes
 * (review #14): Rust may have ended the channel without saying so. Doubles
 * per attempt without a frame, capped at `MAX_RECONNECT_MS`.
 */
export const RECONNECT_INTERVALS = 3;
const MAX_RECONNECT_MS = 60_000;
/**
 * What Rust sends before `subscribe_live` returns (D-066). A missed span
 * longer than this would arrive partly as earlier chunks, which go in front
 * of the oldest row, so the resubscribe refills the ring instead.
 */
const RECENT_MS = 120_000;

/**
 * True while the window is hidden. Rust stops a hidden window's channel and
 * restarts it on show (D-049), so a hidden window neither retries a failed
 * subscribe nor counts missing frames as stale. There is no Rust visibility
 * event yet; `visibilityState` is the best signal WKWebView gives.
 */
function isHidden(): boolean {
  return (
    typeof document !== "undefined" && document.visibilityState === "hidden"
  );
}

interface LiveRequest {
  backfillMs: number;
  series: readonly SeriesSelector[] | undefined;
}

/**
 * Subscribe `store` to the host's live channel for as long as it is mounted.
 *
 * - A failed subscribe shows as `reconnecting` and retries; Rust resends a
 *   backfill on every subscribe, so nothing is lost.
 * - Stale is set when no frame arrives for three frame periods while
 *   sampling is not paused and the display is awake (plan 4.17).
 * - Stale for `RECONNECT_INTERVALS` more while visible: the channel may have
 *   ended silently (review #14), so it shows `reconnecting` and
 *   resubscribes, asking only for the span it missed.
 * - Every subscription's `stream` goes to the window's process interest, so
 *   interest follows the live subscription (D-066).
 */
function useLiveSubscription(
  store: HostStore,
  transport: Transport,
  hostId: HostId,
  request: LiveRequest
) {
  const { backfillMs } = request;
  // Selectors by value: a new array with the same series is no resubscribe.
  const seriesKey = JSON.stringify(request.series ?? null);
  const seriesRef = useRef(request.series);
  seriesRef.current = request.series;

  // biome-ignore lint/correctness/useExhaustiveDependencies: seriesKey stands for request.series, read through seriesRef.
  useEffect(() => {
    let cancelled = false;
    let unsubscribe = () => {};
    let staleTimer: ReturnType<typeof setTimeout> | undefined;
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    let onVisible: (() => void) | undefined;
    // Resubscribes since the last frame; each waits twice as long.
    let reconnects = 0;
    const interest = store.getState().processInterest;
    const clearOnVisible = () => {
      if (onVisible)
        document.removeEventListener("visibilitychange", onVisible);
      onVisible = undefined;
    };

    const armStale = () => {
      clearTimeout(staleTimer);
      clearTimeout(reconnectTimer);
      const status = store.getState().status;
      // Paused or display asleep: Rust sends no frames until it changes, and
      // says so with a status, which re-arms this.
      if (!status || status.paused || status.display_idle) return;
      // Frames come every frame period: the interval, or the multiple of it
      // a minimum period thins them to.
      const period = Math.max(status.frame_period_ms, status.interval_ms);
      staleTimer = setTimeout(() => {
        if (isHidden()) return;
        store.getState().setStale(true);
        const wait = Math.min(
          MAX_RECONNECT_MS,
          RECONNECT_INTERVALS * period * 2 ** reconnects
        );
        reconnectTimer = setTimeout(() => {
          const s = store.getState();
          if (
            cancelled ||
            isHidden() ||
            !s.stale ||
            s.status?.paused ||
            s.status?.display_idle
          )
            return;
          reconnect();
        }, wait);
      }, STALE_INTERVALS * period);
    };

    const onMsg = (msg: LiveMsg) => {
      store.getState().apply(msg);
      if (msg.kind === "frame") reconnects = 0;
      if (msg.kind === "frame" || msg.kind === "status") armStale();
    };

    const retry = (attempt: number, reason: unknown, backfill: number) => {
      console.error("[live] subscribe failed", {
        hostId,
        window: transport.windowLabel(),
        reason,
      });
      store.getState().setConnection("reconnecting");
      const delay = RETRY_MS[Math.min(attempt, RETRY_MS.length - 1)];
      retryTimer = setTimeout(() => {
        if (!isHidden()) {
          void connect(attempt + 1, backfill);
          return;
        }
        // Hidden: wait for the window to show instead of polling Rust.
        clearOnVisible();
        onVisible = () => {
          if (isHidden() || cancelled) return;
          clearOnVisible();
          void connect(attempt + 1, backfill);
        };
        document.addEventListener("visibilitychange", onVisible);
      }, delay);
    };

    const connect = async (attempt: number, backfill: number) => {
      try {
        const sub = await transport.subscribeLive(hostId, onMsg, {
          backfillMs: backfill,
          series: seriesRef.current,
        });
        if (cancelled) {
          sub.unsubscribe();
          return;
        }
        if (sub.info.status === "error") {
          sub.unsubscribe();
          retry(attempt, sub.info.error, backfill);
          return;
        }
        unsubscribe = sub.unsubscribe;
        interest.setStream(sub.info.data.stream);
        armStale();
      } catch (err) {
        if (!cancelled) retry(attempt, err, backfill);
      }
    };

    const reconnect = () => {
      reconnects++;
      const s = store.getState();
      console.warn("[live] no frames while visible; resubscribing", {
        hostId,
        window: transport.windowLabel(),
        lastTsMs: s.lastTsMs,
        attempt: reconnects,
      });
      unsubscribe();
      unsubscribe = () => {};
      interest.setStream(null);
      s.setConnection("reconnecting");
      // Ask Rust for the span missed. Rows already held are dropped as not
      // newer; a gap longer than the recent span refills the ring instead.
      const interval = s.status?.interval_ms ?? 1000;
      let backfill = backfillMs;
      if (s.lastTsMs !== null) {
        const missed = Date.now() - s.lastTsMs + 2 * interval;
        if (missed < RECENT_MS) backfill = Math.min(backfillMs, missed);
        else s.clearRows();
      }
      void connect(0, backfill);
    };

    // Shown again: frames should resume within three intervals (Rust
    // restarts the channel), so count from now.
    const rearmOnShow = () => {
      if (!isHidden()) armStale();
    };
    document.addEventListener("visibilitychange", rearmOnShow);

    void connect(0, backfillMs);
    return () => {
      cancelled = true;
      unsubscribe();
      clearTimeout(staleTimer);
      clearTimeout(reconnectTimer);
      clearTimeout(retryTimer);
      clearOnVisible();
      document.removeEventListener("visibilitychange", rearmOnShow);
    };
  }, [store, transport, hostId, backfillMs, seriesKey]);
}

/**
 * One live store for `hostId`, subscribed while mounted. Key the provider by
 * host id at the call site if the host can change.
 */
export function HostStoreProvider({
  hostId,
  backfillMs = 60_000,
  series,
  children,
}: {
  hostId: HostId;
  /** Ring history to request on subscribe (D-049). Default 60 s. */
  backfillMs?: number;
  /** The series this window draws (D-066); omitted means every series. */
  series?: readonly SeriesSelector[];
  children: ReactNode;
}) {
  const transport = useTransport();
  const [store] = useState(() => createHostStore(hostId, transport));
  useLiveSubscription(store, transport, hostId, { backfillMs, series });
  const parent = useContext(HostsContext);
  const hosts = useMemo(
    () => ({ ...parent, [hostId]: store }),
    [parent, hostId, store]
  );
  return (
    <HostsContext.Provider value={hosts}>
      <CurrentHostContext.Provider value={hostId}>
        {children}
      </CurrentHostContext.Provider>
    </HostsContext.Provider>
  );
}

/** The store instance for `hostId`, or the nearest provider's host. */
export function useHostStore(hostId?: HostId): HostStore {
  const hosts = useContext(HostsContext);
  const current = useContext(CurrentHostContext);
  const id = hostId ?? current;
  const store = id === null ? undefined : hosts[id];
  if (!store) {
    throw new Error(
      `useHostStore: no <HostStoreProvider> for host ${id ?? "(none)"}`
    );
  }
  return store;
}

/** The current host's id. */
export function useHostId(): HostId {
  const current = useContext(CurrentHostContext);
  if (current === null) {
    throw new Error("useHostId must be used within a <HostStoreProvider>");
  }
  return current;
}

/**
 * Read a slice of a host's live state. Return primitives, or wrap the
 * selector in `useShallow` when it builds an object; a fresh object from a
 * plain selector re-renders the caller on every frame.
 */
export function useHost<T>(
  selector: (state: HostLiveState) => T,
  hostId?: HostId
): T {
  return useStore(useHostStore(hostId), selector);
}
