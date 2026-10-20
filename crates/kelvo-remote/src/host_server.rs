//! The metrics server for one host this process samples: the [`ServerProvider`] the app
//! and the headless agent both serve over their metrics socket. [`ServedHost`] is the
//! per-host data it reads; history comes from that host's [`HistoryStore`].
use crate::ServerProvider;
use crate::server::status_to_wire;
use kelvo_engine::{HistoryStore, LiveHub};
use kelvo_proto::{
    Hello, Message, RemoteInterest, SourceStatus, SyncRequest, WireBucket, WireLayout,
};
use kelvo_schema::{Capabilities, HostRecord, SyncKinds, Tier};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// One sampled host as the metrics server sees it.
pub trait ServedHost: Send + Sync + 'static {
    fn record(&self) -> HostRecord;
    fn capabilities(&self) -> Capabilities;
    fn hub(&self) -> &LiveHub;
    /// Replace one session's interest; the default interest releases its demand.
    fn set_remote_interest(&self, session: u64, interest: RemoteInterest);
}

/// What a host's viewers want collected: its own windows (`local`) and each remote
/// session. The source runs at the union, the fastest process period winning.
#[derive(Default)]
pub struct InterestSet {
    pub local: RemoteInterest,
    remote: HashMap<u64, RemoteInterest>,
}

impl InterestSet {
    /// Replaces `session`'s interest; the default interest removes the session.
    pub fn set_remote(&mut self, session: u64, interest: RemoteInterest) {
        if interest == RemoteInterest::default() {
            self.remote.remove(&session);
        } else {
            self.remote.insert(session, interest);
        }
    }

    pub fn merged(&self) -> RemoteInterest {
        let mut interest = self.local.clone();
        for remote in self.remote.values() {
            interest.detail |= remote.detail;
            interest.network_processes |= remote.network_processes;
            interest.gpu_processes |= remote.gpu_processes;
            interest.port_processes |= remote.port_processes;
            if let Some(period) = remote.process_period_ms {
                interest.process_period_ms =
                    Some(interest.process_period_ms.map_or(period, |p| p.min(period)));
            }
        }
        interest
    }
}

pub struct HostServer<H> {
    host: Arc<H>,
    history: Arc<HistoryStore>,
}

impl<H: ServedHost> HostServer<H> {
    pub fn new(host: Arc<H>, history: Arc<HistoryStore>) -> Self {
        Self { host, history }
    }
}

impl<H: ServedHost> ServerProvider for HostServer<H> {
    fn hello(&self) -> Hello {
        Hello {
            proto_version: kelvo_proto::PROTO_VERSION,
            min_compatible: kelvo_proto::MIN_COMPATIBLE,
            app_version: env!("CARGO_PKG_VERSION").into(),
            host: self.host.record().identity(),
            db_instance_uuid: self.history.read(|r| Ok(r.epoch())).unwrap_or(Uuid::nil()),
            capabilities: self.host.capabilities(),
            features: kelvo_proto::local_features(),
        }
    }
    fn live(&self) -> LiveHub {
        self.host.hub().clone()
    }
    fn status(&self) -> SourceStatus {
        status_to_wire(&self.host.hub().status(), self.history.is_available())
    }
    fn recent(&self, from: i64, to: i64) -> Result<Message, String> {
        let now = kelvo_engine::wall_ms();
        let rows = self.host.hub().recent_rows(
            self.host.record().id,
            from.max(now - 300_000),
            to.min(now),
        );
        let mut layouts: Vec<WireLayout> = Vec::new();
        let mut wire = Vec::new();
        for row in rows.into_iter().filter(|r| r.tier == Tier::S10) {
            let idx = layouts
                .iter()
                .position(|l| l.series.as_slice() == row.series.as_ref())
                .unwrap_or_else(|| {
                    let i = layouts.len();
                    layouts.push(WireLayout {
                        layout_no: i as u32,
                        series: row.series.to_vec(),
                    });
                    i
                });
            wire.push(WireBucket {
                seq: 0,
                bucket_ts: row.bucket_ts,
                layout_no: idx as u32,
                stats: row.stats,
            });
        }
        let message = Message::RecentRows {
            epoch: self.hello().db_instance_uuid,
            tier: Tier::S10,
            complete_to_ms: now.div_euclid(10_000) * 10_000,
            layouts,
            rows: wire,
        };
        if kelvo_proto::encode_message(&message)
            .map_err(|e| e.to_string())?
            .len()
            > 256 * 1024
        {
            return Err(
                "Recent history exceeds the page limit; durable history remains available".into(),
            );
        }
        Ok(message)
    }

