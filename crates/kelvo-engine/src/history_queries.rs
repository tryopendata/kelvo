//! Queries merge durable rows with a recent snapshot taken after the read.
//! Unavailable history falls back to recent data; other read failures remain errors.
use crate::{HistoryStore, HistoryStoreError, NetAttribution, RecentNet};
use kelvo_schema::HostId;
use kelvo_store::{BucketRow, HistoryQuery, HistoryResult, StoreError};

impl HistoryStore {
    pub fn history_through_now(
        &self,
        query: &HistoryQuery,
        recent: impl Fn() -> Vec<BucketRow>,
    ) -> Result<HistoryResult, HistoryStoreError> {
        match self.read(|r| r.history_after(query, &recent)) {
            Err(
                HistoryStoreError::Unavailable { .. }
                | HistoryStoreError::Store(StoreError::WriterGone),
            ) => Ok(kelvo_store::history_recent(query, &recent())?),
            read => read,
        }
    }
    /// Complete-to is captured with the same recent snapshot, never a later tick.
    pub fn network_by_app(
        &self,
        host: HostId,
        from_ms: i64,
        to_ms: i64,
        recent: impl FnOnce() -> RecentNet,
    ) -> Result<(NetAttribution, Option<i64>), HistoryStoreError> {
        let mut recent = Some(recent);
        let mut complete_to_ms = None;
        let mut take = || {
            let r = recent.take().map(|f| f()).unwrap_or_default();
            complete_to_ms = r.complete_to_ms;
            r.buckets
        };
        // The store can close between a check and the read, so the read's own
        // `history_unavailable` is what sends this to the ring alone.
        let read = match self.read(|r| r.net_by_app_after(host, from_ms, to_ms, &mut take)) {
            Err(
                HistoryStoreError::Unavailable { .. }
                | HistoryStoreError::Store(StoreError::WriterGone),
            ) => kelvo_store::net_by_app_recent(from_ms, to_ms, &take())?,
            read => read?,
        };
        Ok((crate::attribute(read), complete_to_ms))
    }
}

impl HistoryStore {
    /// The remote/source owner supplies its recorder snapshot after the durable read.
    /// An unavailable source store remains unavailable; live rings are separate APIs.
    pub fn historical_usage(
        &self,
        request: &kelvo_schema::durable_usage::HistoricalUsageRequest,
        recent: impl FnOnce() -> Vec<kelvo_store::usage::UsageBucket>,
    ) -> Result<kelvo_schema::durable_usage::HistoricalUsageReply, HistoryStoreError> {
        self.read(|reader| reader.historical_usage_after(request, recent))
    }

    pub fn usage_processes(
        &self,
        request: &kelvo_schema::durable_usage::UsageProcessRequest,
        recent: impl FnOnce() -> Vec<kelvo_store::usage::UsageBucket>,
    ) -> Result<kelvo_schema::durable_usage::UsageProcessReply, HistoryStoreError> {
        self.read(|reader| reader.usage_processes_after(request, recent))
    }
}
