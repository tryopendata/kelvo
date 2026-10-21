//! Typed remote queries (D-117): each asks the host one `Query` and takes the `Answer`
//! it expects. How a refusal or a mismatched answer becomes an error is decided once, in
//! `ask`.
use super::RemoteSource;
use crate::{RemoteError, Result};
use kelvo_proto::{Answer, ErrorCode, Query};
use kelvo_schema::disk_space::{
    SpaceFile, SpaceReport, SpaceScanStatus, SpaceSubtree, SpaceTarget, SpaceVolume,
};
use kelvo_schema::usage::{UsageByApp, UsageKey};
use std::time::Duration;

/// How long a query waits for its answer before the host counts as busy.
const ANSWER_WITHIN: Duration = Duration::from_secs(15);

impl RemoteSource {
    async fn ask<T>(&self, query: Query, pick: impl FnOnce(Answer) -> Option<T>) -> Result<T> {
        match self.call(query, ANSWER_WITHIN).await? {
            Answer::Error { code, message } => Err(match code {
                ErrorCode::Unsupported => RemoteError::Unsupported(message),
                ErrorCode::Busy => RemoteError::Busy(message),
                _ => RemoteError::Refused(message),
            }),
            // A newer host's answer this build cannot read.
            Answer::Unknown => Err(RemoteError::Unsupported(
                "This host answered in a form this Kelvo can't read; update Kelvo on this Mac"
                    .into(),
            )),
            answer => pick(answer)
                .ok_or_else(|| RemoteError::Protocol("the host sent an unexpected answer".into())),
        }
    }

    pub async fn usage_by_app(
        &self,
        from_ms: i64,
        to_ms: i64,
        by: UsageKey,
        limit: u32,
    ) -> Result<UsageByApp> {
        let query = Query::UsageByApp {
            from_ms,
            to_ms,
            by,
            limit,
        };
        self.ask(query, |a| match a {
            Answer::UsageByApp(usage) => Some(usage),
            _ => None,
        })
        .await
    }

    pub async fn space_volumes(&self) -> Result<Vec<SpaceVolume>> {
        self.ask(Query::SpaceVolumes, |a| match a {
            Answer::SpaceVolumes(v) => Some(v),
            _ => None,
        })
        .await
    }

    pub async fn space_start(&self, volume: String) -> Result<SpaceScanStatus> {
        // A remote scan never enters protected locations (D-117), so none are asked for.
        let query = Query::SpaceStart {
            volume,
            include: Vec::new(),
        };
        self.ask(query, status).await
    }

    pub async fn space_status(&self) -> Result<SpaceScanStatus> {
        self.ask(Query::SpaceStatus, status).await
    }

    pub async fn space_cancel(&self) -> Result<()> {
        self.ask(Query::SpaceCancel, done).await
    }

    /// Drops the scan this session started there (the dashboard closed).
    pub async fn space_clear(&self) -> Result<()> {
        self.ask(Query::SpaceClear, done).await
    }

    pub async fn space_report(&self) -> Result<Option<SpaceReport>> {
        self.ask(Query::SpaceReport, |a| match a {
            Answer::SpaceReport(r) => Some(r),
            _ => None,
        })
        .await
    }

    pub async fn space_subtree(&self, report: u32, node: Option<u32>) -> Result<SpaceSubtree> {
        self.ask(Query::SpaceSubtree { report, node }, |a| match a {
            Answer::SpaceSubtree(s) => Some(s),
            _ => None,
        })
        .await
    }

    pub async fn space_largest_files(&self, report: u32) -> Result<Vec<SpaceFile>> {
        self.ask(Query::SpaceLargestFiles { report }, |a| match a {
            Answer::SpaceFiles(f) => Some(f),
            _ => None,
        })
        .await
    }

    pub async fn space_path(&self, report: u32, target: SpaceTarget) -> Result<String> {
        self.ask(Query::SpacePath { report, target }, |a| match a {
            Answer::SpacePath(p) => Some(p),
            _ => None,
        })
        .await
    }
}

fn status(answer: Answer) -> Option<SpaceScanStatus> {
    match answer {
        Answer::SpaceStatus(s) => Some(s),
        _ => None,
    }
}

fn done(answer: Answer) -> Option<()> {
    matches!(answer, Answer::Done).then_some(())
}
