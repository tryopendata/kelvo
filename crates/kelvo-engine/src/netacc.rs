//! Per-app network bytes in 10 s buckets (D-089).
//!
//! Two collectors feed it on their own cadences: the per-process network collector
//! (bytes per app identity over `[prev, now)` on the process ticks) and the network
//! collector (interface bytes and packets over its own `[prev, now)`, every tick while
//! shown, every 10 s otherwise). Their intervals do not share a phase, so each sample is
//! split pro rata across the S10 buckets its interval overlaps, apps and interface alike.
//! A bucket then holds both streams' bytes for the same wall-clock span, which is what
//! the remainder (interface minus apps) needs. Phase-locking the two collectors to the
//! bucket grid would have meant a new cadence kind in the collector crate and would still
//! break whenever either one skips a tick.
//!
//! The split conserves bytes exactly: each bucket gets the difference of the floored
//! cumulative shares at its two edges, so the shares of one sample add up to its bytes
//! (less only the part clipped off before a reset, which is not measured either).
//!
//! Wall time comes from the tick the sample arrived on, the same mapping the S10 tier
//! uses: `wall = tick.wall_ms - (tick.continuous_ns - ns)`. A clock step resets the
//! accumulator, so one bucket never mixes two timelines.
//!
//! [`NetRing`] is the hub's side: the last hour of closed buckets plus the open ones, for
//! range queries newer than the store's last commit.

use std::collections::VecDeque;
use std::sync::Arc;

use kelvo_collect::{IfaceNet, Interval, ProcessNet};
use kelvo_schema::{Tier, floor_to};
use kelvo_store::{NetApp, NetBucket};

/// Bucket width, ms: the S10 tier's.
pub const NET_BUCKET_MS: i64 = Tier::S10.bucket_ms().expect("S10 has a bucket width");
const NS_PER_MS: i128 = 1_000_000;
const BUCKET_NS: i128 = NET_BUCKET_MS as i128 * NS_PER_MS;

/// Closed buckets the hub keeps: one hour.
pub const NET_RING_BUCKETS: usize = 360;

/// How the continuous clock maps to the wall clock on one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Anchor {
    pub wall_ms: i64,
    pub continuous_ns: u64,
}

impl Anchor {
    /// Wall-clock nanoseconds at continuous `ns`.
    fn wall_ns(self, ns: u64) -> i128 {
        i128::from(self.wall_ms) * NS_PER_MS + (i128::from(ns) - i128::from(self.continuous_ns))
    }
}

/// One app's bytes in a bucket. `name: None` is "other apps".
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppBytes {
    pub name: Option<Arc<str>>,
    pub rx: u64,
    pub tx: u64,
}

/// One 10 s bucket while it is being filled, and in the ring once closed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NetSlot {
    pub start_ms: i64,
    /// Nanoseconds of the bucket covered by measured per-app samples.
    pub measured_ns: u64,
    pub iface_rx_bytes: u64,
    pub iface_tx_bytes: u64,
    pub iface_rx_pkts: u64,
    pub iface_tx_pkts: u64,
    pub apps: Vec<AppBytes>,
}

impl NetSlot {
    fn reset(&mut self, start_ms: i64) {
        self.start_ms = start_ms;
        self.measured_ns = 0;
        self.iface_rx_bytes = 0;
        self.iface_tx_bytes = 0;
        self.iface_rx_pkts = 0;
        self.iface_tx_pkts = 0;
        self.apps.clear();
    }

    pub fn end_ms(&self) -> i64 {
        self.start_ms + NET_BUCKET_MS
    }

    /// Measured span, ms, rounded and capped at the width: samples mapped through
    /// different ticks' anchors can disagree by the clock's slew.
    pub fn measured_ms(&self) -> u32 {
        let ms = (self.measured_ns + 500_000) / 1_000_000;
        u32::try_from(ms.min(NET_BUCKET_MS as u64)).unwrap_or(0)
    }

    /// Something per-app was measured in it. Buckets without are not written: no row is
    /// how history says "not measured".
    pub fn is_measured(&self) -> bool {
        self.measured_ns > 0
    }

