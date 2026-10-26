//! Native restart acceptance. Run through scripts/verify-update-restart.py, which
//! copies this binary into an isolated app bundle and removes only its own data.
use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use kelvo_lib::{AppState, startup};
use kelvo_schema::{Event, EventDetail};
use serde::{Deserialize, Serialize};
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

#[derive(Clone, Serialize, Deserialize)]
struct Expected {
    pid: u32,
    host: String,
    epoch: String,
    event_ms: i64,
    arguments: Vec<String>,
    initial_ui_shown: bool,
}

#[derive(Clone)]
struct Acceptance {
    root: PathBuf,
    identifier: String,
    resumed: bool,
}

fn main() -> anyhow::Result<()> {
    let root =
        PathBuf::from(std::env::var("KELVO_ACCEPTANCE_ROOT").context("use the acceptance runner")?);
    let identifier =
        std::env::var("KELVO_ACCEPTANCE_IDENTIFIER").context("missing acceptance identifier")?;
    ensure!(
        identifier.starts_with("com.tryopendata.kelvo.acceptance."),
        "refusing a production identifier"
    );
    ensure!(root.is_dir(), "missing acceptance output directory");
    let acceptance = Acceptance {
        resumed: root.join("expected.json").exists(),
        root,
        identifier,
    };
    record(
        &acceptance.root.join(if acceptance.resumed {
            "second-pid.json"
        } else {
            "first-pid.json"
        }),
        &std::process::id(),
    )?;
    // This supplies inert assets/config only. Builder<Wry> below runs the actual
    // macOS event loop and WKWebView; no MockRuntime is instantiated.
    let mut context = tauri::test::mock_context::<tauri::Wry, _>(tauri::test::noop_assets());
    *context.config_mut() = serde_json::from_str(include_str!("../tauri.conf.json"))?;
    context.config_mut().identifier = acceptance.identifier.clone();
    context.config_mut().product_name = Some("Kelvo update acceptance".into());
    context.config_mut().app.windows.clear();
    context.config_mut().app.app_directories_override =
        Some(tauri::utils::config::AppDirectoriesOverride::Root(
            acceptance.root.join(&acceptance.identifier),
        ));
    let setup = acceptance.clone();
    let app = tauri::Builder::<tauri::Wry>::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(move |app| {
            kelvo_lib::specta_builder().mount_events(app);
            if let Err(error) = prepare(app.handle(), &setup) {
                record(
                    &setup.root.join("failure.json"),
                    &serde_json::json!({"error": format!("{error:#}")}),
                )?;
                return Err(error.into());
            }
            Ok(())
        })
        .build(context)?;
    app.run(move |app, event| match event {
        RunEvent::Ready if acceptance.resumed => app.exit(0),
        RunEvent::Ready => app.request_restart(),
        RunEvent::ExitRequested { code, .. } => {
            let filename = if acceptance.resumed {
                "second-request.json"
            } else {
                "first-request.json"
            };
            if let Err(error) = record(
                &acceptance.root.join(filename),
                &serde_json::json!({"code": code, "pid": std::process::id()}),
            ) {
                eprintln!("recording exit request: {error:#}");
            }
        }
        RunEvent::Exit => {
            // These are the same shutdown owners used by the shipped app's Exit arm.
            kelvo_lib::remote::shutdown(app);
            app.state::<AppState>().shutdown();
            let filename = if acceptance.resumed {
                "done.json"
            } else {
                "first-exit.json"
            };
            if let Err(error) = record(
                &acceptance.root.join(filename),
                &serde_json::json!({"pid": std::process::id(), "shutdown_completed": true}),
            ) {
                eprintln!("recording shutdown: {error:#}");
            }
        }
        _ => {}
    });
    Ok(())
}

