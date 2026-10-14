use crate::MirrorError;
use kelvo_proto::*;
use kelvo_schema::{Gap, SyncKinds};
use kelvo_store::*;

pub fn page_to_wire(read: CursorRead) -> Message {
    match read {
        CursorRead::Truncated {
            tier,
            earliest_ts_ms,
            epoch,
        } => Message::Truncated {
            tier,
            earliest_ts_ms,
            epoch,
        },
        CursorRead::Page(p) => Message::SyncPage(SyncPage {
            tier: p.tier,
            epoch: p.epoch,
            layouts: p
                .layouts
                .into_iter()
                .map(|l| WireLayout {
                    layout_no: l.layout_no,
                    series: l.series,
                })
                .collect(),
            rows: p
                .rows
                .into_iter()
                .map(|r| WireBucket {
                    seq: r.seq,
                    bucket_ts: r.bucket_ts,
                    layout_no: r.layout_no,
                    stats: r.stats,
                })
                .collect(),
            gaps: p
                .gaps
                .into_iter()
                .map(|g| WireGap {
                    source_id: g.source_id,
                    seq: g.seq,
                    start_ms: g.gap.start_ms,
                    end_ms: g.gap.end_ms,
                    module: g.gap.module,
                    reason: g.gap.reason,
                })
                .collect(),
            events: p
                .events
                .into_iter()
                .map(|e| WireEvent {
                    source_id: e.source_id,
                    seq: e.seq,
                    ts_ms: e.ts_ms,
                    kind: e.kind,
                    payload: e.payload,
                })
                .collect(),
            last_seq: p.last_seq,
            more: p.more,
        }),
    }
}
pub fn page_from_wire(p: SyncPage, kinds: SyncKinds) -> Result<CursorPage, MirrorError> {
    if p.rows.len() + p.gaps.len() + p.events.len() > 2000
        || p.layouts.len() > 2000
        || p.layouts.iter().any(|l| l.series.len() > 8192)
    {
        return Err(MirrorError::ResourceLimit(
            "invalid remote message: sync page cardinality exceeds bounds",
        ));
    }
    if p.gaps.iter().any(|g| g.source_id.is_none_or(|id| id <= 0))
        || p.events
            .iter()
            .any(|e| e.source_id.is_none_or(|id| id <= 0))
    {
        return Err(MirrorError::InvalidInput(
            "invalid remote message: sync mutations require stable source row identities",
        ));
    }
    Ok(CursorPage {
        tier: p.tier,
        epoch: p.epoch,
        kinds,
        layouts: p
            .layouts
            .into_iter()
            .map(|l| PageLayout {
                layout_no: l.layout_no,
                series: l.series,
            })
            .collect(),
        rows: p
            .rows
            .into_iter()
            .map(|r| PageRow {
                seq: r.seq,
                bucket_ts: r.bucket_ts,
                layout_no: r.layout_no,
                stats: r.stats,
            })
            .collect(),
        gaps: p
            .gaps
            .into_iter()
            .map(|g| PageGap {
                source_id: g.source_id,
                seq: g.seq,
                gap: Gap {
                    start_ms: g.start_ms,
                    end_ms: g.end_ms,
                    module: g.module,
                    reason: g.reason,
                },
            })
            .collect(),
        events: p
            .events
            .into_iter()
            .map(|e| PageEvent {
                source_id: e.source_id,
                seq: e.seq,
                ts_ms: e.ts_ms,
                kind: e.kind,
                payload: e.payload,
            })
            .collect(),
        last_seq: p.last_seq,
        more: p.more,
    })
}
