use crate::MirrorError;
use kelvo_proto::*;
use kelvo_schema::{Catalog, Gap, SyncKinds};
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
/// Most rows, gaps and events (together), and most layouts, a sync page may carry.
const MAX_PAGE_ITEMS: usize = 2000;
/// Most series one layout may carry.
const MAX_LAYOUT_SERIES: usize = 8192;

pub fn page_from_wire(p: SyncPage, kinds: SyncKinds) -> Result<CursorPage, MirrorError> {
    if p.rows.len() + p.gaps.len() + p.events.len() > MAX_PAGE_ITEMS
        || p.layouts.len() > MAX_PAGE_ITEMS
        || p.layouts.iter().any(|l| l.series.len() > MAX_LAYOUT_SERIES)
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

/// Drops each layout's series beyond their metric's cardinality cap (D-115), and their
/// `(min, max, avg)` stats from every row in that layout, keeping the first series in
/// layout order as a local engine would. A remote host on an older build, or one that
/// does not cap, then cannot grow a mirror past what a local history holds. Input over
/// the page bounds, and a row whose stats do not match its layout, are left alone for
/// validation to reject. Returns the number of series dropped.
pub(crate) fn cap_layouts(
    catalog: &Catalog,
    layouts: &mut [WireLayout],
    rows: &mut [WireBucket],
) -> usize {
    if rows.len() > MAX_PAGE_ITEMS
        || layouts.len() > MAX_PAGE_ITEMS
        || layouts.iter().any(|l| l.series.len() > MAX_LAYOUT_SERIES)
    {
        return 0;
    }
    let mut dropped = 0;
    for layout in layouts {
        let over = catalog.over_cap(&layout.series);
        let width = layout.series.len().saturating_mul(3);
        let mut in_layout = rows.iter().filter(|r| r.layout_no == layout.layout_no);
        // Trimming the layout under a malformed row could make that row fit it.
        if over.is_empty() || in_layout.any(|r| r.stats.len() != width) {
            continue;
        }
        dropped += over.len();
        for row in rows.iter_mut().filter(|r| r.layout_no == layout.layout_no) {
            let mut i = 0;
            row.stats.retain(|_| {
                i += 1;
                over.binary_search(&((i - 1) / 3)).is_err()
            });
        }
        let mut i = 0;
        layout.series.retain(|_| {
            i += 1;
            over.binary_search(&(i - 1)).is_err()
        });
    }
    dropped
}
