#![cfg_attr(test, allow(clippy::too_many_lines))]
//! Kelvo's app shell: Tauri setup, the commands and events the webview reaches, settings
//! ownership, live channels, and the local source's lifecycle. Business logic lives in
//! the `kelvo-*` crates; see `state.rs` for the startup order and the API the tray and
//! window code builds on.

mod addresses;
mod bench;
mod calibration;
mod commands;
mod disk_space;
mod docker;
pub mod edition;
mod error;
mod export;
pub mod facts;
mod history;
mod hosts;
pub mod ipc;
pub mod live;
mod logging;
mod notify;
mod platform;
pub mod popover;
pub mod process_signal;
pub mod remote;
pub mod remote_network;
pub mod settings;
pub mod state;
pub mod tray;
mod updates;
mod usage;
mod windows;

use specta_typescript::Typescript;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_specta::{Builder, collect_commands, collect_events};

pub use error::CommandError;
pub use hosts::{HostEntry, HostRegistry};
pub use state::AppState;

/// Where the TypeScript bindings land. Generated, never hand-edited; see
/// `.claude/rules/generated-bindings.md`.
pub const BINDINGS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../src/core/generated/bindings.ts"
);

/// The bundle identifier of debug builds; release builds use `tauri.conf.json`'s.
#[cfg(debug_assertions)]
pub const DEV_IDENTIFIER: &str = "com.tryopendata.kelvo.dev";

/// Remote setup and background launches defer UI creation until a tray click.
fn deferred_ui_launch(args: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    args.into_iter()
        .any(|arg| matches!(arg.as_ref(), "--remote-setup" | "--background"))
}