    fn add_app(&mut self, name: &Option<Arc<str>>, rx: u64, tx: u64) {
        if rx == 0 && tx == 0 {
            return;
        }
        // Identities are interned, so a pointer comparison usually decides it.
        let same = |a: &Option<Arc<str>>| match (a, name) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || **a == **b,
            (None, None) => true,
            _ => false,
        };
        match self.apps.iter_mut().find(|a| same(&a.name)) {
            Some(a) => {
                a.rx = a.rx.saturating_add(rx);
                a.tx = a.tx.saturating_add(tx);
            }
            None => self.apps.push(AppBytes {
                name: name.clone(),
                rx,
                tx,
            }),
        }
    }

    /// Copies `other` into `self`, reusing `self`'s app buffer.
    fn copy_from(&mut self, other: &NetSlot) {
        self.start_ms = other.start_ms;
        self.measured_ns = other.measured_ns;
        self.iface_rx_bytes = other.iface_rx_bytes;
        self.iface_tx_bytes = other.iface_tx_bytes;
        self.iface_rx_pkts = other.iface_rx_pkts;
        self.iface_tx_pkts = other.iface_tx_pkts;
        self.apps.clone_from(&other.apps);
    }

    /// The store's form of the bucket.
    pub fn to_net_bucket(&self) -> NetBucket {
        NetBucket {
            measured_ms: self.measured_ms(),
            iface_rx_bytes: self.iface_rx_bytes,
            iface_tx_bytes: self.iface_tx_bytes,
            iface_rx_pkts: self.iface_rx_pkts,
            iface_tx_pkts: self.iface_tx_pkts,
            apps: self
                .apps
                .iter()
                .map(|a| NetApp {
                    name: a.name.as_deref().map(str::to_owned),
                    rx_bytes: a.rx,
                    tx_bytes: a.tx,
                })
                .collect(),
        }
    }
}

/// `bytes` times the fraction `[from, to)` of an interval `len` long, as the difference
/// of floored cumulative shares: consecutive pieces of one interval add up exactly.
fn share(bytes: u64, from: i128, to: i128, len: i128) -> u64 {
    let b = i128::from(bytes);
    let cum = |off: i128| (b * off).div_euclid(len);
    u64::try_from(cum(to) - cum(from)).unwrap_or(0)
}

/// A sample's interval in wall nanoseconds, with the part that may be counted.
struct Span {
    /// Start of the whole interval and its length: the shares' denominator.
    start: i128,
    len: i128,
    /// The countable part, `start <= lo < hi = start + len`.
    lo: i128,
    hi: i128,
}

/// The open buckets and what limits them. See the module docs.
#[derive(Default)]
pub(crate) struct NetAppAcc {
    /// Open buckets, by start.
    open: Vec<NetSlot>,
    /// Slots to reuse, so steady state does not allocate one per bucket.
    spare: Vec<NetSlot>,
    /// Nothing before this continuous time is counted (the last restart).
    from_ns: Option<u64>,
    /// Wall ms of `from_ns` when it was set: nothing lands before its 10 s bucket.
    restart_wall_ms: Option<i64>,
    /// End of the newest closed bucket: later samples that reach back before it lose
    /// that part rather than reopen a written bucket.
    closed_to_ms: Option<i64>,
    /// How far each stream has reported, wall ms.
    apps_to_ms: Option<i64>,
    iface_to_ms: Option<i64>,
}

impl NetAppAcc {
    pub fn open(&self) -> &[NetSlot] {
        &self.open
    }

    fn span(&self, anchor: Anchor, iv: Interval) -> Option<Span> {
        let start = anchor.wall_ns(iv.prev_ns);
        let hi = anchor.wall_ns(iv.now_ns);
        let len = hi - start;
        if len <= 0 {
            return None;
        }
        let mut lo = start;
        if let Some(from) = self.from_ns {
            lo = lo.max(anchor.wall_ns(from));
        }
        if let Some(closed) = self.closed_to_ms {
            lo = lo.max(i128::from(closed) * NS_PER_MS);
        }
        (lo < hi).then_some(Span { start, len, lo, hi })
    }

    /// Each bucket `span` overlaps, with the overlap's offsets from the interval's start.
    fn pieces(span: &Span, mut f: impl FnMut(i64, i128, i128)) {
        let mut b = span.lo.div_euclid(BUCKET_NS) * BUCKET_NS;
        while b < span.hi {
            let from = span.lo.max(b) - span.start;
            let to = span.hi.min(b + BUCKET_NS) - span.start;
            let start_ms = i64::try_from(b / NS_PER_MS).unwrap_or(i64::MAX);
            f(start_ms, from, to);
            b += BUCKET_NS;
        }
    }

