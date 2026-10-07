import type { Appearance, WindowAppearance } from "@core/generated/bindings";
import { useEffect, useLayoutEffect, useState } from "react";
import { useTransport } from "~/lib/transport-context";
import { useSettings } from "~/stores/settings-store";

const DARK_QUERY = "(prefers-color-scheme: dark)";

/** Whether `.dark` belongs on <html> for an appearance and the OS scheme. */
export function resolveDark(appearance: Appearance, systemDark: boolean) {
  if (appearance === "system") return systemDark;
  return appearance === "dark";
}

function useSystemDark(): boolean {
  const [dark, setDark] = useState(() => window.matchMedia(DARK_QUERY).matches);
  useEffect(() => {
    const media = window.matchMedia(DARK_QUERY);
    const onChange = (e: MediaQueryListEvent) => setDark(e.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);
  return dark;
}

/**
 * Reflect the window's appearance on <html> (design-system.md "Themes",
 * "Vibrant surfaces", "Motion"): `.dark` from the Appearance setting (Match
 * system follows `prefers-color-scheme`), and `data-performance` /
 * `data-reduce-transparency` from `get_window_appearance` and its event.
 * `override` pins the theme (the gallery's `?theme=`).
 */
export function useWindowAppearance(override?: "light" | "dark" | null) {
  const transport = useTransport();
  const appearance = useSettings((s) => s.general.appearance) ?? "system";
  const systemDark = useSystemDark();
  const [flags, setFlags] = useState<WindowAppearance | null>(null);

  useEffect(() => {
    let cancelled = false;
    transport.getWindowAppearance().then(
      (a) => {
        if (!cancelled) setFlags(a);
      },
      (err: unknown) =>
        console.error("[appearance] get_window_appearance failed", err)
    );
    const unsubscribe = transport.onWindowAppearanceChanged((e) =>
      setFlags(e.appearance)
    );
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, [transport]);

  const dark = override
    ? override === "dark"
    : resolveDark(appearance, systemDark);

  useLayoutEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", dark);
    root.toggleAttribute(
      "data-performance",
      (flags?.performance ?? "off") !== "off"
    );
    root.toggleAttribute(
      "data-reduce-transparency",
      flags?.reduce_transparency ?? false
    );
  }, [dark, flags]);
}
