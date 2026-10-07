//! The dashboard and onboarding windows (architecture.md, Window and channel lifecycle).
//!
//! - `dashboard`: overlay title bar with the traffic lights inset over the sidebar,
//!   minimum 1024 × 700, size and position remembered by tauri-plugin-window-state.
//!   Closing hides it; after 5 minutes hidden it is destroyed. `open_dashboard(route)`
//!   creates it at `route` (the URL path) or shows it and sends `NavigateRequested`.
//! - `onboarding`: fixed 820 × 566, shown at launch while `onboarding.completed` is false.
//!
//! Both report visibility to the live registry on show and hide, and through an occlusion
//! observer for minimize and being covered (D-036, D-049).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{
    AppHandle, LogicalPosition, Manager, TitleBarStyle, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window,
};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};
use tauri_specta::Event;

use crate::error::CommandError;
use crate::ipc::NavigateRequested;
use crate::state::AppState;

pub const DASHBOARD: &str = "dashboard";
pub const ONBOARDING: &str = "onboarding";
const DEFAULT_ROUTE: &str = "/dashboard/overview";
/// A hidden dashboard keeps its webview this long, so reopening is instant.
const DESTROY_AFTER: Duration = Duration::from_secs(5 * 60);
/// Traffic lights inset over the 220 px sidebar.
const TRAFFIC_LIGHTS: LogicalPosition<f64> = LogicalPosition::new(18.0, 22.0);

/// Bumped on every dashboard show and hide; a teardown timer only fires if nothing
/// happened since it started.
#[derive(Default)]
pub struct DashboardLifecycle {
    generation: AtomicU64,
}

fn window_err(e: tauri::Error) -> CommandError {
    CommandError::Window {
        message: e.to_string(),
    }
}

fn set_visible(app: &AppHandle, label: &str, visible: bool) {
    if let Some(state) = app.try_state::<AppState>() {
        state.live.window_visible(label, visible);
    }
}

fn bump(app: &AppHandle) -> u64 {
    app.try_state::<DashboardLifecycle>()
        .map_or(0, |l| l.generation.fetch_add(1, Ordering::AcqRel) + 1)
}

#[cfg(target_os = "macos")]
fn observe_occlusion(app: &AppHandle, window: &WebviewWindow) {
    use crate::platform::appkit;
    let Some(ns) = appkit::ns_window(window) else {
        return;
    };
    let label = window.label().to_owned();
    let handle = app.clone();
    appkit::observe_occlusion(&label.clone(), &ns, move |visible| {
        tracing::debug!(label, visible, "window occlusion");
        set_visible(&handle, &label, visible);
    });
}

#[cfg(not(target_os = "macos"))]
fn observe_occlusion(_app: &AppHandle, _window: &WebviewWindow) {}

/// Drops per-window observers when a window is destroyed.
pub fn destroyed(label: &str) {
    #[cfg(target_os = "macos")]
    crate::platform::appkit::remove_occlusion_observer(label);
    #[cfg(not(target_os = "macos"))]
    let _ = label;
}

/// `/dashboard/...` only: the dashboard window has no other routes.
fn check_route(route: &str) -> Result<(), CommandError> {
    if route == "/dashboard" || route.starts_with("/dashboard/") {
        Ok(())
    } else {
        Err(CommandError::InvalidArgument {
            message: format!("{route} is not a dashboard route"),
        })
    }
}

pub fn open_dashboard(app: &AppHandle, route: Option<&str>) -> Result<(), CommandError> {
    if let Some(r) = route {
        check_route(r)?;
    }
    bump(app);
    if let Some(w) = app.get_webview_window(DASHBOARD) {
        w.unminimize().map_err(window_err)?;
        w.show().map_err(window_err)?;
        w.set_focus().map_err(window_err)?;
        set_visible(app, DASHBOARD, true);
        if let Some(route) = route {
            let event = NavigateRequested {
                route: route.to_owned(),
            };
            event.emit_to(app, DASHBOARD).map_err(window_err)?;
        }
        return Ok(());
    }
    let path = route.unwrap_or(DEFAULT_ROUTE).trim_start_matches('/');
    let w = WebviewWindowBuilder::new(app, DASHBOARD, WebviewUrl::App(path.into()))
        .title("Kelvo")
        .inner_size(1200.0, 800.0)
        .min_inner_size(1024.0, 700.0)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(TRAFFIC_LIGHTS)
        .visible(false)
        .build()
        .map_err(window_err)?;
    // The window-state plugin restored the remembered frame during build.
    observe_occlusion(app, &w);
    w.show().map_err(window_err)?;
    w.set_focus().map_err(window_err)?;
    set_visible(app, DASHBOARD, true);
    tracing::info!(route = path, "dashboard created");
    Ok(())
}

/// The dashboard's close button: hide, remember the frame, and destroy it after
/// [`DESTROY_AFTER`] unless it is shown again first.
pub fn hide_dashboard(window: &Window) {
    let app = window.app_handle().clone();
    if let Err(e) = app.save_window_state(StateFlags::all()) {
        tracing::warn!("saving the dashboard frame: {e}");
    }
    if let Err(e) = window.hide() {
        tracing::warn!("hiding the dashboard: {e}");
    }
    set_visible(&app, DASHBOARD, false);
    let generation = bump(&app);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(DESTROY_AFTER).await;
        let current = app
            .try_state::<DashboardLifecycle>()
            .map(|l| l.generation.load(Ordering::Acquire));
        if current != Some(generation) {
            return;
        }
        if let Some(w) = app.get_webview_window(DASHBOARD)
            && !w.is_visible().unwrap_or(true)
        {
            tracing::info!("destroying the dashboard after 5 minutes hidden");
            if let Err(e) = w.destroy() {
                tracing::warn!("destroying the dashboard: {e}");
            }
        }
    });
}

/// Shows the onboarding window on a first run. Runs in `setup`.
pub fn show_onboarding_if_needed(app: &AppHandle) -> anyhow::Result<()> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    if state.settings.settings().onboarding.completed {
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(app, ONBOARDING, WebviewUrl::App("onboarding".into()))
        .title("Set up Kelvo")
        .inner_size(820.0, 566.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .center()
        .build()?;
    observe_occlusion(app, &w);
    set_visible(app, ONBOARDING, true);
    w.set_focus()?;
    tracing::info!("onboarding shown");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_dashboard_routes() {
        assert!(check_route("/dashboard/settings").is_ok());
        assert!(check_route("/dashboard").is_ok());
        assert!(check_route("/popover").is_err());
        assert!(check_route("/dashboardx").is_err());
        assert!(check_route("dashboard/cpu").is_err());
    }
}
