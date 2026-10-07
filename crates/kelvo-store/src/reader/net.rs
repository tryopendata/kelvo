//! `net_by_app`: per-app network bytes over a range, stitched from the three network
//! tiers the way `Auto` history reads pick a tier, but per sub-range (D-089).
//!
//! Each minute is answered by its 10 s rows when it has any, else by its `proc_net_1m`
//! row. A quarter hour none of whose minutes has either is answered by its
//! `proc_net_15m` row. The tiers never hold the same span twice in normal operation (the
//! 1 m row is the sum of the minute's 10 s rows, and minutes are deleted as they are
//! rolled into quarters), so taking one per sub-range sums every byte once. Buckets are
//! used whole: the result's `[from_ms, to_ms)` is the request widened to the buckets that
//! answered it.

use std::collections::{BTreeMap, HashMap};

use kelvo_schema::{HostId, Tier, ceil_to, floor_to};
use rusqlite::{OptionalExtension, params};

use super::Reader;
use crate::blob::{self, NetHeader, OTHER_APPS};
use crate::db::net_table;
use crate::error::{Result, StoreError};
use crate::types::{NetApp, NetBucket, NetByApp, NetSpan};

const S10: i64 = Tier::S10.bucket_ms().expect("S10 has a bucket width");
const M1: i64 = Tier::M1.bucket_ms().expect("M1 has a bucket width");
const M15: i64 = Tier::M15.bucket_ms().expect("M15 has a bucket width");

/// A 10 s bucket: a stored row, or one of the caller's in-memory buckets.
enum Ten<'a> {
    Stored(Vec<u8>),
    Recent(&'a NetBucket),
}

/// Running sums of a read.
#[derive(Default)]
struct Sums {
    header: NetHeader,
    by_id: HashMap<u32, (u64, u64)>,
    by_name: HashMap<String, (u64, u64)>,
    /// `measured_ms` as u64: the header's u32 would overflow past 49 days.
    measured_ms: u64,
}

fn add_to(slot: &mut (u64, u64), rx: u64, tx: u64) {
    slot.0 = slot.0.saturating_add(rx);
    slot.1 = slot.1.saturating_add(tx);
}

impl Sums {
    fn add_header(&mut self, h: &NetHeader) {
        self.measured_ms = self.measured_ms.saturating_add(u64::from(h.measured_ms));
        self.header.add(&NetHeader {
            measured_ms: 0,
            ..*h
        });
    }

    fn add_blob(&mut self, b: &[u8]) -> Result<()> {
        let (header, rows) = blob::unpack_net(b)?;
        self.add_header(&header);
        for r in rows {
            add_to(self.by_id.entry(r.name_id).or_default(), r.rx, r.tx);
        }
        Ok(())
    }

    fn add_ten(&mut self, ten: &Ten<'_>) -> Result<()> {
        match ten {
            Ten::Stored(b) => self.add_blob(b),
            Ten::Recent(b) => {
                self.add_recent(b);
                Ok(())
            }
        }
    }

    fn add_recent(&mut self, b: &NetBucket) {
        self.add_header(&NetHeader {
            measured_ms: b.measured_ms.min(S10 as u32),
            rx_bytes: b.iface_rx_bytes,
            tx_bytes: b.iface_tx_bytes,
            rx_pkts: b.iface_rx_pkts,
            tx_pkts: b.iface_tx_pkts,
        });
        for a in &b.apps {
            match a.name.as_deref().filter(|n| !n.is_empty()) {
                Some(n) => add_to(
                    self.by_name.entry(n.to_owned()).or_default(),
                    a.rx_bytes,
                    a.tx_bytes,
                ),
                None => add_to(
                    self.by_id.entry(OTHER_APPS).or_default(),
                    a.rx_bytes,
                    a.tx_bytes,
                ),
            }
        }
    }
}

/// Appends `[from, to)` answered by `tier`, merging it into the last span when they meet
/// and agree.
fn push_span(spans: &mut Vec<NetSpan>, from_ms: i64, to_ms: i64, tier: Option<Tier>) {
    if let Some(last) = spans.last_mut()
        && last.to_ms == from_ms
        && last.tier == tier
    {
        last.to_ms = to_ms;
        return;
    }
    spans.push(NetSpan {
        from_ms,
        to_ms,
        tier,
    });
}

/// An empty read of a range that starts at `from10`.
fn empty(from10: i64) -> NetByApp {
    NetByApp {
        from_ms: from10,
        to_ms: from10,
        coverage: Vec::new(),
        resolution_ms: S10,
        measured_ms: 0,
        iface_rx_bytes: 0,
        iface_tx_bytes: 0,
        iface_rx_pkts: 0,
        iface_tx_pkts: 0,
        apps: Vec::new(),
    }
}

