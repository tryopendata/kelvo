/**
 * A chart's time selection (D-089), pinned in absolute time: it stays put
 * while the chart scrolls and after it scrolls off. Same scoped `createStore`
 * + provider pattern as the host store, so only the chart overlay and the
 * views that read the selection subscribe; the rest of the page does not
 * re-render when it changes.
 *
 * `range` is the committed selection (what tables query); `draft` is the
 * range under a drag in progress, drawn on the chart but not queried.
 */
import { sameRange, type TimeRange } from "@core/brush";
import { createContext, type ReactNode, useContext, useState } from "react";
import { createStore, type StoreApi, useStore } from "zustand";

export interface BrushState {
  range: TimeRange | null;
  draft: TimeRange | null;
  select: (range: TimeRange) => void;
  setDraft: (draft: TimeRange | null) => void;
  clear: () => void;
}

export type BrushStore = StoreApi<BrushState>;

export function createBrushStore(initial: TimeRange | null = null): BrushStore {
  return createStore<BrushState>()((set, get) => ({
    range: initial,
    draft: null,
    select: (range) => {
      if (sameRange(get().range, range) && get().draft === null) return;
      set({ range, draft: null });
    },
    setDraft: (draft) => {
      if (sameRange(get().draft, draft)) return;
      set({ draft });
    },
    clear: () => {
      if (get().range === null && get().draft === null) return;
      set({ range: null, draft: null });
    },
  }));
}

const BrushContext = createContext<BrushStore | null>(null);

export function BrushProvider({
  initial = null,
  children,
}: {
  initial?: TimeRange | null;
  children: ReactNode;
}) {
  const [store] = useState(() => createBrushStore(initial));
  return (
    <BrushContext.Provider value={store}>{children}</BrushContext.Provider>
  );
}

/** The provider's store, or null outside one (a chart without a brush). */
export function useBrushStoreOptional(): BrushStore | null {
  return useContext(BrushContext);
}

export function useBrushStore(): BrushStore {
  const store = useContext(BrushContext);
  if (!store)
    throw new Error("useBrushStore must be used within <BrushProvider>");
  return store;
}

// Stands in outside a provider so the hook below is called unconditionally.
const NO_BRUSH = createBrushStore();

/** A slice of the brush state; reads an empty selection outside a provider. */
export function useBrush<T>(selector: (s: BrushState) => T): T {
  return useStore(useContext(BrushContext) ?? NO_BRUSH, selector);
}

/** The committed selection (what tables query). */
export const useBrushRange = () => useBrush((s) => s.range);
