// First, so Tailwind's `@layer theme, base, components, utilities` is the
// first layer statement the page sees. A CSS module imported earlier that
// opens `@layer components` would otherwise rank components below base, and
// preflight's `border: 0 solid` would erase every card border.
import "~/app.css";
import { createAppTransport } from "@core/app-transport";
import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "~/app";
import { initialRoute } from "~/routes/initial-route";

const search = window.location.search;
// Explicit browser previews keep routing/theme controls with production React.
const preview =
  import.meta.env.DEV || import.meta.env.VITE_TRANSPORT === "mock";
const themeParam = new URLSearchParams(search).get("theme");
const theme =
  preview && (themeParam === "light" || themeParam === "dark")
    ? themeParam
    : null;

createAppTransport(search).then(
  (transport) => {
    // Window-scoped styles key off these: in the app the popover page stays
    // clear so the native material behind the webview shows (base.css). The
    // browser dev server has no material, so it keeps the opaque page.
    const root = document.documentElement;
    root.dataset.window = transport.windowLabel();
    root.toggleAttribute("data-native", transport.kind === "tauri");
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
      <React.StrictMode>
        <App
          transport={transport}
          router={{
            initialEntry: initialRoute(
              transport.windowLabel(),
              search,
              preview,
              window.location.pathname
            ),
            theme,
            dev: import.meta.env.DEV,
          }}
        />
      </React.StrictMode>
    );
  },
  (err: unknown) => console.error("[main] transport failed to start", err)
);
