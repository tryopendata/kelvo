/**
 * The entry route for a window (architecture.md "Routing by window label").
 * In the app the label comes from the Tauri window; in the browser dev server
 * from `?window=`. `?route=` overrides the entry where allowed (dev builds),
 * so the gallery and any page can be opened directly.
 *
 * A dashboard window Rust creates for `open_dashboard(route)` loads that
 * route as its URL path (`/dashboard/settings`), so the dashboard starts
 * there instead of Overview.
 */
export function initialRoute(
  label: string,
  search = "",
  allowOverride = false,
  pathname = "/"
): string {
  const override = new URLSearchParams(search).get("route");
  if (allowOverride && override?.startsWith("/")) return override;
  if (label === "popover") return "/popover";
  if (label === "onboarding") return "/onboarding";
  if (pathname.startsWith("/dashboard/")) return pathname;
  return "/dashboard/overview";
}