    fn slot_mut<'a>(
        open: &'a mut Vec<NetSlot>,
        spare: &mut Vec<NetSlot>,
        start_ms: i64,
    ) -> Option<&'a mut NetSlot> {
        let i = match open.binary_search_by_key(&start_ms, |s| s.start_ms) {
            Ok(i) => i,
            Err(i) => {
                let mut slot = spare.pop().unwrap_or_default();
                slot.reset(start_ms);
                open.insert(i, slot);
                i
            }
        };
        open.get_mut(i)
    }

    /// Adds one measured per-app batch covering `iv`. Entries are keyed by identity; an
    /// exited process's entry keeps its identity, and `None` goes to "other apps".
    ///
    /// An entry's late bytes moved before `iv`, at a time no longer known, while its
    /// flow's owner was unknown. They go whole to the oldest open bucket with measured
    /// time (one this batch just measured, if no older one), never a closed one, and
    /// add no measured time. They can land up to about two buckets after they moved
    /// (the wait for a description, up to 10 s, plus the bucket boundary). They are
    /// dropped whenever the batch's span is empty: wholly before the last restart, or
    /// before the closed edge (the first 10 s or so after startup when the previous
    /// run's written bucket ends in the future, and after history or the network module
    /// comes back on within the bucket the reset wrote).
    pub fn add_apps(&mut self, anchor: Anchor, iv: Interval, entries: &[ProcessNet]) {
        let Some(span) = self.span(anchor, iv) else {
            return;
        };
        let (open, spare) = (&mut self.open, &mut self.spare);
        Self::pieces(&span, |start_ms, from, to| {
            let Some(slot) = Self::slot_mut(open, spare, start_ms) else {
                return;
            };
            slot.measured_ns = slot
                .measured_ns
                .saturating_add(u64::try_from(to - from).unwrap_or(0));
            for e in entries {
                let rx = share(e.rx_bytes, from, to, span.len);
                let tx = share(e.tx_bytes, from, to, span.len);
                slot.add_app(&e.identity, rx, tx);
            }
        });
        // This batch's span measured at least one open bucket, so this finds one.
        if let Some(oldest) = self.open.iter_mut().find(|s| s.is_measured()) {
            for e in entries {
                if e.late_rx_bytes > 0 || e.late_tx_bytes > 0 {
                    oldest.add_app(&e.identity, e.late_rx_bytes, e.late_tx_bytes);
                }
            }
        }
        let to_ms = i64::try_from(span.hi.div_euclid(NS_PER_MS)).unwrap_or(i64::MAX);
        self.apps_to_ms = Some(self.apps_to_ms.map_or(to_ms, |t| t.max(to_ms)));
    }

    /// Adds one interface-totals sample.
    pub fn add_iface(&mut self, anchor: Anchor, n: &IfaceNet) {
        let Some(span) = self.span(anchor, n.interval) else {
            return;
        };
        let (open, spare) = (&mut self.open, &mut self.spare);
        Self::pieces(&span, |start_ms, from, to| {
            let Some(slot) = Self::slot_mut(open, spare, start_ms) else {
                return;
            };
            let part = |v: u64| share(v, from, to, span.len);
            slot.iface_rx_bytes = slot.iface_rx_bytes.saturating_add(part(n.rx_bytes));
            slot.iface_tx_bytes = slot.iface_tx_bytes.saturating_add(part(n.tx_bytes));
            slot.iface_rx_pkts = slot.iface_rx_pkts.saturating_add(part(n.rx_packets));
            slot.iface_tx_pkts = slot.iface_tx_pkts.saturating_add(part(n.tx_packets));
        });
        let to_ms = i64::try_from(span.hi.div_euclid(NS_PER_MS)).unwrap_or(i64::MAX);
        self.iface_to_ms = Some(self.iface_to_ms.map_or(to_ms, |t| t.max(to_ms)));
    }

    /// Moves the buckets that are complete into `out`, oldest first: those both streams
    /// have reported past (no later sample can reach back into them), and any whose end
    /// is `grace_ms` behind `now_ms` (a stream that stopped). Return the slots with
    /// [`NetAppAcc::recycle`].
    pub fn close_due(&mut self, now_ms: i64, grace_ms: i64, out: &mut Vec<NetSlot>) {
        let covered = self.apps_to_ms.zip(self.iface_to_ms).map(|(a, i)| a.min(i));
        let due = self
            .open
            .iter()
            .take_while(|s| {
                let end = s.end_ms();
                covered.is_some_and(|c| end <= c) || end.saturating_add(grace_ms) <= now_ms
            })
            .count();
        if due == 0 {
            return;
        }
        if let Some(last) = self.open.get(due - 1) {
            self.closed_to_ms = Some(last.end_ms());
        }
        out.extend(self.open.drain(..due));
    }

    /// Takes back slots handed out by [`NetAppAcc::close_due`].
    pub fn recycle(&mut self, slots: &mut Vec<NetSlot>) {
        self.spare.append(slots);
    }

    /// Counts nothing before continuous time `ns`, wall `wall_ms`, from now on (at
    /// startup, after a wake, a resume or a reset). Open buckets stay open; what they
    /// hold was measured.
    pub fn restart_at(&mut self, wall_ms: i64, ns: u64) {
        self.from_ns = Some(ns);
        self.restart_wall_ms = Some(wall_ms);
    }

    /// Drops the open buckets as they are (the setting going off, the network module
    /// switching), keeping the edge they reached: a later sample reaching into one of
    /// them clips at its end instead of reopening a bucket that was written, whose row a
    /// second write would replace. Flush them first to keep what they hold.
    pub fn close_all(&mut self) {
        if let Some(end) = self.open.last().map(NetSlot::end_ms) {
            self.written_to(end);
        }
        self.spare.append(&mut self.open);
        self.apps_to_ms = None;
        self.iface_to_ms = None;
    }

    /// Buckets ending at or before `end_ms` are written (the store's newest row at
    /// startup, or a flush): samples count only after it.
    pub fn written_to(&mut self, end_ms: i64) {
        self.closed_to_ms = Some(self.closed_to_ms.map_or(end_ms, |c| c.max(end_ms)));
    }

    /// With no bucket open, where buckets stop being final: the end of the newest one
    /// closed or written, or with none, the start of the bucket the last restart is in
    /// (nothing is counted before the restart). `None` before any restart.
    pub fn final_to(&self) -> Option<i64> {
        self.closed_to_ms
            .or_else(|| self.restart_wall_ms.map(|w| floor_to(w, NET_BUCKET_MS)))
    }

    /// Drops the open buckets and forgets the timeline (a clock step, another store).
    /// Flush them first to keep what they hold.
    pub fn clear(&mut self) {
        self.spare.append(&mut self.open);
        self.closed_to_ms = None;
        self.restart_wall_ms = None;
        self.apps_to_ms = None;
        self.iface_to_ms = None;
    }
}

