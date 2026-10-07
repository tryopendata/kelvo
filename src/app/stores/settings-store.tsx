/**
 * Read-only mirror of the Rust-owned settings (architecture.md item 8,
 * D-050). Same scoped `createStore` + provider pattern as the host store.
 * The mirror is replaced whole on `settings-changed` and on the result of
 * our own `update_settings`; a snapshot not newer than the one held is
 * dropped, so a late event cannot roll settings back. Accepting a newer one
 * invalidates the TanStack Query keys whose data depends on the sections
 * that changed (`settingsInvalidation`).
 */
import type {
  Settings,
  SettingsPatch,
  SettingsSnapshot,
} from "@core/generated/bindings";
import { settingsInvalidation } from "@core/query-keys";
import type { CommandResult } from "@core/transport";
import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import { createStore, type StoreApi, useStore } from "zustand";
import { useTransport } from "~/lib/transport-context";

/**
 * Whether the mirror holds settings: `loading` until the first snapshot,
 * `failed` when `get_settings` failed and no snapshot has arrived since.
 */
export type SettingsStatus = "loading" | "ready" | "failed";

export interface SettingsState {
  snapshot: SettingsSnapshot | null;
  status: SettingsStatus;
  /** Take `next` if it is newer than what is held. Returns whether it did. */
  replace: (next: SettingsSnapshot) => boolean;
  /** `get_settings` failed: the mirror is failed unless it already holds a snapshot. */
  fail: () => void;
}

export type SettingsStore = StoreApi<SettingsState>;

export function createSettingsStore(
  initial: SettingsSnapshot | null = null
): SettingsStore {
  return createStore<SettingsState>()((set, get) => ({
    snapshot: initial,
    status: initial ? "ready" : "loading",
    replace: (next) => {
      const cur = get().snapshot;
      if (cur && next.revision <= cur.revision) return false;
      set({ snapshot: next, status: "ready" });
      return true;
    },
    fail: () => {
      if (get().snapshot === null) set({ status: "failed" });
    },
  }));
}

/**
 * Take `next` into the mirror and, if it was newer, invalidate what the
 * sections that changed made stale.
 */
function replaceAndInvalidate(
  store: SettingsStore,
  queryClient: QueryClient,
  next: SettingsSnapshot
) {
  const prev = store.getState().snapshot?.settings ?? null;
  if (!store.getState().replace(next)) return;
  const stale = settingsInvalidation(prev, next.settings);
  void queryClient.invalidateQueries({
    predicate: (q) => stale(q.queryKey),
  });
}

const SettingsContext = createContext<SettingsStore | null>(null);

export function SettingsProvider({
  initial = null,
  children,
}: {
  initial?: SettingsSnapshot | null;
  children: ReactNode;
}) {
  const transport = useTransport();
  const queryClient = useQueryClient();
  const [store] = useState(() => createSettingsStore(initial));

  useEffect(() => {
    let cancelled = false;
    const unsubscribe = transport.onSettingsChanged((event) => {
      replaceAndInvalidate(store, queryClient, event);
    });
    transport.getSettings().then(
      (snapshot) => {
        if (!cancelled) store.getState().replace(snapshot);
      },
      (err: unknown) => {
        console.error("[settings] get_settings failed", {
          window: transport.windowLabel(),
          err,
        });
        if (!cancelled) store.getState().fail();
      }
    );
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [store, transport, queryClient]);

  return (
    <SettingsContext.Provider value={store}>
      {children}
    </SettingsContext.Provider>
  );
}

export function useSettingsStore(): SettingsStore {
  const store = useContext(SettingsContext);
  if (!store) {
    throw new Error("useSettingsStore must be used within <SettingsProvider>");
  }
  return store;
}

/**
 * Read a slice of the settings. `null` until the first `get_settings`
 * answers.
 */
export function useSettings<T>(selector: (settings: Settings) => T): T | null {
  return useStore(useSettingsStore(), (s) =>
    s.snapshot ? selector(s.snapshot.settings) : null
  );
}

/** Whether the mirror has loaded, is still loading, or failed to load. */
export function useSettingsStatus(): SettingsStatus {
  return useStore(useSettingsStore(), (s) => s.status);
}

/**
 * Write through `update_settings`. The returned snapshot replaces the mirror
 * (and invalidates dependent queries) without waiting for the event.
 */
export function useUpdateSettings(): (
  patch: SettingsPatch
) => Promise<CommandResult<SettingsSnapshot>> {
  const transport = useTransport();
  const store = useSettingsStore();
  const queryClient = useQueryClient();
  return useCallback(
    async (patch) => {
      const result = await transport.updateSettings(patch);
      if (result.status === "ok") {
        replaceAndInvalidate(store, queryClient, result.data);
      }
      return result;
    },
    [transport, store, queryClient]
  );
}
