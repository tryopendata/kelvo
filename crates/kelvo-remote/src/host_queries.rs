//! Answers to remote queries (D-117) from the host this process samples: per-app use from
//! its usage ring and its space report service. A scan belongs to the session that
//! started it: only that session cancels or clears it, and it is cancelled and its report
//! dropped when that session ends. A remote scan never enters privacy-protected
//! locations, so a remote user cannot read through this app's macOS privacy grants.
use crate::host_server::{HostServer, ServedHost};
use kelvo_engine::disk_space::{SpaceError, SpaceService};
use kelvo_proto::{Answer, ErrorCode, Query};
use kelvo_schema::JS_SAFE_INT_MAX;
use kelvo_schema::disk_space::SpaceVolume;
use kelvo_schema::lock::LockExt;
use std::sync::{Arc, Mutex};

/// The host's space report service, and which session started its scan.
pub struct SpaceAccess {
    service: Arc<SpaceService>,
    owner: Mutex<Option<u64>>,
}

impl SpaceAccess {
    pub fn new(service: Arc<SpaceService>) -> Self {
        Self {
            service,
            owner: Mutex::new(None),
        }
    }

    fn answer(&self, session: u64, query: Query) -> Answer {
        let service = &self.service;
        let result = match query {
            Query::SpaceVolumes => Ok(Answer::SpaceVolumes(
                service
                    .volumes()
                    .into_iter()
                    .map(|v| SpaceVolume {
                        protected: Vec::new(),
                        ..v
                    })
                    .collect(),
            )),
            // `include` names protected locations, which a remote scan never enters.
            Query::SpaceStart { volume, .. } => service.start(&volume, &[]).map(|status| {
                *self.owner.lock_ok() = Some(session);
                Answer::SpaceStatus(status)
            }),
            Query::SpaceStatus => Ok(Answer::SpaceStatus(service.status())),
            Query::SpaceCancel => {
                if matches!(*self.owner.lock_ok(), Some(owner) if owner != session) {
                    return Answer::Error {
                        code: ErrorCode::Internal,
                        message: "Another Kelvo started this scan".into(),
                    };
                }
                service.cancel();
                Ok(Answer::Done)
            }
            Query::SpaceClear => {
                self.release(session);
                Ok(Answer::Done)
            }
            Query::SpaceReport => Ok(Answer::SpaceReport(service.report())),
            Query::SpaceSubtree { report, node } => {
                service.subtree(report, node).map(Answer::SpaceSubtree)
            }
            Query::SpaceLargestFiles { report } => {
                service.largest_files(report).map(Answer::SpaceFiles)
            }
            Query::SpacePath { report, target } => service
                .reveal_path(report, target)
                .map(|path| Answer::SpacePath(path.to_string_lossy().into_owned())),
            _ => return unsupported(),
        };
        result.unwrap_or_else(|e: SpaceError| Answer::Error {
            code: ErrorCode::Internal,
            message: e.to_string(),
        })
    }

    /// A session ended: a scan it started stops, and its report goes, so a controller
    /// that went away leaves no walk running and no tree held on the host.
    fn release(&self, session: u64) {
        let mut owner = self.owner.lock_ok();
        if *owner == Some(session) {
            *owner = None;
            self.service.cancel();
            self.service.clear();
        }
    }
}

fn unsupported() -> Answer {
    Answer::Error {
        code: ErrorCode::Unsupported,
        message: "This host does not answer that query".into(),
    }
}

impl<H: ServedHost> HostServer<H> {
    pub(crate) fn answer_query(&self, session: u64, query: Query) -> Answer {
        match query {
            Query::UsageByApp {
                from_ms,
                to_ms,
                by,
                limit,
            } => Answer::UsageByApp(self.host().hub().usage_by_app(
                // Timestamps are below 2^53 (kelvo-schema); a wire value past that would
                // overflow the bucket rounding.
                from_ms.clamp(-JS_SAFE_INT_MAX, JS_SAFE_INT_MAX),
                to_ms.clamp(-JS_SAFE_INT_MAX, JS_SAFE_INT_MAX),
                by,
                usize::try_from(limit).unwrap_or(usize::MAX).min(500),
            )),
            Query::Unknown => unsupported(),
            space => match self.space() {
                Some(access) => access.answer(session, space),
                None => unsupported(),
            },
        }
    }

    pub(crate) fn release_queries(&self, session: u64) {
        if let Some(access) = self.space() {
            access.release(session);
        }
    }
}
