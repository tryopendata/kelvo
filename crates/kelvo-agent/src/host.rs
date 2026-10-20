//! This machine as the agent samples it: the in-process engine, its hub and the
//! combined collection interest of every connected session. The headless counterpart
//! of the app's `HostEntry`.
use std::sync::{Arc, Mutex};

use anyhow::Context;
use kelvo_engine::{
    ContainerNames, EngineParts, LiveHub, LocalSource, NoScaleStore, Source, SourceHandle,
    SourceSink,
};
use kelvo_proto::RemoteInterest;
use kelvo_remote::{InterestSet, ServedHost};
use kelvo_schema::lock::LockExt;
use kelvo_schema::{Capabilities, HostRecord, Settings};
use kelvo_store::Writer;

pub struct AgentHost {
    record: HostRecord,
    source: Arc<LocalSource>,
    hub: LiveHub,
    handle: Mutex<Option<SourceHandle>>,
    interests: Mutex<InterestSet>,
}

impl AgentHost {
    /// Starts this platform's engine for `record` into a new hub and, when history is
    /// available, `store`. The agent learns no CPU power scales (`NoScaleStore`).
    /// Containers are named through `names`.
    pub fn start(
        record: HostRecord,
        settings: Settings,
        store: Option<Writer>,
        names: ContainerNames,
    ) -> anyhow::Result<Arc<Self>> {
        let source = Arc::new(LocalSource::new(
            record.clone(),
            EngineParts::platform_with(Arc::new(NoScaleStore), names),
            settings,
        ));
        let hub = LiveHub::default();
        let handle = Arc::clone(&source)
            .start(SourceSink {
                live: hub.clone(),
                store,
            })
            .context("starting the engine")?;
        Ok(Arc::new(Self {
            record,
            source,
            hub,
            handle: Mutex::new(Some(handle)),
            interests: Mutex::default(),
        }))
    }

    /// Stops the engine and waits for it: open buckets flush and open gaps close.
    /// Idempotent.
    pub fn stop(&self) {
        let handle = self.handle.lock_ok().take();
        if let Some(mut handle) = handle {
            handle.stop();
        }
    }

    /// What this process itself wants collected (`sample` asks for everything).
    pub fn set_local_interest(&self, interest: RemoteInterest) {
        self.change_interest(|all| all.local = interest);
    }

    fn change_interest(&self, update: impl FnOnce(&mut InterestSet)) {
        let mut all = self.interests.lock_ok();
        update(&mut all);
        let interest = all.merged();
        // Applied under the interest lock, so a concurrent release cannot overwrite a
        // newer aggregate with an older one (as in the app's `HostEntry`).
        if let Some(handle) = self.handle.lock_ok().as_ref() {
            let control = handle.control();
            control.set_process_interest(interest.process_period_ms);
            control.set_network_process_interest(interest.network_processes);
            control.set_gpu_process_interest(interest.gpu_processes);
            control.set_port_process_interest(interest.port_processes);
            control.set_detail_interest(interest.detail);
        }
    }
}

impl ServedHost for AgentHost {
    fn record(&self) -> HostRecord {
        self.record.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.source.capabilities()
    }

    fn hub(&self) -> &LiveHub {
        &self.hub
    }

    fn set_remote_interest(&self, session: u64, interest: RemoteInterest) {
        self.change_interest(|all| all.set_remote(session, interest));
    }
}

impl Drop for AgentHost {
    fn drop(&mut self) {
        self.stop();
    }
}
