/**
 * TanStack Query client for history and other command-backed data. Adapted
 * from opendata's `app/lib/query.tsx`: same centralised `shouldRetry` /
 * `getRetryDelay` and a stable client held in `useState`. What changed: no
 * SSR or hydration (each Tauri window is its own client), and the errors are
 * `CommandFailure`s from the transport instead of HTTP errors. Live data is
 * never put in this cache.
 */
import { CommandFailure } from "@core/transport";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { type ReactNode, useState } from "react";

/** Command errors that a retry cannot fix. */
const PERMANENT_ERRORS = new Set([
  "unknown_host",
  "history_unavailable",
  "store_corrupt",
  "store_too_new",
  "invalid_argument",
  "invalid_settings",
]);

/**
 * Retry decisions live here, not in the transport. Commands are local IPC,
 * so a failure is rarely transient: retry a store error once (a busy
 * writer), never a permanent one.
 */
export function shouldRetry(failureCount: number, error: Error): boolean {
  if (error instanceof CommandFailure) {
    if (PERMANENT_ERRORS.has(error.error.kind)) return false;
    return failureCount < 1;
  }
  return failureCount < 1;
}

/** Short backoff: 250 ms, then 500 ms. IPC has no thundering herd. */
export function getRetryDelay(attemptIndex: number): number {
  return 250 * 2 ** attemptIndex;
}

export function makeQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // History below the last closed bucket does not change; the Timeline
        // appends new buckets itself while following Live.
        staleTime: 60 * 1000,
        // A window regaining focus is not a reason to re-read history.
        refetchOnWindowFocus: false,
        retry: shouldRetry,
        retryDelay: getRetryDelay,
      },
    },
  });
}

export function QueryProvider({
  client,
  children,
}: {
  client?: QueryClient;
  children: ReactNode;
}) {
  // One client per window, stable across re-renders.
  const [queryClient] = useState(() => client ?? makeQueryClient());
  return (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}
