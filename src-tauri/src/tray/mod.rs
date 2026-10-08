//! The menu bar items (architecture.md, Tray rendering pipeline; v1-local-monitor.md 4.2).
//!
//! A `kelvo-tray` thread follows the local host's bus. On each frame it records the
//! graph history and builds the quantized [`TrayItem`](model::TrayItem)s for the current
//! menu bar settings: the combined item, and one item per module in an own-item mode
//! (D-080). Status items are created and removed so exactly those exist; each has a
//! stable `autosaveName` (its tray id, D-037), so macOS keeps its ⌘-drag position. A
//! settings change applies at once, even while paused.
//!
//! The items are paced together (D-077, D-080): a frame equal to the item's last drawn one
//! is skipped, and the changed items draw at most once per redraw period (2 s, 4 s while
//! backed off), all at the same moment. A changed frame inside the period is held, and a
//! timer draws the held frames if no newer ones arrive by then. Frames whose images keep
//! their size are handed to AppKit directly as template bitmaps, with the accessibility
//! label only if its words changed, one main-thread call per item, into status items
//! whose length is pinned (D-073). A size change
//! goes through tray-icon, which sets image and template flag in one call (D-030) and
//! resizes its click target, and then the new length is pinned. Pausing draws at once.
//! While the display sleeps or the screen is locked it draws nothing.
//!
//! Left click on any item toggles the popover under that item; right click or
//! Control-click opens the menu.

pub mod model;
pub mod render;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use kelvo_engine::{BusMsg, EngineStatus};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager, Wry};
use tauri_specta::Event;

use self::model::{
    ItemKey, ItemState, Pacer, Readings, Retries, TrayContent, TrayHistory, TrayItem, TraySeries,
};
use crate::hosts::HostEntry;
use crate::ipc::SettingsChanged;
use crate::live::LiveFeed;
use crate::state::AppState;
use crate::windows;

/// The combined item's tray id.
pub const TRAY_ID: &str = "kelvo";
/// How often the debug log reports the pacing counters.
const COUNTER_LOG_EVERY: Duration = Duration::from_secs(60);

/// Menu items whose state follows the engine.
pub struct TrayMenu {
    pause: CheckMenuItem<Wry>,
}

/// Device pixels per point for the tray image: 2 if any display is Retina. A 2x image on
/// a 1x display is downsampled by AppKit; a 1x image on Retina would be blurry.
fn image_scale(app: &AppHandle) -> u32 {
    let max = app
        .available_monitors()
        .map(|ms| ms.iter().map(|m| m.scale_factor()).fold(1.0, f64::max))
        .unwrap_or(2.0);
    if max >= 1.5 { 2 } else { 1 }
}