/// The hub's recent per-app buckets: the last [`NET_RING_BUCKETS`] closed ones and the
/// open ones.
///
/// Closed buckets are shared (`Arc`), so a range query copies pointers under the lock
/// and builds its [`NetBucket`]s after releasing it ([`NetRing::snapshot`]).
#[derive(Default)]
pub(crate) struct NetRing {
    closed: VecDeque<Arc<NetSlot>>,
    /// The engine's open buckets as of its last update; the first `open_len` are live,
    /// the rest keep their buffers for reuse.
    open: Vec<NetSlot>,
    open_len: usize,
    /// The engine's [`NetAppAcc::final_to`] as of its last update.
    final_to_ms: Option<i64>,
}

/// Buckets copied out of the ring under its lock, to convert after releasing it.
pub(crate) struct NetSnapshot {
    closed: Vec<Arc<NetSlot>>,
    open: Vec<NetSlot>,
    /// Where the engine's buckets stop being final: the start of the oldest one it still
    /// has open, or with none open, the end of the newest it closed or wrote (the next
    /// sample lands after it). `None` when it has neither.
    pub complete_to_ms: Option<i64>,
}

impl NetSnapshot {
    /// The measured buckets, closed then open, by start, in the store's form.
    pub fn into_buckets(self) -> Vec<(i64, NetBucket)> {
        self.closed
            .iter()
            .map(|s| &**s)
            .chain(&self.open)
            .map(|s| (s.start_ms, s.to_net_bucket()))
            .collect()
    }
}

impl NetRing {
    /// Appends closed buckets (oldest first), replaces the open ones, and records where
    /// the engine's buckets stop being final with none open.
    pub fn update(&mut self, closed: &[NetSlot], open: &[NetSlot], final_to_ms: Option<i64>) {
        self.final_to_ms = final_to_ms;
        for s in closed.iter().filter(|s| s.is_measured()) {
            // A bucket at or before the newest one replaces from there on (cannot happen
            // on one timeline; a reset clears the ring first).
            while self.closed.back().is_some_and(|b| b.start_ms >= s.start_ms) {
                self.closed.pop_back();
            }
            let mut slot = if self.closed.len() >= NET_RING_BUCKETS {
                self.closed.pop_front().unwrap_or_default()
            } else {
                Arc::default()
            };
            // Reuse the evicted slot's buffers unless a query still holds it.
            match Arc::get_mut(&mut slot) {
                Some(inner) => inner.copy_from(s),
                None => slot = Arc::new(s.clone()),
            }
            self.closed.push_back(slot);
        }
        for (i, s) in open.iter().enumerate() {
            match self.open.get_mut(i) {
                Some(slot) => slot.copy_from(s),
                None => self.open.push(s.clone()),
            }
        }
        self.open_len = open.len();
    }

    /// Forgets buckets that end after `ts_ms` (the wall clock stepped back to it), and
    /// the open ones.
    pub fn drop_after(&mut self, ts_ms: i64) {
        while self.closed.back().is_some_and(|b| b.end_ms() > ts_ms) {
            self.closed.pop_back();
        }
        self.open_len = 0;
        let floor = floor_to(ts_ms, NET_BUCKET_MS);
        self.final_to_ms = self.final_to_ms.map(|f| f.min(floor));
    }