fn prepare(app: &tauri::AppHandle, acceptance: &Acceptance) -> anyhow::Result<()> {
    let data = app.path().app_data_dir()?;
    let cache = app.path().app_cache_dir()?;
    ensure!(
        data.file_name().and_then(|p| p.to_str()) == Some(&acceptance.identifier),
        "data path escaped acceptance identity"
    );
    ensure!(
        cache == data.join("caches"),
        "cache path escaped acceptance data directory"
    );
    record(
        &acceptance.root.join("paths.json"),
        &serde_json::json!({"data": data, "cache": cache}),
    )?;
    AppState::start(app)?;
    let state = app.state::<AppState>();
    // First-run defaults deliberately remain incomplete, preventing login-item
    // reconciliation. No test path registers or unregisters any login item.
    ensure!(
        !state.settings.settings().onboarding.completed,
        "acceptance must not reconcile login registration"
    );
    let restarted = startup::take_background_restart(app);
    let deferred = startup::deferred_ui_launch(std::env::args());
    let show = startup::show_initial_windows(restarted, deferred, false);
    if show {
        let window = WebviewWindowBuilder::new(
            app,
            "onboarding",
            WebviewUrl::External("about:blank".parse()?),
        )
        .title("Kelvo update acceptance")
        .inner_size(320.0, 160.0)
        .focused(false)
        .build()?;
        ensure!(
            window.is_visible()?,
            "ordinary launch did not create a visible native window"
        );
        // The unattended installation occurs later, after interactive UI closes.
        window.hide()?;
    }
    if acceptance.resumed {
        verify_resumed(app, acceptance, restarted, show)?;
    } else {
        ensure!(
            !restarted,
            "a fresh acceptance identity had a restart marker"
        );
        ensure!(show != deferred, "initial launch policy mismatch");
        let event_ms = kelvo_engine::wall_ms();
        let expected = Expected {
            pid: std::process::id(),
            host: state.local.to_string(),
            epoch: state
                .history
                .read(|reader| Ok(reader.epoch().to_string()))?,
            event_ms,
            arguments: std::env::args().skip(1).collect(),
            initial_ui_shown: show,
        };
        state.history.writer()?.record_event(
            state.local,
            &Event {
                ts_ms: event_ms,
                start_ms: event_ms,
                processes: vec!["native-update-acceptance".into()],
                detail: EventDetail::FansRamped {
                    from_rpm: 1.0,
                    to_rpm: 2.0,
                },
            },
        )?;
        // No explicit history flush here: Exit must perform it before relaunch.
        record(&acceptance.root.join("expected.json"), &expected)?;
        startup::mark_background_restart(app, env!("CARGO_PKG_VERSION"))?;
    }
    Ok(())
}

fn verify_resumed(
    app: &tauri::AppHandle,
    acceptance: &Acceptance,
    restarted: bool,
    show: bool,
) -> anyhow::Result<()> {
    let expected: Expected =
        serde_json::from_slice(&fs::read(acceptance.root.join("expected.json"))?)?;
    ensure!(
        acceptance.root.join("first-exit.json").is_file(),
        "restart skipped native Exit/shutdown"
    );
    ensure!(
        std::process::id() != expected.pid,
        "no new process was launched"
    );
    ensure!(
        restarted && !show,
        "update restart did not suppress interactive startup"
    );
    ensure!(
        app.webview_windows().is_empty(),
        "restart reopened a webview window"
    );
    ensure!(
        !startup::take_background_restart(app),
        "restart marker was not consumed once"
    );
    let state = app.state::<AppState>();
    ensure!(
        state.local.to_string() == expected.host,
        "host identity changed on restart"
    );
    let (epoch, events) = state.history.read(|reader| {
        Ok((
            reader.epoch().to_string(),
            reader.events(state.local, expected.event_ms, expected.event_ms + 1)?,
        ))
    })?;
    ensure!(epoch == expected.epoch, "history epoch changed on restart");
    ensure!(
        events
            .iter()
            .any(|event| event.processes == ["native-update-acceptance"]),
        "Exit failed to preserve queued history"
    );
    ensure!(
        std::env::args().skip(1).collect::<Vec<_>>() == expected.arguments,
        "restart dropped startup arguments"
    );
    record(
        &acceptance.root.join("verified.json"),
        &serde_json::json!({
            "first_pid": expected.pid, "second_pid": std::process::id(), "host": expected.host,
            "epoch": epoch, "arguments": expected.arguments, "visible_windows": 0,
            "initial_ui_shown": expected.initial_ui_shown,
            "history_preserved": true, "background_restart": true,
        }),
    )
}

fn record(path: &Path, value: &impl Serialize) -> anyhow::Result<()> {
    use std::io::Write;
    let temporary = path.with_extension("tmp");
    let mut file = fs::File::create(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}