/// Creates the status items and starts the tray thread. Runs in `setup`, after
/// [`AppState::start`].
pub fn create(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let entry = state.host(state.local).context("the local host")?;
    let status = entry.status();
    let scale = image_scale(app);

    let pause = CheckMenuItem::with_id(
        app,
        "pause",
        "Pause Sampling",
        true,
        status.paused,
        None::<&str>,
    )?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", "Open Dashboard", true, None::<&str>)?,
            &MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?,
            &pause,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit Kelvo", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    // Once for the app, not per status item: Tauri keeps every tray's menu handler as a
    // global listener, so per-item handlers would toggle Pause once per item.
    app.on_menu_event(on_menu_event);
    app.manage(TrayMenu { pause });

    let mut items = Items {
        app: app.clone(),
        menu,
        shown: BTreeMap::new(),
        pacer: Pacer::default(),
        retries: Retries::default(),
    };
    let first = model::build(
        &Readings::default(),
        &TrayHistory::default(),
        &state.settings.settings(),
        status.paused,
        scale,
    );
    items.show(first, Instant::now(), Pacer::for_status(&status), true);
    if items.shown.is_empty() {
        anyhow::bail!("no status item could be created");
    }

    // Settings apply at once, not with the next frame: a mode change can add or remove an
    // item, and while paused no frame comes.
    let changed = Arc::new(tokio::sync::Notify::new());
    let notify = Arc::clone(&changed);
    app.listen_any(SettingsChanged::NAME, move |_| notify.notify_one());

    let handle = app.clone();
    std::thread::Builder::new()
        .name("kelvo-tray".into())
        .spawn(move || run(&handle, &entry, items, &changed, scale, status))
        .context("starting the tray thread")?;
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    tracing::debug!(item = event.id().as_ref(), "tray menu");
    match event.id().as_ref() {
        "open" => {
            if let Err(e) = windows::open_dashboard(app, None) {
                tracing::warn!("opening the dashboard from the menu: {e}");
            }
        }
        "settings" => {
            if let Err(e) = windows::open_dashboard(app, Some("/dashboard/settings")) {
                tracing::warn!("opening settings from the menu: {e}");
            }
        }
        "pause" => {
            // The check item has already toggled itself.
            let paused = app
                .try_state::<TrayMenu>()
                .and_then(|m| m.pause.is_checked().ok())
                .unwrap_or(false);
            if let Some(state) = app.try_state::<AppState>()
                && let Err(e) = state.set_paused(state.local, paused)
            {
                tracing::warn!("pausing from the tray: {e}");
            }
        }
        // An exit code, unlike closing the last window, ends the app (lib.rs `run`), and
        // `RunEvent::Exit` stops the engine and closes the store.
        "quit" => app.exit(0),
        _ => {}
    }
}

fn on_tray_event(tray: &TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Down,
        ..
    } = event
    {
        let clicked = Instant::now();
        left_click(tray, clicked);
    }
}

#[cfg(target_os = "macos")]
fn left_click(tray: &TrayIcon, clicked: Instant) {
    if crate::platform::appkit::control_key_down() {
        // Control-click is a right click on macOS; tray-icon only knows the button.
        if let Err(e) = tray.with_inner_tray_icon(|t| t.show_menu()) {
            tracing::warn!("showing the tray menu: {e}");
        }
        return;
    }
    crate::popover::toggle(tray.app_handle(), clicked, tray.id().as_ref());
}

#[cfg(not(target_os = "macos"))]
fn left_click(_tray: &TrayIcon, _clicked: Instant) {}

