/**
 * Integration test helper: render a component inside the providers the app
 * uses, backed by the mock transport (frontend/testing.md). Tests drive live
 * data with `transport.push(msg)` / `transport.tick()`; nothing ticks on its
 * own.
 */

import type { SeriesSelector } from "@core/generated/bindings";
import { MOCK_HOST_ID } from "@core/mock/fixtures";
import {
  createMockTransport,
  type MockTransport,
  type MockTransportOptions,
} from "@core/mock-transport";
import { QueryClient } from "@tanstack/react-query";
import { type RenderOptions, render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { createMemoryRouter, RouterProvider } from "react-router";
import { QueryProvider } from "~/lib/query";
import { TransportProvider } from "~/lib/transport-context";
import { HostStoreProvider } from "~/stores/host-store";
import { SettingsProvider } from "~/stores/settings-store";

/** A QueryClient with retries off so failures surface on the first try. */
export function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
      mutations: { retry: false },
    },
  });
}

export interface RenderWithProvidersOptions
  extends Omit<RenderOptions, "wrapper"> {
  transport?: MockTransport;
  transportOptions?: MockTransportOptions;
  queryClient?: QueryClient;
  /** Router path the element is mounted at. Default "/". */
  route?: string;
  /** Live backfill requested on subscribe. Default 60 s. */
  backfillMs?: number;
  /** Series the live channel is projected to (D-066). Default all. */
  series?: readonly SeriesSelector[];
}

export function renderWithProviders(
  ui: ReactElement,
  {
    transportOptions,
    transport = createMockTransport({ ...transportOptions, autoTick: false }),
    queryClient = createTestQueryClient(),
    route = "/",
    backfillMs,
    series,
    ...renderOptions
  }: RenderWithProvidersOptions = {}
) {
  const router = createMemoryRouter([{ path: "*", element: ui }], {
    initialEntries: [route],
  });
  const result = render(
    <TransportProvider transport={transport}>
      <QueryProvider client={queryClient}>
        <SettingsProvider>
          <HostStoreProvider
            hostId={MOCK_HOST_ID}
            backfillMs={backfillMs}
            series={series}
          >
            <RouterProvider router={router} />
          </HostStoreProvider>
        </SettingsProvider>
      </QueryProvider>
    </TransportProvider>,
    renderOptions
  );
  return { ...result, transport, queryClient, router, user: userEvent.setup() };
}