    fn set_interest(&self, session: u64, interest: RemoteInterest) {
        self.host.set_remote_interest(session, interest);
    }
    fn sync(&self, req: &SyncRequest, kinds: SyncKinds) -> Result<Message, String> {
        let host = self.host.record().id;
        let mut max = req.max_rows.clamp(1, 2000);
        let cap = req.max_bytes.unwrap_or(16 * 1024).clamp(1024, 256 * 1024) as usize;
        loop {
            let read = self
                .history
                .read(|r| r.read_after_since(host, req.tier, req.after, max, kinds, req.from_ms))
                .map_err(|e| e.to_string())?;
            let message = crate::page_to_wire(read);
            if kelvo_proto::encode_message(&message)
                .map_err(|e| e.to_string())?
                .len()
                <= cap
            {
                return Ok(message);
            }
            if max == 1 {
                if kelvo_proto::encode_message(&message)
                    .map_err(|e| e.to_string())?
                    .len()
                    <= 256 * 1024
                {
                    return Ok(message);
                }
                return Err("A history row exceeds the maximum page size".into());
            }
            max = (max / 2).max(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelvo_schema::lock::LockExt;
    use kelvo_schema::{HostId, HostInfo, OsKind};
    use std::sync::Mutex;

    struct Host {
        record: HostRecord,
        hub: LiveHub,
        interests: Mutex<Vec<(u64, RemoteInterest)>>,
    }
    impl ServedHost for Host {
        fn record(&self) -> HostRecord {
            self.record.clone()
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        fn hub(&self) -> &LiveHub {
            &self.hub
        }
        fn set_remote_interest(&self, session: u64, interest: RemoteInterest) {
            self.interests.lock_ok().push((session, interest));
        }
    }

    fn host() -> Arc<Host> {
        Arc::new(Host {
            record: HostRecord {
                id: HostId(Uuid::from_u128(7)),
                is_local: true,
                display_name: "Server".into(),
                info: HostInfo {
                    os: OsKind::Linux,
                    os_version: String::new(),
                    model: None,
                    chip: None,
                    chip_known: false,
                    cpu_topology: Vec::new(),
                    mem_total_bytes: 0,
                    boot_time_ms: 0,
                    gpu_dvfs_mhz: Vec::new(),
                    boot_mounts: Vec::new(),
                },
            },
            hub: LiveHub::default(),
            interests: Mutex::default(),
        })
    }

    fn request() -> SyncRequest {
        SyncRequest {
            tier: Tier::S10,
            after: None,
            max_rows: 100,
            from_ms: None,
            max_bytes: None,
        }
    }

    /// The handshake, status and history pages come from the host and its store.
    #[test]
    fn serves_the_host_and_its_history() {
        let dir = tempfile::tempdir().unwrap();
        let history = Arc::new(HistoryStore::open(dir.path()));
        let host = host();
        assert!(history.register_host(&host.record).is_some());
        let server = HostServer::new(host.clone(), history.clone());

        let hello = server.hello();
        assert_eq!(hello.host, host.record.identity());
        assert_eq!(
            hello.db_instance_uuid,
            history.read(|r| Ok(r.epoch())).unwrap()
        );
        assert_ne!(hello.db_instance_uuid, Uuid::nil());
        assert!(server.status().history_available);
        assert!(matches!(
            server.sync(&request(), SyncKinds::ALL),
            Ok(Message::SyncPage(page)) if page.tier == Tier::S10 && page.epoch == hello.db_instance_uuid
        ));
        let interest = RemoteInterest {
            detail: true,
            ..Default::default()
        };
        server.set_interest(3, interest.clone());
        assert_eq!(*host.interests.lock_ok(), vec![(3, interest)]);
    }

    /// Without history the session still runs: a nil epoch, `history_available: false`,
    /// and sync answers with an error the server sends as a message.
    #[test]
    fn unavailable_history_is_reported_not_fatal() {
        let server = HostServer::new(host(), Arc::new(HistoryStore::unavailable()));
        assert_eq!(server.hello().db_instance_uuid, Uuid::nil());
        assert!(!server.status().history_available);
        assert!(server.sync(&request(), SyncKinds::ALL).is_err());
    }

    #[test]
    fn closing_one_viewer_preserves_other_viewers_and_local_demand() {
        let mut interests = InterestSet::default();
        interests.local.detail = true;
        interests.local.process_period_ms = Some(5000);
        interests.set_remote(
            1,
            RemoteInterest {
                detail: true,
                process_period_ms: Some(1000),
                gpu_processes: true,
                ..Default::default()
            },
        );
        interests.set_remote(
            2,
            RemoteInterest {
                network_processes: true,
                process_period_ms: Some(3000),
                ..Default::default()
            },
        );
        let merged = interests.merged();
        assert_eq!(merged.process_period_ms, Some(1000));
        assert!(merged.gpu_processes && merged.network_processes && merged.detail);
        interests.set_remote(1, RemoteInterest::default());
        let merged = interests.merged();
        assert_eq!(merged.process_period_ms, Some(3000));
        assert!(!merged.gpu_processes);
        assert!(merged.network_processes && merged.detail);
        interests.set_remote(2, RemoteInterest::default());
        assert_eq!(interests.merged(), interests.local);
    }
}