    pub fn clear(&mut self) {
        self.closed.clear();
        self.open_len = 0;
        self.final_to_ms = None;
    }

    /// The measured buckets overlapping `[from_ms, to_ms)`, closed then open, and where
    /// the open ones start. Cheap under the lock: pointer copies for the closed buckets,
    /// clones of the few open ones. Convert with [`NetSnapshot::into_buckets`] after
    /// releasing it.
    pub fn snapshot(&self, from_ms: i64, to_ms: i64) -> NetSnapshot {
        let overlaps = |s: &NetSlot| s.is_measured() && s.start_ms < to_ms && s.end_ms() > from_ms;
        let open = self.open.get(..self.open_len).unwrap_or_default();
        NetSnapshot {
            closed: self
                .closed
                .iter()
                .filter(|s| overlaps(s))
                .cloned()
                .collect(),
            open: open.iter().filter(|s| overlaps(s)).cloned().collect(),
            complete_to_ms: open.first().map(|s| s.start_ms).or(self.final_to_ms),
        }
    }

    /// [`NetRing::snapshot`], converted.
    #[cfg(test)]
    pub fn recent(&self, from_ms: i64, to_ms: i64) -> Vec<(i64, NetBucket)> {
        self.snapshot(from_ms, to_ms).into_buckets()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const T0: i64 = 1_791_158_400_000;
    const S: u64 = 1_000_000_000;

    /// Continuous time equals wall time minus `T0`, plus 1 s so nothing is at zero.
    fn anchor_at(ns: u64) -> Anchor {
        Anchor {
            wall_ms: T0 + i64::try_from(ns / 1_000_000).unwrap() - 1_000,
            continuous_ns: ns,
        }
    }

    /// Continuous ns at wall `T0 + ms`.
    fn ns(ms: i64) -> u64 {
        u64::try_from(ms + 1_000).unwrap() * 1_000_000
    }

    fn iv(from_ms: i64, to_ms: i64) -> Interval {
        Interval {
            prev_ns: ns(from_ms),
            now_ns: ns(to_ms),
        }
    }

    fn app(pid: i32, name: Option<&Arc<str>>, rx: u64, tx: u64) -> ProcessNet {
        ProcessNet {
            pid,
            identity: name.cloned(),
            rx_bytes: rx,
            tx_bytes: tx,
            rx_bps: 0.0,
            tx_bps: 0.0,
            late_rx_bytes: 0,
            late_tx_bytes: 0,
        }
    }

    fn iface(from_ms: i64, to_ms: i64, rx: u64, tx: u64, rp: u64, tp: u64) -> IfaceNet {
        IfaceNet {
            interval: iv(from_ms, to_ms),
            rx_bytes: rx,
            tx_bytes: tx,
            rx_packets: rp,
            tx_packets: tp,
        }
    }

    fn close_all(acc: &mut NetAppAcc) -> Vec<NetSlot> {
        let mut out = Vec::new();
        acc.close_due(i64::MAX, 0, &mut out);
        out
    }

    fn rx_of(s: &NetSlot, name: Option<&str>) -> u64 {
        s.apps
            .iter()
            .find(|a| a.name.as_deref() == name)
            .map_or(0, |a| a.rx)
    }

    #[test]
    fn split_across_one_boundary_conserves_bytes() {
        let curl: Arc<str> = Arc::from("curl");
        let mut acc = NetAppAcc::default();
        // 7 s to 13 s: 3 s in the first bucket, 3 s in the second, 1,000,001 bytes.
        acc.add_apps(
            anchor_at(ns(13_000)),
            iv(7_000, 13_000),
            &[app(5, Some(&curl), 1_000_001, 7)],
        );
        let b = close_all(&mut acc);
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].start_ms, b[1].start_ms), (T0, T0 + 10_000));
        assert_eq!(
            rx_of(&b[0], Some("curl")) + rx_of(&b[1], Some("curl")),
            1_000_001
        );
        assert_eq!(b[0].apps[0].tx + b[1].apps[0].tx, 7);
        assert_eq!(rx_of(&b[0], Some("curl")), 500_000);
        assert_eq!((b[0].measured_ms(), b[1].measured_ms()), (3_000, 3_000));
    }

    #[test]
    fn split_across_several_buckets_conserves_bytes() {
        let a: Arc<str> = Arc::from("a");
        let mut acc = NetAppAcc::default();
        // 60 s at a 60 s base tick: from 5 s to 65 s, seven buckets touched.
        for bytes in [1u64, 7, 999_999_937, u64::MAX / 4] {
            let mut acc2 = NetAppAcc::default();
            acc2.add_apps(
                anchor_at(ns(65_000)),
                iv(5_000, 65_000),
                &[app(1, Some(&a), bytes, bytes / 3)],
            );
            let b = close_all(&mut acc2);
            assert_eq!(b.len(), 7);
            let rx: u64 = b.iter().map(|s| rx_of(s, Some("a"))).sum();
            let tx: u64 = b.iter().flat_map(|s| &s.apps).map(|x| x.tx).sum();
            assert_eq!((rx, tx), (bytes, bytes / 3), "exact for {bytes}");
            let measured: u32 = b.iter().map(NetSlot::measured_ms).sum();
            assert_eq!(measured, 60_000);
        }
        acc.add_apps(anchor_at(ns(1)), iv(0, 0), &[app(1, Some(&a), 5, 5)]);
        assert!(acc.open().is_empty(), "an empty interval adds nothing");
    }

    #[test]
    fn apps_and_interface_align_per_bucket_across_phases() {
        let a: Arc<str> = Arc::from("a");
        let mut acc = NetAppAcc::default();
        // Apps every 10 s from 3 s, interface every 10 s from 8 s: different phases, the
        // same steady 1,000 B/s on both.
        for k in 0..6 {
            let (af, at) = (3_000 + k * 10_000, 13_000 + k * 10_000);
            acc.add_apps(
                anchor_at(ns(at)),
                iv(af, at),
                &[app(1, Some(&a), 10_000, 0)],
            );
            let (f, t) = (8_000 + k * 10_000, 18_000 + k * 10_000);
            acc.add_iface(anchor_at(ns(t)), &iface(f, t, 10_000, 0, 10, 0));
        }
        let mut out = Vec::new();
        // Apps reported to 63 s, the interface to 68 s: the buckets up to 60 s are
        // complete, and those from 10 s on are whole for both streams.
        acc.close_due(T0 + 68_000, 30_000, &mut out);
        let whole: Vec<_> = out
            .iter()
            .filter(|s| s.start_ms >= T0 + 10_000)
            .map(|s| {
                (
                    rx_of(s, Some("a")),
                    s.iface_rx_bytes,
                    s.iface_rx_pkts,
                    s.measured_ms(),
                )
            })
            .collect();
        assert_eq!(whole, vec![(10_000, 10_000, 10, 10_000); 5]);
        assert_eq!(out.last().map(|s| s.start_ms), Some(T0 + 50_000));
        assert_eq!(acc.open().first().map(|s| s.start_ms), Some(T0 + 60_000));
    }

    #[test]
    fn identities_key_the_bytes_and_none_is_other_apps() {
        let git: Arc<str> = Arc::from("git");
        let git2: Arc<str> = Arc::from("git");
        let mut acc = NetAppAcc::default();
        acc.add_apps(
            anchor_at(ns(9_000)),
            iv(1_000, 9_000),
            &[
                // An exited process keeps its entry under its old pid and identity.
                app(41, Some(&git), 100, 1),
                app(42, Some(&git2), 50, 2),
                app(43, None, 7, 0),
                app(44, None, 3, 0),
                app(45, Some(&git), 0, 0),
            ],
        );
        let b = close_all(&mut acc);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].apps.len(), 2, "{:?}", b[0].apps);
        assert_eq!(rx_of(&b[0], Some("git")), 150);
        assert_eq!(rx_of(&b[0], None), 10);
        let nb = b[0].to_net_bucket();
        assert_eq!(nb.measured_ms, 8_000);
        assert!(nb.apps.iter().any(|a| a.name.is_none() && a.rx_bytes == 10));
    }

    /// Bytes charged once an app was identified moved at an unknown earlier time: they
    /// go whole to the oldest open bucket, never reopen a closed one, and add no
    /// measured time.
    #[test]
    fn late_bytes_go_to_the_oldest_open_bucket() {
        let curl: Arc<str> = Arc::from("curl");
        let mut acc = NetAppAcc::default();
        let mut out = Vec::new();
        // The bucket at 0 s closes (both streams past it); 10 s and 20 s stay open.
        acc.add_apps(anchor_at(ns(10_000)), iv(0, 10_000), &[]);
        acc.add_iface(anchor_at(ns(10_000)), &iface(0, 10_000, 0, 0, 0, 0));
        acc.close_due(T0 + 10_000, 30_000, &mut out);
        assert_eq!(out.len(), 1);
        acc.add_iface(anchor_at(ns(25_000)), &iface(10_000, 25_000, 0, 0, 0, 0));
        acc.add_apps(anchor_at(ns(15_000)), iv(10_000, 15_000), &[]);
        let late = ProcessNet {
            late_rx_bytes: 30_000_000,
            late_tx_bytes: 500,
            ..app(5, Some(&curl), 2_000, 20)
        };
        acc.add_apps(anchor_at(ns(25_000)), iv(15_000, 25_000), &[late]);
        let b = close_all(&mut acc);
        assert_eq!(out[0].start_ms, T0);
        assert_eq!(
            rx_of(&out[0], Some("curl")),
            0,
            "the closed bucket stays closed"
        );
        assert_eq!((b[0].start_ms, b[1].start_ms), (T0 + 10_000, T0 + 20_000));
        assert_eq!(rx_of(&b[0], Some("curl")), 30_000_000 + 1_000);
        assert_eq!(rx_of(&b[1], Some("curl")), 1_000);
        assert_eq!(b[0].apps[0].tx, 500 + 10);
        assert_eq!(
            (b[0].measured_ms(), b[1].measured_ms()),
            (10_000, 5_000),
            "no measured time for late bytes"
        );
    }

    /// An older open bucket the interface alone has reported into has no measured
    /// time and is never written: late bytes skip it.
    #[test]
    fn late_bytes_skip_an_open_bucket_without_measured_time() {
        let curl: Arc<str> = Arc::from("curl");
        let mut acc = NetAppAcc::default();
        acc.add_iface(anchor_at(ns(10_000)), &iface(5_000, 10_000, 100, 0, 0, 0));
        let late = ProcessNet {
            late_rx_bytes: 9_000,
            ..app(5, Some(&curl), 1_000, 0)
        };
        acc.add_apps(anchor_at(ns(15_000)), iv(10_000, 15_000), &[late]);
        let b = close_all(&mut acc);
        assert_eq!((b[0].start_ms, b[0].measured_ms()), (T0, 0));
        assert_eq!(rx_of(&b[0], Some("curl")), 0);
        assert_eq!(rx_of(&b[1], Some("curl")), 10_000);
    }

    #[test]
    fn a_stopped_stream_closes_by_grace_and_late_samples_do_not_reopen() {
        let a: Arc<str> = Arc::from("a");
        let mut acc = NetAppAcc::default();
        acc.add_apps(
            anchor_at(ns(10_000)),
            iv(0, 10_000),
            &[app(1, Some(&a), 100, 0)],
        );
        let mut out = Vec::new();
        // No interface stream: not covered, so only the grace closes it.
        acc.close_due(T0 + 39_999, 30_000, &mut out);
        assert!(out.is_empty());
        acc.close_due(T0 + 40_000, 30_000, &mut out);
        assert_eq!(out.len(), 1);
        acc.recycle(&mut out);
        // A late interface sample over 5 s to 15 s: only the half after the close counts.
        acc.add_iface(anchor_at(ns(41_000)), &iface(5_000, 15_000, 1_000, 0, 0, 0));
        assert_eq!(acc.open().len(), 1);
        assert_eq!(acc.open()[0].start_ms, T0 + 10_000);
        assert_eq!(acc.open()[0].iface_rx_bytes, 500);
    }

    #[test]
    fn restart_clips_and_clear_forgets() {
        let a: Arc<str> = Arc::from("a");
        let mut acc = NetAppAcc::default();
        acc.add_apps(
            anchor_at(ns(4_000)),
            iv(0, 4_000),
            &[app(1, Some(&a), 400, 0)],
        );
        // Paused from 4 s, resumed at 7 s: a sample reaching back to 2 s counts from 7 s.
        acc.restart_at(T0 + 7_000, ns(7_000));
        acc.add_apps(
            anchor_at(ns(9_000)),
            iv(2_000, 9_000),
            &[app(1, Some(&a), 700, 0)],
        );
        let b = close_all(&mut acc);
        assert_eq!(
            b[0].measured_ms(),
            6_000,
            "4 s before, 2 s after: not the pause"
        );
        assert_eq!(rx_of(&b[0], Some("a")), 400 + 200);
        acc.recycle(&mut b.clone());

        acc.add_apps(
            anchor_at(ns(14_000)),
            iv(10_000, 14_000),
            &[app(1, Some(&a), 1, 0)],
        );
        acc.clear();
        assert!(acc.open().is_empty());
        assert!(close_all(&mut acc).is_empty());
    }

    #[test]
    fn close_all_keeps_the_written_edge_and_clear_forgets_it() {
        let a: Arc<str> = Arc::from("a");
        let mut acc = NetAppAcc::default();
        acc.add_apps(
            anchor_at(ns(13_000)),
            iv(10_000, 13_000),
            &[app(1, Some(&a), 300, 0)],
        );
        // Written and dropped at 13 s; a sample from 15 s reaches into the same bucket.
        acc.close_all();
        acc.restart_at(T0 + 15_000, ns(15_000));
        acc.add_apps(
            anchor_at(ns(25_000)),
            iv(15_000, 25_000),
            &[app(1, Some(&a), 1_000, 0)],
        );
        let b = close_all(&mut acc);
        let shape: Vec<_> = b
            .iter()
            .map(|s| (s.start_ms - T0, s.measured_ms()))
            .collect();
        assert_eq!(
            shape,
            [(20_000, 5_000)],
            "nothing reopens the bucket at 10 s"
        );
        assert_eq!(rx_of(&b[0], Some("a")), 500);

        // A store row ending at 30 s (the previous run's) clips the same way.
        let mut acc = NetAppAcc::default();
        acc.written_to(T0 + 30_000);
        acc.add_apps(
            anchor_at(ns(35_000)),
            iv(25_000, 35_000),
            &[app(1, Some(&a), 1_000, 0)],
        );
        let b = close_all(&mut acc);
        assert_eq!(b.len(), 1);
        assert_eq!((b[0].start_ms, b[0].measured_ms()), (T0 + 30_000, 5_000));
        // A clock step forgets it: the new timeline may reuse those buckets.
        acc.clear();
        acc.add_apps(
            anchor_at(ns(25_000)),
            iv(20_000, 25_000),
            &[app(1, Some(&a), 1, 0)],
        );
        assert_eq!(acc.open().first().map(|s| s.start_ms), Some(T0 + 20_000));
    }

    #[test]
    fn a_snapshot_keeps_its_buckets_through_eviction() {
        let mut ring = NetRing::default();
        let slot = |k: i64| NetSlot {
            start_ms: T0 + k * NET_BUCKET_MS,
            measured_ns: 10 * S,
            iface_rx_bytes: u64::try_from(k).unwrap(),
            ..NetSlot::default()
        };
        let all: Vec<NetSlot> = (0..NET_RING_BUCKETS as i64).map(slot).collect();
        ring.update(&all, &[], None);
        let held = ring.snapshot(T0, T0 + NET_BUCKET_MS);
        // Evicts the bucket the snapshot holds, then one more whose buffers are reused.
        ring.update(&[slot(360), slot(361)], &[], None);
        assert_eq!(held.into_buckets(), [(T0, slot(0).to_net_bucket())]);
        let tail = ring.recent(T0 + 360 * NET_BUCKET_MS, i64::MAX);
        assert_eq!(tail.len(), 2);
        assert_eq!(tail[1].1.iface_rx_bytes, 361);
    }

    #[test]
    fn ring_holds_an_hour_and_shows_the_open_buckets() {
        let mut ring = NetRing::default();
        let slot = |k: i64| NetSlot {
            start_ms: T0 + k * NET_BUCKET_MS,
            measured_ns: 10 * S,
            iface_rx_bytes: u64::try_from(k).unwrap(),
            ..NetSlot::default()
        };
        let closed: Vec<NetSlot> = (0..400).map(slot).collect();
        for c in closed.chunks(7) {
            ring.update(c, &[], None);
        }
        let all = ring.recent(i64::MIN, i64::MAX);
        assert_eq!(all.len(), NET_RING_BUCKETS);
        assert_eq!(all[0].0, T0 + 40 * NET_BUCKET_MS, "the oldest 40 evicted");

        let open = NetSlot {
            measured_ns: 2 * S,
            ..slot(400)
        };
        let unmeasured = slot(401);
        let unmeasured = NetSlot {
            measured_ns: 0,
            ..unmeasured
        };
        assert_eq!(ring.snapshot(0, 1).complete_to_ms, None, "no edge yet");
        ring.update(&[], &[], Some(T0 + 400 * NET_BUCKET_MS));
        assert_eq!(
            ring.snapshot(0, 1).complete_to_ms,
            Some(T0 + 400 * NET_BUCKET_MS),
            "nothing open: the engine's closed edge"
        );
        ring.update(&[], &[open, unmeasured], Some(T0 + 399 * NET_BUCKET_MS));
        assert_eq!(
            ring.snapshot(0, 1).complete_to_ms,
            Some(T0 + 400 * NET_BUCKET_MS),
            "the oldest open bucket, even outside the range asked for"
        );
        let tail = ring.recent(T0 + 399 * NET_BUCKET_MS + 1, i64::MAX);
        let shape: Vec<_> = tail.iter().map(|(t, b)| (*t, b.measured_ms)).collect();
        assert_eq!(
            shape,
            vec![
                (T0 + 399 * NET_BUCKET_MS, 10_000),
                (T0 + 400 * NET_BUCKET_MS, 2_000)
            ]
        );

        ring.drop_after(T0 + 395 * NET_BUCKET_MS);
        assert_eq!(
            ring.recent(i64::MIN, i64::MAX).last().map(|b| b.0),
            Some(T0 + 394 * NET_BUCKET_MS)
        );
        ring.clear();
        assert!(ring.recent(i64::MIN, i64::MAX).is_empty());
    }
}
