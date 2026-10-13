//! Typed Docker shell. No arbitrary endpoint or command crosses IPC.
use crate::{AppState, error::CommandError, settings::SettingsPatch};
use kelvo_schema::{docker::*, lock::LockExt};
use std::{collections::HashMap, sync::Mutex};
use tauri::{AppHandle, Manager, State, ipc::Channel};

struct DockerStream {
    window: String,
    wants_visible: bool,
    active: tokio::sync::watch::Sender<bool>,
    task: tokio::task::AbortHandle,
}
#[derive(Default)]
pub struct DockerStreams {
    next: Mutex<u32>,
    streams: Mutex<HashMap<u32, DockerStream>>,
    visibility: Mutex<HashMap<String, bool>>,
    logs: Mutex<HashMap<u32, String>>,
}
impl DockerStreams {
    fn next(&self) -> u32 {
        let mut id = self.next.lock_ok();
        *id = id.wrapping_add(1);
        *id
    }
    pub fn track_log(&self, id: u32, label: &str) {
        self.logs.lock_ok().insert(id, label.to_owned());
    }
    pub fn remove_log(&self, id: u32) {
        self.logs.lock_ok().remove(&id);
    }
    fn dispatch_window_log_cancellations(&self, label: &str, mut cancel: impl FnMut(u32)) {
        let ids: Vec<_> = {
            let mut logs = self.logs.lock_ok();
            let ids = logs
                .iter()
                .filter(|(_, window)| window.as_str() == label)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            for id in &ids {
                logs.remove(id);
            }
            ids
        };
        for id in ids {
            cancel(id);
        }
    }
    fn close_window_logs(&self, state: &AppState, label: &str) {
        self.dispatch_window_log_cancellations(label, |id| {
            state.docker.cancel_logs(
                id,
                "Logs paused because this window was hidden; reopen logs to continue",
            )
        });
    }
    pub fn window_closed(&self, state: &AppState, label: &str) {
        self.visibility.lock_ok().insert(label.to_owned(), false);
        self.close_window_logs(state, label);
        let ids: Vec<_> = self
            .streams
            .lock_ok()
            .iter()
            .filter(|(_, stream)| stream.window == label)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.close(state, id);
        }
    }
    pub fn window_visible(&self, state: &AppState, label: &str, visible: bool) {
        self.visibility.lock_ok().insert(label.to_owned(), visible);
        if !visible {
            self.close_window_logs(state, label);
        }
        for stream in self
            .streams
            .lock_ok()
            .values_mut()
            .filter(|stream| stream.window == label)
        {
            let active = visible && stream.wants_visible;
            if *stream.active.borrow() != active {
                state.docker.set_visible(active);
                stream.active.send_replace(active);
            }
        }
    }
    fn close(&self, state: &AppState, id: u32) {
        if let Some(stream) = self.streams.lock_ok().remove(&id) {
            stream.task.abort();
            if *stream.active.borrow() {
                state.docker.set_visible(false);
            }
        }
    }
}
fn error(message: impl ToString) -> CommandError {
    CommandError::Docker {
        message: message.to_string(),
    }
}
#[tauri::command]
#[specta::specta]
pub async fn docker_discover(
    state: State<'_, AppState>,
) -> Result<Vec<DockerEngine>, CommandError> {
    Ok(state.docker.discover().await)
}
#[tauri::command]
#[specta::specta]
pub fn docker_snapshot(state: State<'_, AppState>) -> Result<DockerSnapshot, CommandError> {
    Ok(state.docker.snapshot())
}
#[tauri::command]
#[specta::specta]
pub async fn docker_configure(
    app: AppHandle,
    state: State<'_, AppState>,
    mut settings: DockerSettings,
) -> Result<DockerSnapshot, CommandError> {
    if settings.enabled && settings.selected_engine.is_none() {
        let engines = state.docker.discover().await;
        settings.selected_engine = engines
            .iter()
            .find(|e| e.reachable && e.preferred)
            .map(|e| e.socket.clone())
            .or_else(|| {
                let mut found = engines.iter().filter(|e| e.reachable);
                let sole = found.next()?;
                found.next().is_none().then(|| sole.socket.clone())
            });
    }
    state.update_settings(
        &app,
        &SettingsPatch {
            docker: Some(settings),
            ..Default::default()
        },
    )?;
    Ok(state.docker.snapshot())
}
#[tauri::command]
#[specta::specta]
pub fn docker_resume(state: State<'_, AppState>) -> Result<DockerSnapshot, CommandError> {
    state.docker.resume();
    Ok(state.docker.snapshot())
}
#[tauri::command]
#[specta::specta]
pub fn docker_subscribe(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    channel: Channel<DockerSnapshot>,
    visible: bool,
) -> Result<u32, CommandError> {
    let mut streams = state.docker_streams.streams.lock_ok();
    if streams.len() >= 32 {
        return Err(error("Too many Docker subscriptions"));
    }
    let id = state.docker_streams.next();
    let shown = state
        .docker_streams
        .visibility
        .lock_ok()
        .get(window.label())
        .copied()
        .unwrap_or_else(|| window.is_visible().unwrap_or(false));
    let active = visible && shown;
    let (active_sender, mut active_receiver) = tokio::sync::watch::channel(active);
    let mut receiver = state.docker.subscribe();
    if active {
        channel
            .send(receiver.borrow().clone())
            .map_err(|_| error("Docker channel closed"))?;
        state.docker.set_visible(true);
    }
    let handle = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                result = receiver.changed() => { if result.is_err() { break; } },
                result = active_receiver.changed() => { if result.is_err() { break; } },
            }
            let current = receiver.borrow_and_update().clone();
            if *active_receiver.borrow_and_update() && channel.send(current).is_err() {
                break;
            }
        }
        if let Some(state) = handle.try_state::<AppState>() {
            state.docker_streams.close(&state, id);
        }
    });
    streams.insert(
        id,
        DockerStream {
            window: window.label().into(),
            wants_visible: visible,
            active: active_sender,
            task: task.inner().abort_handle(),
        },
    );
    Ok(id)
}
#[tauri::command]
#[specta::specta]
pub fn docker_unsubscribe(state: State<'_, AppState>, stream: u32) -> Result<(), CommandError> {
    state.docker_streams.close(&state, stream);
    Ok(())
}
#[tauri::command]
#[specta::specta]
pub async fn docker_detail(
    state: State<'_, AppState>,
    engine_id: String,
    container_id: String,
) -> Result<DockerContainer, CommandError> {
    state
        .docker
        .detail_live(&engine_id, &container_id)
        .await
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub async fn docker_retained(
    state: State<'_, AppState>,
    request: DockerRetainedRequest,
) -> Result<DockerRetainedPage, CommandError> {
    let handle = state.docker.clone();
    tauri::async_runtime::spawn_blocking(move || handle.retained(&request))
        .await
        .map_err(|_| error("Retained Docker catalog task failed"))?
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub async fn docker_comparison(
    state: State<'_, AppState>,
    request: DockerComparisonRequest,
) -> Result<DockerComparisonPage, CommandError> {
    let handle = state.docker.clone();
    tauri::async_runtime::spawn_blocking(move || handle.comparison(&request))
        .await
        .map_err(|_| error("Docker comparison task failed"))?
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub async fn docker_history(
    state: State<'_, AppState>,
    request: DockerHistoryRequest,
) -> Result<DockerHistoryPage, CommandError> {
    let handle = state.docker.clone();
    tauri::async_runtime::spawn_blocking(move || handle.history(&request))
        .await
        .map_err(|_| error("Docker history task failed"))?
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub async fn docker_events(
    state: State<'_, AppState>,
    request: DockerEventsRequest,
) -> Result<DockerEventsPage, CommandError> {
    let handle = state.docker.clone();
    tauri::async_runtime::spawn_blocking(move || handle.query_events(&request))
        .await
        .map_err(|_| error("Docker events task failed"))?
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub fn docker_logs(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    request: DockerLogsRequest,
    channel: Channel<DockerLogBatch>,
) -> Result<u32, CommandError> {
    let visibility = state.docker_streams.visibility.lock_ok();
    if !visibility
        .get(window.label())
        .copied()
        .unwrap_or_else(|| window.is_visible().unwrap_or(false))
    {
        return Err(error("Open a visible container detail pane to read logs"));
    }
    let mut sub = state.docker.open_logs(request).map_err(error)?;
    let id = sub.id;
    state.docker_streams.track_log(id, window.label());
    drop(visibility);
    let handle = state.docker.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(initial) = sub.initial.take() {
            let complete = initial.complete;
            if channel.send(initial).is_err() || complete {
                handle.close_logs(id);
                if let Some(state) = app.try_state::<AppState>() {
                    state.docker_streams.remove_log(id);
                }
                return;
            }
        }
        loop {
            match sub.receiver.recv().await {
                Ok(batch) => {
                    let done = batch.complete;
                    if channel.send(batch).is_err() || done {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let batch = DockerLogBatch {
                        generation: handle.snapshot().generation,
                        lines: Vec::new(),
                        truncated: true,
                        complete: false,
                        error: None,
                    };
                    if channel.send(batch).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        if let Some(state) = app.try_state::<AppState>() {
            state.docker.close_logs(id);
            state.docker_streams.remove_log(id);
        }
    });
    Ok(id)
}
#[tauri::command]
#[specta::specta]
pub fn docker_close_logs(state: State<'_, AppState>, stream: u32) -> Result<(), CommandError> {
    state.docker.close_logs(stream);
    state.docker_streams.remove_log(stream);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_window_dispatches_only_its_log_subscribers_and_reopen_can_register_again() {
        let streams = DockerStreams::default();
        streams.track_log(1, "main");
        streams.track_log(2, "other-window");
        streams.track_log(3, "main");
        let mut cancelled = Vec::new();
        streams.dispatch_window_log_cancellations("main", |id| cancelled.push(id));
        cancelled.sort_unstable();
        assert_eq!(cancelled, [1, 3]);
        assert_eq!(
            streams.logs.lock_ok().get(&2).map(String::as_str),
            Some("other-window")
        );
        streams.track_log(4, "main");
        cancelled.clear();
        streams.dispatch_window_log_cancellations("main", |id| cancelled.push(id));
        assert_eq!(cancelled, [4]);
        assert_eq!(streams.logs.lock_ok().len(), 1);
    }
}

#[tauri::command]
#[specta::specta]
pub async fn docker_storage_scan(
    state: State<'_, AppState>,
    force: bool,
) -> Result<kelvo_engine::docker::storage::DockerStorageView, CommandError> {
    Ok(state.docker.storage_scan(force).await)
}
#[tauri::command]
#[specta::specta]
pub async fn docker_storage_prepare(
    state: State<'_, AppState>,
    selections: Vec<kelvo_schema::docker_storage::DockerStorageSelection>,
) -> Result<kelvo_engine::docker::storage::DockerStorageReview, CommandError> {
    state
        .docker
        .storage_prepare(selections)
        .await
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub async fn docker_storage_execute(
    state: State<'_, AppState>,
    token: String,
    confirm_volumes: bool,
) -> Result<kelvo_engine::docker::storage::DockerStorageOperation, CommandError> {
    state
        .docker
        .storage_execute(&token, confirm_volumes)
        .map_err(error)
}
#[tauri::command]
#[specta::specta]
pub fn docker_storage_operation(
    state: State<'_, AppState>,
    operation_id: String,
) -> Result<kelvo_engine::docker::storage::DockerStorageOperation, CommandError> {
    state.docker.storage_operation(&operation_id).map_err(error)
}

#[tauri::command]
#[specta::specta]
pub async fn docker_container_action(
    state: State<'_, AppState>,
    engine_id: String,
    container_id: String,
    action: kelvo_schema::docker_storage::DockerContainerAction,
) -> Result<kelvo_engine::docker::storage::DockerStorageOperation, CommandError> {
    state
        .docker
        .container_action(&engine_id, &container_id, action)
        .map_err(error)
}