/// The read from its spans and sums; `name_of` resolves a stored `name_id`.
fn finish(
    from10: i64,
    spans: Vec<NetSpan>,
    mut sums: Sums,
    mut name_of: impl FnMut(u32) -> Result<String>,
) -> Result<NetByApp> {
    let mut out = empty(from10);
    if let (Some(first), Some(last)) = (spans.first(), spans.last()) {
        out.from_ms = first.from_ms;
        out.to_ms = last.to_ms;
    }
    out.resolution_ms = spans
        .iter()
        .filter_map(|s| s.tier.and_then(Tier::bucket_ms))
        .max()
        .unwrap_or(S10);
    out.coverage = spans;
    out.measured_ms = sums.measured_ms;
    out.iface_rx_bytes = sums.header.rx_bytes;
    out.iface_tx_bytes = sums.header.tx_bytes;
    out.iface_rx_pkts = sums.header.rx_pkts;
    out.iface_tx_pkts = sums.header.tx_pkts;

    let mut other = (0, 0);
    let mut by_name = std::mem::take(&mut sums.by_name);
    for (id, (rx, tx)) in sums.by_id {
        if id == OTHER_APPS {
            add_to(&mut other, rx, tx);
            continue;
        }
        let name = name_of(id)?;
        add_to(by_name.entry(name).or_default(), rx, tx);
    }
    let mut apps: Vec<NetApp> = by_name
        .into_iter()
        .map(|(name, (rx, tx))| NetApp {
            name: Some(name),
            rx_bytes: rx,
            tx_bytes: tx,
        })
        .collect();
    if (other.0 | other.1) != 0 {
        apps.push(NetApp {
            name: None,
            rx_bytes: other.0,
            tx_bytes: other.1,
        });
    }
    apps.retain(|a| a.total() > 0);
    apps.sort_by(|a, b| b.total().cmp(&a.total()).then_with(|| a.name.cmp(&b.name)));
    out.apps = apps;
    Ok(out)
}

/// Per-app network bytes over `[from_ms, to_ms)` from in-memory 10 s buckets alone (the
/// engine's ring): what a range query can still answer while the store is unavailable
/// and the app runs live-only. Spans with no bucket among `recent` read as not
/// recorded.
pub fn net_by_app_recent(
    from_ms: i64,
    to_ms: i64,
    recent: &[(i64, NetBucket)],
) -> Result<NetByApp> {
    let (from10, to10) = (floor_to(from_ms, S10), ceil_to(to_ms, S10));
    if to10 <= from10 {
        return Ok(empty(from10));
    }
    let tens: BTreeMap<i64, &NetBucket> = recent
        .iter()
        .map(|(ts, b)| (floor_to(*ts, S10), b))
        .filter(|(ts, _)| (from10..to10).contains(ts))
        .collect();
    let mut sums = Sums::default();
    let mut spans = Vec::new();
    let mut t = from10;
    for (&ts, b) in &tens {
        if t < ts {
            push_span(&mut spans, t, ts, None);
        }
        sums.add_recent(b);
        push_span(&mut spans, ts, ts + S10, Some(Tier::S10));
        t = ts + S10;
    }
    if t < to10 {
        push_span(&mut spans, t, to10, None);
    }
    // In-memory buckets carry names, never a stored `name_id`.
    finish(from10, spans, sums, |id| {
        Err(StoreError::Corrupt(format!(
            "an in-memory network bucket has name id {id}"
        )))
    })
}

impl Reader {
    /// Per-app network bytes of `host` over `[from_ms, to_ms)`, from the stored tiers
    /// alone. See [`Reader::net_by_app_with`].
    pub fn net_by_app(&mut self, host: HostId, from_ms: i64, to_ms: i64) -> Result<NetByApp> {
        self.net_by_app_with(host, from_ms, to_ms, &[])
    }

    /// Per-app network bytes of `host` over `[from_ms, to_ms)`: the finest stored tier
    /// per sub-range (10 s where the minute has 10 s rows, else 1 m, else 15 m), summed
    /// exactly, with the interface counters and measured span of the same rows.
    ///
    /// `recent` is the engine's in-memory 10 s buckets `(bucket_ts, bucket)` (its ring
    /// and the open bucket): the writer commits every 5 minutes, so the newest buckets
    /// are not in the file yet. Each one replaces any stored row of the same 10 s bucket,
    /// and a minute holding one is read at 10 s, so a bucket both committed and still in
    /// the ring counts once. Pass every ring bucket overlapping the range; the reader
    /// does the deduplication. Timestamps are floored to the 10 s grid.
    pub fn net_by_app_with(
        &mut self,
        host: HostId,
        from_ms: i64,
        to_ms: i64,
        recent: &[(i64, NetBucket)],
    ) -> Result<NetByApp> {
        self.net_by_app_after(host, from_ms, to_ms, || recent)
    }

