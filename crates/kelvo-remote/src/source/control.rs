//! The engine's handle on a remote source: interest changes go to the session loop
//! through the source's control channel, and stopping joins its thread.
use super::RemoteSource;
use kelvo_engine::{EngineStatus, SourceControl};
use kelvo_schema::{Settings, lock::LockExt};
use std::sync::Arc;

pub(super) struct Control {
    pub(super) source: Arc<RemoteSource>,
    pub(super) thread: Option<std::thread::JoinHandle<()>>,
}
impl SourceControl for Control {
    fn set_paused(&self, _paused: bool) {}
    fn set_process_interest(&self, v: Option<u32>) {
        self.source
            .control
            .send_modify(|s| s.interest.process_period_ms = v);
    }
    fn set_network_process_interest(&self, v: bool) {
        self.source
            .control
            .send_modify(|s| s.interest.network_processes = v);
    }
    fn set_gpu_process_interest(&self, v: bool) {
        self.source
            .control
            .send_modify(|s| s.interest.gpu_processes = v);
    }
    fn set_port_process_interest(&self, v: bool) {
        self.source
            .control
            .send_modify(|s| s.interest.port_processes = v);
    }
    fn set_network_history(&self, _on: bool) {}
    fn set_detail_interest(&self, v: bool) {
        self.source.control.send_modify(|s| s.interest.detail = v);
    }
    fn apply_settings(&self, _settings: &Settings) {}
    fn set_store(&self, _store: Option<kelvo_store::Writer>) {}
    fn status(&self) -> EngineStatus {
        self.source.status.lock_ok().clone()
    }
    fn stop(&mut self) {
        self.source.control.send_modify(|s| s.stop = true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