/// Every command and event the webview can reach. The same builder feeds the invoke
/// handler and the bindings export, so the two cannot drift apart.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        // One TS type per Rust type. Phased export splits any type that reaches a
        // `deserialize_with` field (`Capabilities`, `Settings`) into `_Serialize` and
        // `_Deserialize` aliases, and in rc.25 it then exports the internally tagged
        // `LiveMsg` as externally tagged, which is not what serde sends. No Kelvo type has
        // a different shape per direction, so nothing is lost (D-048).
        .disable_serde_phases()
        .commands(collect_commands![
            docker::docker_discover, docker::docker_configure, docker::docker_snapshot,
            docker::docker_resume, docker::docker_subscribe, docker::docker_unsubscribe,
            docker::docker_detail, docker::docker_retained, docker::docker_history, docker::docker_comparison, docker::docker_events,
            docker::docker_logs, docker::docker_close_logs,
            docker::docker_storage_scan, docker::docker_storage_prepare,
            docker::docker_storage_execute, docker::docker_storage_operation,
            docker::docker_container_action,
            disk_space::disk_space_volumes, disk_space::disk_space_start,
            disk_space::disk_space_status, disk_space::disk_space_cancel,
            disk_space::disk_space_report, disk_space::disk_space_subtree,
            disk_space::disk_space_largest_files, disk_space::disk_space_reveal,
            disk_space::disk_space_path,
            commands::list_hosts,
            remote::list_remote_hosts,
            remote::remote_ssh_config_status,
            remote::inspect_remote_ssh_host,
            remote::accept_remote_ssh_host,
            remote::prepare_remote_host,
            remote::add_remote_host,
            remote::cancel_remote_setup,
            remote::update_remote_host,
            remote::remove_remote_host,
            remote::retry_remote_host,
            remote::reset_remote_host_identity, remote::update_remote_agent, remote::repair_remote_host,
            commands::get_host,
            commands::get_capabilities,
            commands::subscribe_live,
            commands::set_process_interest,
            commands::query_history,
            commands::query_disk_io,
            commands::list_disk_io_devices,
            commands::query_processes_at,
            commands::query_network_by_app,
            commands::query_usage_by_app, commands::query_usage_series,
            commands::query_series_stats,
            commands::get_network_addresses,
            commands::get_public_ip,
            commands::query_events,
            commands::query_heatmap,
            commands::battery_hours,
            commands::export_csv,
            commands::get_settings,
            commands::update_settings,
            commands::history_size,
            commands::history_growth,
            commands::clear_history,
            commands::history_health,
            commands::reset_history,
            commands::set_paused,
            commands::open_dashboard,
            commands::sensor_dump,
            commands::process_signal,
            commands::get_edition,
            updates::check_for_updates,
            updates::get_update_status,
            updates::install_update,
            commands::get_window_appearance,
            commands::report_popover_paint, popover::commands::report_popover_content_height,
        ])
        .events(collect_events![
            updates::UpdateStatusChanged,
            ipc::SettingsChanged,
            ipc::CapabilitiesChanged,
            ipc::WindowAppearanceChanged,
            ipc::NavigateRequested,
            ipc::HistoryHealthChanged,
            ipc::HostsChanged,
            ipc::EventRecorded,
        ])
        // Each catalog metric's kind, for code that stands in for the engine (the mock
        // transport) and so must label its layouts as the engine does (D-090).
        .constant("METRIC_KINDS", facts::metric_kinds())
        // Engine, store and settings facts the webview and the mock read instead of
        // keeping copies (D-092). See `facts.rs`.
        .constant("METRIC_UNITS", facts::metric_units())
        .constant("METRIC_MODULES", facts::metric_modules())
        .constant("METRIC_PERIODS_MS", facts::metric_periods_ms())
        .constant("METRIC_CODES", kelvo_schema::metric_codes())
        .constant("RING_SPAN_MS", kelvo_engine::RING_SPAN_MS)
        .constant("HISTORY_COMMIT_MS", facts::history_commit_ms())
        // How far back `query_history` answers with history unavailable (live-only).
        .constant("HISTORY_RECENT_MS", kelvo_engine::RECENT_ROWS_MS)
        .constant("RING_MAX_ROWS", kelvo_engine::RING_MAX_ROWS)
        .constant("HOLD_FACTOR", facts::hold_factor())
        .constant("PERFORMANCE_VISIBLE_MS", kelvo_engine::PERFORMANCE_VISIBLE_MS)
        .constant("NET_BUCKET_MS", kelvo_engine::NET_BUCKET_MS)
        .constant("USAGE_BUCKET_MS", kelvo_engine::USAGE_BUCKET_MS)
        // The longest range `query_usage_series` answers.
        .constant("USAGE_KEEP_MS", kelvo_engine::USAGE_KEEP_MS)
        .constant("HEADER_BYTES_PER_PACKET", kelvo_engine::HEADER_BYTES_PER_PACKET)
        .constant("CLAMP_SLACK_BYTES", kelvo_engine::CLAMP_SLACK_BYTES)
        .constant("MAX_NET_SPAN_MS", history::MAX_NET_SPAN_MS)
        .constant("SAMPLING_INTERVALS_MS", facts::SAMPLING_INTERVALS_MS)
        .constant("RETENTION_DAYS", facts::RETENTION_DAYS)
        .constant("SIZE_LIMITS_MB", facts::SIZE_LIMITS_MB)
        .constant("SETTINGS_MODULES", kelvo_schema::settings::SETTINGS_MODULES)
        .constant("ITEM_MODES", facts::item_modes())
        .constant("READOUTS", facts::READOUTS)
        .constant("HISTORY_TIERS", facts::history_tiers())
        .constant("HISTORY_PROJECTION", facts::history_projection())
        .constant("SAMPLING_PLANS", facts::sampling_plans())
        .typ::<facts::SamplingPlan>()
        .typ::<kelvo_schema::Readout>()
}