    /// [`Reader::net_by_app_with`], with the in-memory buckets taken by `recent` only
    /// after the stored rows are read, all in one read snapshot. The engine puts a
    /// closed bucket in its ring before it queues the bucket to the writer, so a ring
    /// copy taken after the read is never older than the stored row it replaces; one
    /// taken before could be the partial copy of a bucket that closed and was committed
    /// in between.
    pub fn net_by_app_after<R: AsRef<[(i64, NetBucket)]>>(
        &mut self,
        host: HostId,
        from_ms: i64,
        to_ms: i64,
        recent: impl FnOnce() -> R,
    ) -> Result<NetByApp> {
        let host_ref = self.host_ref(host)?;
        let (from10, to10) = (floor_to(from_ms, S10), ceil_to(to_ms, S10));
        if to10 <= from10 {
            return Ok(empty(from10));
        }
        // Whole quarters around the range: whether a minute or quarter has finer rows
        // is decided over all of it, not just the part inside the range.
        let (q_start, q_end) = (floor_to(from10, M15), ceil_to(to10, M15));
        let (stored_tens, minutes, quarters) = {
            // One snapshot for the three tiers: a commit or prune between the reads could
            // otherwise show a span in two tiers, or in none.
            let tx = self.conn.unchecked_transaction()?;
            let tens = self.net_rows(host_ref, Tier::S10, q_start, q_end)?;
            let minutes: BTreeMap<i64, Vec<u8>> = self
                .net_rows(host_ref, Tier::M1, q_start, q_end)?
                .into_iter()
                .collect();
            let quarters: BTreeMap<i64, Vec<u8>> = self
                .net_rows(host_ref, Tier::M15, q_start, q_end)?
                .into_iter()
                .collect();
            tx.finish()?;
            (tens, minutes, quarters)
        };
        let recent = recent();
        let mut tens: BTreeMap<i64, Ten<'_>> = stored_tens
            .into_iter()
            .map(|(ts, b)| (ts, Ten::Stored(b)))
            .collect();
        for (ts, b) in recent.as_ref() {
            let ts = floor_to(*ts, S10);
            if (q_start..q_end).contains(&ts) {
                tens.insert(ts, Ten::Recent(b));
            }
        }

        let mut sums = Sums::default();
        let mut spans: Vec<NetSpan> = Vec::new();
        let mut q = q_start;
        while q < q_end {
            let finer = tens.range(q..q + M15).next().is_some()
                || minutes.range(q..q + M15).next().is_some();
            if !finer && let Some(b) = quarters.get(&q) {
                sums.add_blob(b)?;
                push_span(&mut spans, q, q + M15, Some(Tier::M15));
                q += M15;
                continue;
            }
            let mut m = q;
            while m < q + M15 {
                let (lo, hi) = (m.max(from10), (m + M1).min(to10));
                if lo < hi {
                    if tens.range(m..m + M1).next().is_some() {
                        let mut t = lo;
                        while t < hi {
                            match tens.get(&t) {
                                Some(ten) => {
                                    sums.add_ten(ten)?;
                                    push_span(&mut spans, t, t + S10, Some(Tier::S10));
                                }
                                None => push_span(&mut spans, t, t + S10, None),
                            }
                            t += S10;
                        }
                    } else if let Some(b) = minutes.get(&m) {
                        sums.add_blob(b)?;
                        push_span(&mut spans, m, m + M1, Some(Tier::M1));
                    } else {
                        push_span(&mut spans, lo, hi, None);
                    }
                }
                m += M1;
            }
            q += M15;
        }

        finish(from10, spans, sums, |id| self.proc_name(id))
    }

    /// `(bucket_ts, blob)` of `tier`'s network table in `[from, to)`.
    fn net_rows(
        &self,
        host_ref: i64,
        tier: Tier,
        from: i64,
        to: i64,
    ) -> Result<Vec<(i64, Vec<u8>)>> {
        let table = net_table(tier)?;
        Ok(self
            .conn
            .prepare_cached(&format!(
                "SELECT bucket_ts, blob FROM {table}
                 WHERE host_id = ?1 AND bucket_ts >= ?2 AND bucket_ts < ?3"
            ))?
            .query_map(params![host_ref, from, to], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    }

    fn proc_name(&self, id: u32) -> Result<String> {
        self.conn
            .prepare_cached("SELECT name FROM proc_names WHERE id = ?1")?
            .query_row([id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| StoreError::Corrupt(format!("missing process name {id}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_merge_only_when_they_meet_and_agree() {
        let mut s = Vec::new();
        push_span(&mut s, 0, 10, Some(Tier::S10));
        push_span(&mut s, 10, 20, Some(Tier::S10));
        push_span(&mut s, 20, 30, None);
        push_span(&mut s, 40, 50, None);
        assert_eq!(
            s,
            vec![
                NetSpan {
                    from_ms: 0,
                    to_ms: 20,
                    tier: Some(Tier::S10)
                },
                NetSpan {
                    from_ms: 20,
                    to_ms: 30,
                    tier: None
                },
                NetSpan {
                    from_ms: 40,
                    to_ms: 50,
                    tier: None
                },
            ]
        );
    }
}