#[cfg(target_os = "macos")]
fn set_label(tray: &TrayIcon, label: String) {
    let res = tray.with_inner_tray_icon(move |t| {
        if let Some(item) = t.ns_status_item() {
            crate::platform::appkit::set_status_item_label(&item, &label);
        }
    });
    if let Err(e) = res {
        tracing::debug!("setting the tray accessibility label: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
fn set_label(tray: &TrayIcon, label: String) {
    let _ = tray.set_tooltip(Some(label));
}

/// Gives the status item its `autosaveName`, so macOS restores its ⌘-drag position.
#[cfg(target_os = "macos")]
fn set_autosave_name(tray: &TrayIcon, name: &'static str) {
    let res = tray.with_inner_tray_icon(move |t| {
        if let Some(item) = t.ns_status_item() {
            crate::platform::appkit::set_status_item_autosave_name(&item, name);
        }
    });
    if let Err(e) = res {
        tracing::warn!(name, "setting the status item's autosave name: {e}");
    }
}

#[cfg(not(target_os = "macos"))]
fn set_autosave_name(_tray: &TrayIcon, _name: &'static str) {}

/// Swaps the status item's image for `r` at the same size, and sets `label` if given,
/// in one main-thread call. False if nothing was set.
#[cfg(target_os = "macos")]
fn swap_image(tray: &TrayIcon, r: render::Rendered, label: Option<String>) -> bool {
    let res = tray.with_inner_tray_icon(move |t| {
        let Some(item) = t.ns_status_item() else {
            return false;
        };
        // tray-icon's sizing: 18 pt tall, width scaled to match.
        let h = f64::from(render::HEIGHT_PT);
        let w = f64::from(r.width) * h / f64::from(r.height.max(1));
        let set = crate::platform::appkit::set_status_item_image(
            &item,
            &r.rgba,
            r.width,
            r.height,
            (w, h),
        );
        if set && let Some(label) = label {
            crate::platform::appkit::set_status_item_label(&item, &label);
        }
        set
    });
    match res {
        Ok(set) => set,
        Err(e) => {
            tracing::warn!("setting the tray image: {e}");
            false
        }
    }
}

/// Pins the status item's length to its current width, or releases it before a resize.
#[cfg(target_os = "macos")]
fn pin_length(tray: &TrayIcon, pin: bool) {
    let res = tray.with_inner_tray_icon(move |t| {
        t.ns_status_item()
            .and_then(|item| crate::platform::appkit::pin_status_item_length(&item, pin))
    });
    match res {
        Ok(width) => tracing::debug!(pin, ?width, "tray length"),
        Err(e) => tracing::debug!("pinning the tray length: {e}"),
    }
}

#[cfg(not(target_os = "macos"))]
fn pin_length(_tray: &TrayIcon, _pin: bool) {}

#[cfg(not(target_os = "macos"))]
fn swap_image(_tray: &TrayIcon, _r: render::Rendered, _label: Option<String>) -> bool {
    false
}

fn render_item(content: &TrayContent) -> Option<render::Rendered> {
    render::render(&content.frame)
        .inspect_err(|e| tracing::warn!("rendering the tray icon: {e}"))
        .ok()
}

/// Draws `content` into one status item. Most frames keep the image's size and only
/// swap the image into a status item whose length is pinned, in one main-thread call of
/// its own: several items due together still get one call each, so the main thread is
/// never held for all their `setImage:` calls at once. A size change, or a failed swap,
/// takes [`draw_resized`]. Returns false when nothing was drawn.
fn draw(tray: &TrayIcon, item: &mut ItemState, content: TrayContent) -> bool {
    let Some(r) = render_item(&content) else {
        return false;
    };
    let changes = item.changes(r.width, r.height, &content.accessibility);
    if !changes.resize && swap_image(tray, r, changes.label) {
        tracing::trace!(label = %content.accessibility, "tray image swapped");
        return true;
    }
    draw_resized(tray, item, content)
}

/// Draws `content` through tray-icon, which resizes the status item and its click
/// target, then pins the new length. For a size change, or a failed swap. Returns false
/// when nothing was drawn (render or `set_icon` failed).
fn draw_resized(tray: &TrayIcon, item: &mut ItemState, content: TrayContent) -> bool {
    let Some(r) = render_item(&content) else {
        return false;
    };
    item.forget();
    pin_length(tray, false);
    let (w, h) = (r.width, r.height);
    if let Err(e) = tray.set_icon_with_as_template(Some(Image::new_owned(r.rgba, w, h)), true) {
        tracing::warn!("setting the tray icon: {e}");
        return false;
    }
    pin_length(tray, true);
    item.changes(w, h, &content.accessibility);
    tracing::trace!(label = %content.accessibility, "tray drawn");
    set_label(tray, content.accessibility);
    true
}

/// One status item on screen.
struct Shown {
    tray: TrayIcon,
    state: ItemState,
}

/// The status items on screen, by key, paced together.
struct Items {
    app: AppHandle,
    menu: Menu<Wry>,
    shown: BTreeMap<ItemKey, Shown>,
    pacer: Pacer,
    /// Items that could not be created, and when each is tried again.
    retries: Retries,
}

impl Items {
    /// Creates the status item for `key`, empty: its first frame draws on arrival.
    fn create(&self, key: ItemKey) -> anyhow::Result<TrayIcon> {
        let tray = TrayIconBuilder::with_id(key.id())
            .icon_as_template(true)
            .menu(&self.menu)
            .show_menu_on_left_click(false)
            .on_tray_icon_event(on_tray_event)
            .build(&self.app)
            .with_context(|| format!("creating the {} status item", key.id()))?;
        set_autosave_name(&tray, key.id());
        tracing::debug!(id = key.id(), "status item created");
        Ok(tray)
    }

    /// Removes the status item on the main thread, where AppKit wants it removed and
    /// where tray-icon's handle may be dropped.
    fn remove(&self, key: ItemKey, shown: Shown) {
        let app = self.app.clone();
        let res = self.app.run_on_main_thread(move || {
            drop(app.remove_tray_by_id(key.id()));
            drop(shown);
        });
        match res {
            Ok(()) => tracing::debug!(id = key.id(), "status item removed"),
            Err(e) => tracing::warn!(id = key.id(), "removing a status item: {e}"),
        }
    }

    /// Tries the items that could not be created again with the next build (the
    /// settings changed, or the display woke).
    fn retry_now(&mut self) {
        self.retries.retry_now(Instant::now());
    }

    /// Makes the status items on screen exactly `wanted`, and offers their content.
    fn show(&mut self, wanted: Vec<TrayItem>, now: Instant, period: Duration, urgent: bool) {
        let keys: BTreeSet<ItemKey> = self.shown.keys().copied().collect();
        let waiting = self.retries.waiting(&wanted, now);
        let change = model::item_change(&keys, &wanted, &waiting);
        for key in change.remove {
            if let Some(shown) = self.shown.remove(&key) {
                self.pacer.remove(key);
                self.remove(key, shown);
            }
        }
        for key in change.create {
            match self.create(key) {
                Ok(tray) => {
                    self.retries.created(key);
                    let shown = Shown {
                        tray,
                        state: ItemState::default(),
                    };
                    self.shown.insert(key, shown);
                }
                // Logged once per streak; tried again after `RETRY_AFTER`.
                Err(e) => {
                    if self.retries.failed(key, now) {
                        tracing::warn!("{e:#}; trying again every 30 s");
                    } else {
                        tracing::debug!("{e:#}");
                    }
                }
            }
        }
        let wanted = wanted
            .into_iter()
            .filter(|w| self.shown.contains_key(&w.key))
            .collect();
        let due = self.pacer.offer(wanted, now, period, urgent);
        self.draw(due);
    }

    /// Draws the items the [`Pacer`] found due, each with its own main-thread call. An
    /// item that was not drawn has the [`Pacer`] forget its frame, so the next one
    /// redraws instead of being skipped as already shown.
    fn draw(&mut self, due: Vec<TrayItem>) {
        for item in due {
            let Some(shown) = self.shown.get_mut(&item.key) else {
                continue;
            };
            if !draw(&shown.tray, &mut shown.state, item.content) {
                self.pacer.forget_last(item.key);
            }
        }
    }

    /// When the held frames draw, or, while the display is awake, an item that could
    /// not be created is tried again.
    fn deadline(&self, period: Duration, display_idle: bool) -> Option<Instant> {
        let retry = if display_idle {
            None
        } else {
            self.retries.deadline()
        };
        [self.pacer.deadline(period), retry]
            .into_iter()
            .flatten()
            .min()
    }

    /// Whether an item that could not be created is due to be tried again.
    fn retry_due(&self, now: Instant) -> bool {
        self.retries.deadline().is_some_and(|at| at <= now)
    }

    /// Draws the held frames once their deadline has passed.
    fn due(&mut self, now: Instant, period: Duration) {
        let due = self.pacer.due(now, period);
        self.draw(due);
    }

    fn drop_pending(&mut self) {
        self.pacer.drop_pending();
    }

    /// Drawn, skipped and held frames over every item.
    fn counters(&self) -> (u64, u64, u64) {
        (self.pacer.drawn, self.pacer.skipped, self.pacer.held)
    }
}

/// Sleeps until `deadline`, or forever when there is none.
async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(d) => tokio::time::sleep_until(d.into()).await,
        None => std::future::pending().await,
    }
}

fn run(
    app: &AppHandle,
    entry: &HostEntry,
    items: Items,
    changed: &tokio::sync::Notify,
    scale: u32,
    status: EngineStatus,
) {
    // A runtime of its own on this thread, for the held frames' timer next to the bus.
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("tray thread: no runtime, the menu bar will not update: {e}");
            return;
        }
    };
    rt.block_on(follow(app, entry, items, changed, scale, status));
}