/// Writes the TypeScript bindings for `builder` to [`BINDINGS_PATH`].
pub fn export_bindings(builder: &Builder<tauri::Wry>) -> Result<(), specta_typescript::Error> {
    builder.export(Typescript::default(), BINDINGS_PATH)?;
    // Specta emits spaces after union separators. Normalize generation itself so
    // freshly added variants pass the repository whitespace gate reproducibly.
    let exported = std::fs::read_to_string(BINDINGS_PATH)?;
    let clean = exported
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(BINDINGS_PATH, clean)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[expect(
    clippy::too_many_lines,
    reason = "split when substantially changed; architecture refactor section 10"
)]
pub fn run() {
    let builder = specta_builder();

    // Debug builds refresh the bindings on every launch. `make bindings` does the same
    // without starting the app, and CI diffs its output against the commit.
    #[cfg(debug_assertions)]
    if let Err(err) = export_bindings(&builder) {
        eprintln!("failed to export TypeScript bindings to {BINDINGS_PATH}: {err}");
    }

    let app = tauri::Builder::default()
        // First, so a second launch hands over before it opens anything: the running
        // instance shows its dashboard and the new process exits (one writer per history
        // file, architecture.md infra 8; the store's lock is the backstop).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if deferred_ui_launch(&args) { return; }
            if let Err(e) = windows::open_dashboard(app, None) {
                tracing::warn!("showing the dashboard for a second launch: {e}");
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // The CSV export's save dialog, opened from Rust only: no window is granted the
        // plugin's permissions.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        // Alert notifications, posted from Rust only (notify.rs): no window is granted the
        // plugin's permissions.
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_denylist(&[popover::LABEL, windows::ONBOARDING])
                .build(),
        )
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            // Neither an unresolvable nor an unwritable log directory stops the launch:
            // logging falls back to stderr.
            let log_dir = app.path().app_log_dir().ok();
            // The guard flushes the log writer when the app exits.
            if let Some(guard) = logging::init(log_dir.as_deref()) {
                app.manage(guard);
            }
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                logs = ?log_dir,
                "Kelvo starting"
            );
            builder.mount_events(app);
            AppState::start(app.handle())?;
            updates::start(app.handle());
            app.manage(windows::DashboardLifecycle::default());
            tray::create(app.handle())?;
            #[cfg(target_os = "macos")]
            if !deferred_ui_launch(std::env::args()) {
                popover::create(app.handle())?;
            }
            remote::start(app.handle())?;
            match bench::scenario() {
                Some(scenario) => bench::start(app.handle(), scenario),
                None => {
                    if !deferred_ui_launch(std::env::args()) && !platform::launched_at_login() {
                        windows::show_onboarding_if_needed(app.handle())?;
                        if app.state::<AppState>().settings.settings().onboarding.completed {
                            windows::open_dashboard(app.handle(),None)?;
                        }
                    }
                },
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The popover hides when it loses key status: an outside click, or another
            // app activating (D-033).
            #[cfg(target_os = "macos")]
            WindowEvent::Focused(false) if window.label() == popover::LABEL => {
                popover::hide(window.app_handle());
            }
            WindowEvent::CloseRequested { api, .. } if window.label() == windows::DASHBOARD => {
                api.prevent_close();
                windows::hide_dashboard(window);
            }
            WindowEvent::Destroyed => {
                windows::destroyed(window.label());
                if let Some(state) = window.try_state::<AppState>() {
                    state.live.window_closed(window.label());
                    state.docker_streams.window_closed(&state, window.label());
                    if window.label() == windows::DASHBOARD {
                        disk_space::dashboard_closed(&state);
                    }
                }
            }
            _ => {}
        });
    // Replaces Tauri's default immediate reload: a showing popover reloads on its next
    // hide instead (D-035).
    #[cfg(target_os = "macos")]
    let app = app
        .plugin(tauri_nspanel::init())
        .on_web_content_process_terminate(popover::web_content_terminated);
    let mut context = tauri::generate_context!();
    // Bench bundles keep their explicit isolated identity even in debug builds.
    let identifier = runtime_identifier(&context.config().identifier).to_owned();
    context.config_mut().identifier = identifier;
    let app = app.build(context);
    let app = match app {
        Ok(app) => app,
        Err(e) => {
            tracing::error!("Kelvo failed to start: {e}");
            eprintln!("Kelvo failed to start: {e}");
            std::process::exit(1);
        }
    };
    app.run(|app, event| match event {
        // A menu bar app keeps running when its last window closes; only an explicit
        // quit (which carries an exit code) ends it.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            if let Err(e) = windows::open_dashboard(app, None) {
                tracing::warn!("reopening dashboard: {e}");
            }
        }
        RunEvent::Exit => {
            remote::shutdown(app);
            if let Some(state) = app.try_state::<AppState>() {
                state.shutdown();
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::deferred_ui_launch;

    #[test]
    fn launch_modes_defer_ui_only_for_remote_setup_and_background() {
        for args in [vec!["kelvo"], vec!["kelvo", "--unrelated"], vec![]] {
            assert!(!deferred_ui_launch(args));
        }
        for args in [
            vec!["kelvo", "--remote-setup"],
            vec!["kelvo", "--background"],
            vec!["kelvo", "--background", "--remote-setup"],
        ] {
            assert!(deferred_ui_launch(args));
        }
    }
}

/// Development builds are isolated from installed data; bench builds supply their own ID.
fn runtime_identifier(configured: &str) -> &str {
    if cfg!(all(debug_assertions, not(feature = "bench"))) {
        DEV_IDENTIFIER
    } else {
        configured
    }
}

#[cfg(test)]
mod identity_tests {
    #[test]
    fn bench_keeps_the_configured_identity_and_normal_debug_uses_dev_data() {
        let configured = "com.tryopendata.kelvo.bench.ux-native";
        let expected = if cfg!(all(debug_assertions, not(feature = "bench"))) {
            super::DEV_IDENTIFIER
        } else {
            configured
        };
        assert_eq!(super::runtime_identifier(configured), expected);
    }
}
