import { chartRange, relativeWindow } from "@core/chart-range";
import type { ChartWindow } from "@core/generated/bindings";
import { settingsErrorText } from "@core/settings-patch";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useRef,
  useState,
} from "react";
import { toast } from "sonner";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useSettingsStore, useUpdateSettings } from "~/stores/settings-store";

interface NavigationSession {
  back: ChartWindow[];
  relativeMs: number;
  initialized: boolean;
  busy: boolean;
}
const fresh = (): NavigationSession => ({
  back: [],
  relativeMs: 86_400_000,
  initialized: false,
  busy: false,
});
const SessionContext = createContext<NavigationSession | null>(null);

/** Session-only navigation survives tab and host changes, never another window. */
export function RangeNavigationProvider({ children }: { children: ReactNode }) {
  const session = useRef<NavigationSession>(fresh());
  return (
    <SessionContext.Provider value={session.current}>
      {children}
    </SessionContext.Provider>
  );
}

export function useRangeNavigation() {
  const shared = useContext(SessionContext);
  const local = useRef<NavigationSession>(fresh());
  const session = shared ?? local.current;
  const range = useChartWindow();
  const settings = useSettingsStore();
  const update = useUpdateSettings();
  const [pending, setPending] = useState(false);
  if (range && !session.initialized) {
    session.initialized = true;
    if (range.endMs === null) session.relativeMs = range.windowMs;
  }

  const apply = useCallback(
    async (next: ChartWindow, back = false): Promise<boolean> => {
      const previous =
        settings.getState().snapshot?.settings.general.chart_window;
      if (
        !previous ||
        session.busy ||
        JSON.stringify(previous) === JSON.stringify(next)
      )
        return false;
      session.busy = true;
      setPending(true);
      try {
        const result = await update({ general: { chart_window: next } });
        if (result.status === "error") {
          toast.error(settingsErrorText(result.error), {
            action: { label: "Retry", onClick: () => void apply(next, back) },
          });
          return false;
        }
        if (back) session.back.pop();
        else session.back = [...session.back, previous].slice(-20);
        const chosen = chartRange(next);
        if (chosen.endMs === null) session.relativeMs = chosen.windowMs;
        return true;
      } catch (error) {
        console.error("[timeline] range save failed", error);
        toast.error("The time range could not be saved.", {
          action: { label: "Retry", onClick: () => void apply(next, back) },
        });
        return false;
      } finally {
        session.busy = false;
        setPending(false);
      }
    },
    [settings, update, session]
  );
  return {
    range,
    pending,
    canBack: session.back.length > 0,
    apply,
    back: () => {
      const previous = session.back.at(-1);
      if (previous) void apply(previous, true);
    },
    live: () => void apply(relativeWindow(session.relativeMs)),
  };
}