async fn follow(
    app: &AppHandle,
    entry: &HostEntry,
    mut items: Items,
    changed: &tokio::sync::Notify,
    scale: u32,
    mut status: EngineStatus,
) {
    let mut sub = entry.bus().subscribe();
    let mut series = TraySeries::default();
    // The Disk readout's volume; host facts do not change while the app runs.
    let boot_mount = entry.record().info.boot_mounts.first().cloned();
    let mut readings = Readings::default();
    let mut history = TrayHistory::default();
    let mut idle_frames = 0u64;
    let mut last_log = Instant::now();
    let counter_every = crate::bench::tray_counters_every().unwrap_or(COUNTER_LOG_EVERY);
    let settings = || {
        app.try_state::<AppState>()
            .map(|s| s.settings.settings())
            .unwrap_or_default()
    };

    loop {
        let period = Pacer::for_status(&status);
        let msg = tokio::select! {
            msg = sub.recv() => msg,
            () = changed.notified() => {
                items.retry_now();
                if !status.display_idle {
                    let wanted = model::build(&readings, &history, &settings(), status.paused, scale);
                    items.show(wanted, Instant::now(), period, false);
                }
                continue;
            }
            () = until(items.deadline(period, status.display_idle)) => {
                let now = Instant::now();
                items.due(now, period);
                if items.retry_due(now) && !status.display_idle {
                    let wanted = model::build(&readings, &history, &settings(), status.paused, scale);
                    items.show(wanted, now, period, false);
                }
                continue;
            }
        };
        let Some(msg) = msg else { break };
        match msg {
            BusMsg::Frame(f) => {
                if status.display_idle {
                    idle_frames += 1;
                } else {
                    if series.layout_no != Some(f.layout.layout_no) {
                        series = TraySeries::resolve(&f.layout, boot_mount.as_deref());
                    }
                    readings = series.readings(&f.held);
                    history.record(&readings);
                    let wanted =
                        model::build(&readings, &history, &settings(), status.paused, scale);
                    items.show(wanted, Instant::now(), period, false);
                }
            }
            BusMsg::Status(s) => {
                let before = std::mem::replace(&mut status, s);
                let s = &status;
                if before.interval_ms != s.interval_ms {
                    tracing::debug!(
                        interval_ms = s.interval_ms,
                        on_battery = s.on_battery,
                        "tray cadence"
                    );
                }
                if before.display_idle != s.display_idle {
                    tracing::debug!(
                        display_idle = s.display_idle,
                        "tray redraw {}",
                        if s.display_idle { "stopped" } else { "resumed" }
                    );
                    if !s.display_idle {
                        items.retry_now();
                    }
                    if s.display_idle {
                        items.drop_pending();
                        // The next sample is not next to the last one in time.
                        history.clear();
                    }
                }
                if before.paused != s.paused {
                    if let Some(m) = app.try_state::<TrayMenu>()
                        && let Err(e) = m.pause.set_checked(s.paused)
                    {
                        tracing::debug!("updating the pause item: {e}");
                    }
                    if s.paused {
                        history.clear();
                    }
                    // No frames arrive while paused: draw the paused state now.
                    if !s.display_idle {
                        let wanted =
                            model::build(&readings, &history, &settings(), s.paused, scale);
                        let period = Pacer::for_status(s);
                        items.show(wanted, Instant::now(), period, true);
                    }
                }
            }
            _ => {}
        }
        if last_log.elapsed() >= counter_every {
            last_log = Instant::now();
            let (drawn, skipped, held) = items.counters();
            tracing::debug!(
                items = items.shown.len(),
                drawn,
                skipped,
                held,
                idle = idle_frames,
                "tray frames"
            );
        }
    }
    tracing::info!("tray thread stopped: the bus closed");
}
