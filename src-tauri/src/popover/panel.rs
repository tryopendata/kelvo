//! The AppKit side of the popover: a tauri-nspanel `NSPanel` (D-033) with the Popover
//! material behind a transparent webview (D-034).

use std::time::Instant;

use anyhow::Context;
use tauri::{AppHandle, Manager, Webview, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_nspanel::{CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt};
use window_vibrancy::{NSVisualEffectMaterial, NSVisualEffectState};

use super::{LABEL, Popover, ROUTE, Rect, place};
use crate::platform::{self, appkit};
use crate::state::AppState;

mod panels {
    // `tauri_panel!` injects `use` items, so it gets a module of its own (D-033).
    use tauri_nspanel::tauri_panel;
    tauri_panel! {
        panel!(PopoverPanel {
            config: {
                can_become_key_window: true,
                can_become_main_window: false,
                is_floating_panel: true
            }
        })
    }
}

/// Corner radius of the material, matching the page's 12 px panel radius.
const RADIUS: f64 = 12.0;

fn apply_material(win: &WebviewWindow, reduce_transparency: bool) {
    // Reduce Transparency removes the material; the page switches to its opaque tokens
    // from `data-reduce-transparency` (design-system.md, Vibrant surfaces).
    let res = if reduce_transparency {
        window_vibrancy::clear_vibrancy(win).map(|_| ())
    } else {
        window_vibrancy::apply_vibrancy(
            win,
            NSVisualEffectMaterial::Popover,
            Some(NSVisualEffectState::Active),
            Some(RADIUS),
        )
    };
    if let Err(e) = res {
        tracing::warn!(reduce_transparency, "popover material: {e}");
    }
}

fn set_visible(app: &AppHandle, visible: bool) {
    if let Some(state) = app.try_state::<AppState>() {
        state.live.window_visible(LABEL, visible);
    }
}

/// Creates the panel hidden, so the first open only has to show it. Runs in `setup`.
pub fn create(app: &AppHandle) -> anyhow::Result<()> {
    app.manage(Popover::default());
    // Hidden from the start: no frames until the first show (D-049).
    set_visible(app, false);
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(ROUTE.into()))
        .title("Kelvo")
        .inner_size(super::WIDTH_PT, super::HEIGHT_PT)
        .decorations(false)
        .resizable(false)
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .transparent(true)
        .build()
        .context("creating the popover window")?;
    apply_material(&win, platform::reduce_transparency());

    let panel = win
        .to_panel::<panels::PopoverPanel>()
        .context("converting the popover to a panel")?;
    panel.set_level(PanelLevel::Status.value());
    if let Err(e) = panel.set_style_mask(StyleMask::empty().nonactivating_panel().into()) {
        tracing::warn!("popover style mask: {e}");
    }
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .stationary()
            .value(),
    );
    panel.set_has_shadow(true);

    let ns = panel.as_panel();
    let handle = app.clone();
    appkit::observe_occlusion(LABEL, ns, move |visible| {
        tracing::debug!(visible, "popover occlusion");
        set_visible(&handle, visible);
    });
    let handle = app.clone();
    appkit::on_escape_in(ns, move || hide(&handle));

    let handle = app.clone();
    platform::observe_reduce_transparency(move |reduce| {
        if let Some(w) = handle.get_webview_window(LABEL) {
            apply_material(&w, reduce);
        }
    });
    tracing::info!("popover panel created");
    Ok(())
}

/// The frame of the status item `tray_id` and its screen's visible frame.
fn anchor(app: &AppHandle, tray_id: &str) -> Option<(Rect, Rect)> {
    app.tray_by_id(tray_id)?
        .with_inner_tray_icon(|t| {
            t.ns_status_item()
                .and_then(|item| appkit::status_item_frames(&item))
        })
        .ok()
        .flatten()
}

/// Left click on the status item `tray_id` (there is one per own-item module, D-080), at
/// `clicked`. Shows the panel under it, or hides it if it is showing.
pub fn toggle(app: &AppHandle, clicked: Instant, tray_id: &str) {
    let Ok(panel) = app.get_webview_panel(LABEL) else {
        tracing::warn!("tray click before the popover exists");
        return;
    };
    let Some(state) = app.try_state::<Popover>() else {
        return;
    };
    if panel.is_visible() {
        hide(app);
        return;
    }
    if state.recently_hidden(clicked) {
        // This click is what took the focus and hid the panel.
        return;
    }
    if let Some((item, screen)) = anchor(app, tray_id) {
        appkit::set_frame(panel.as_panel(), place(item, screen));
    } else {
        tracing::warn!("status item frame unavailable; showing the popover where it was");
    }
    panel.show_and_make_key();
    set_visible(app, true);

    let token = state.start_open(clicked);
    if let Some(w) = app.get_webview_window(LABEL) {
        // The second animation frame runs after the first frame painted with the panel on
        // screen. The page needs no code of its own for this.
        let js = format!(
            "requestAnimationFrame(()=>requestAnimationFrame(()=>window.__TAURI_INTERNALS__\
             ?.invoke('report_popover_paint',{{token:{token}}})))"
        );
        if let Err(e) = w.eval(&js) {
            tracing::debug!("popover paint probe: {e}");
        }
    }
}

/// Hides the panel (outside click, Esc, second tray click) and stops its live channel.
pub fn hide(app: &AppHandle) {
    if let Ok(panel) = app.get_webview_panel(LABEL)
        && panel.is_visible()
    {
        panel.hide();
    }
    set_visible(app, false);
    let reload = app
        .try_state::<Popover>()
        .is_some_and(|p| p.hidden(Instant::now()));
    if reload && let Some(w) = app.get_webview_window(LABEL) {
        tracing::info!("reloading the popover after its WebContent process ended");
        if let Err(e) = w.reload() {
            tracing::warn!("reloading the popover: {e}");
        }
    }
}

/// A webview's WebContent process died (D-035). A showing popover reloads on its next
/// hide, so the user is not looking at a reload; everything else reloads now.
pub fn web_content_terminated(webview: &Webview) {
    let label = webview.label();
    let app = webview.app_handle();
    let showing = label == LABEL && app.get_webview_panel(LABEL).is_ok_and(|p| p.is_visible());
    tracing::warn!(label, showing, "WebContent process terminated");
    if showing {
        if let Some(p) = app.try_state::<Popover>() {
            p.set_reload_on_hide();
        }
    } else if let Err(e) = webview.reload() {
        tracing::warn!(label, "reloading after WebContent termination: {e}");
    }
}
