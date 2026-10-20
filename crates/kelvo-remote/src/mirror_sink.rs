//! What a remote session needs from the controller's storage for one host: where sync
//! resumes, where pages and recent snapshots go, and the history its live ring refills
//! from. [`crate::Mirror`] is the SQLite implementation.
use crate::MirrorError;
use kelvo_proto::{SyncPage, WireBucket, WireLayout};
use kelvo_schema::{Cursor, SyncKinds, Tier};

/// Each sink owns its isolated mirror and reset generation. Methods must reject
/// writes after that mirror was detached/reset. Failed sync never stops live metrics.
pub trait MirrorSink: Send + Sync + 'static {
    fn cursor(
        &self,
        tier: Tier,
        epoch: uuid::Uuid,
        kinds: SyncKinds,
    ) -> std::result::Result<Option<Cursor>, MirrorError>;
    fn ingest(&self, page: SyncPage, kinds: SyncKinds) -> std::result::Result<(), MirrorError>;
    fn recent(
        &self,
        _epoch: uuid::Uuid,
        _tier: Tier,
        _complete_to_ms: i64,
        _layouts: Vec<WireLayout>,
        _rows: Vec<WireBucket>,
    ) -> std::result::Result<(), MirrorError> {
        Ok(())
    }
    fn truncated(
        &self,
        _tier: Tier,
        _epoch: uuid::Uuid,
        _earliest_ms: i64,
    ) -> std::result::Result<(), MirrorError> {
        Ok(())
    }
    fn enforce_budget(&self, _now_ms: i64) -> std::result::Result<(), MirrorError> {
        Ok(())
    }
    fn prune(&self, _now_ms: i64) -> std::result::Result<(), MirrorError> {
        Ok(())
    }
    /// The live ring's hour of history ending at `now_ms`, durable rows merged with the
    /// recent snapshot ([`kelvo_engine::ring_history_query`]); `None` when unreadable.
    fn ring_history(&self, _now_ms: i64) -> Option<kelvo_store::HistoryResult> {
        None
    }
    fn history_floor(&self, tier: Tier, now_ms: i64) -> i64 {
        now_ms.saturating_sub(match tier {
            Tier::S10 => 86_400_000,
            Tier::M1 => 604_800_000,
            _ => 2_592_000_000,
        })
    }
}
