//! Accounting, no FFI: per-flow counts into per-pid bytes ([`Ledger`]), app identities
//! ([`Names`]), and the failure [`Breaker`].

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use super::{BACKOFF, FAIL_LIMIT};

/// What one counts callback says about a flow.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// `0` when the framework has not described the flow yet: a flow that existed
    /// before the manager reports its owner only after a description query.
    pub pid: i32,
    /// The owner's `uniqueProcessID` (never reused within a boot); `0` when unknown.
    pub upid: u64,
    /// Cumulative bytes received over the flow's life.
    pub rx: u64,
    pub tx: u64,
    /// The flow is on an interface whose bytes are in the interface totals.
    pub counted: bool,
}

/// An owner whose app identity is not known yet: the callback that learned it resolves
/// one ([`Ledger::identify`]) while the dictionary and, usually, the process are alive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub pid: i32,
    pub upid: u64,
}

impl Owner {
    /// The identity cache key: the unique id, or the pid (tagged so it cannot collide
    /// with a unique id) on a release whose dictionaries lack one.
    fn key(self) -> u64 {
        if self.upid != 0 {
            self.upid
        } else {
            (1 << 63) | u64::from(self.pid.unsigned_abs())
        }
    }
}

/// An interned identity's address, so maps key on it without hashing the string: one
/// ledger interns each name once, so equal names share one allocation. `0` for none.
fn ident_addr(ident: &Option<Arc<str>>) -> usize {
    ident
        .as_ref()
        .map_or(0, |a| Arc::as_ptr(a).cast::<u8>() as usize)
}

#[derive(Debug)]
struct Flow {
    /// Owner, from a counts or description dictionary; `0` until one names it.
    pid: i32,
    /// The owner's unique id, when a dictionary gave one.
    upid: u64,
    /// The owner's app identity, once looked for.
    ident: Option<Arc<str>>,
    /// An identity was looked for (found or not), so later callbacks do not look again.
    identified: bool,
    /// Latest cumulative `(rx, tx)`.
    bytes: Option<(u64, u64)>,
    /// As [`Counts::counted`], from the latest counts.
    counted: bool,
    /// The `(rx, tx)` already accounted for, `None` before the first settle that saw it.
    base: Option<(u64, u64)>,
    /// Added after the baseline: the flow began after a settle that accounted for
    /// everything, so its whole count is new. A flow from before the baseline that
    /// reports late starts from its first count instead.
    fresh: bool,
    /// `(rx, tx)` a fresh flow moved before its owner was known, owed to the owner's
    /// byte totals at the next settle but not to its rate (see [`Flow::own`]).
    late: (u64, u64),
}

impl Flow {
    fn new(fresh: bool) -> Self {
        Flow {
            pid: 0,
            upid: 0,
            ident: None,
            identified: false,
            bytes: None,
            counted: false,
            base: None,
            fresh,
            late: (0, 0),
        }
    }

    /// The pid and the bytes not yet accounted for, with the identity, marking them
    /// accounted. `None` before the first counts and for flows not counted. A flow whose
    /// owner is still unknown keeps its bytes unaccounted until a description names it.
    fn take(&mut self) -> Option<(i32, Settled)> {
        let (rx, tx) = self.bytes?;
        if self.pid <= 0 {
            return None;
        }
        let (rx0, tx0) = self
            .base
            .unwrap_or(if self.fresh { (0, 0) } else { (rx, tx) });
        self.base = Some((rx, tx));
        let (late_rx, late_tx) = std::mem::take(&mut self.late);
        self.counted.then(|| {
            (
                self.pid,
                Settled {
                    ident: self.ident.clone(),
                    rx: rx.saturating_sub(rx0),
                    tx: tx.saturating_sub(tx0),
                    late_rx,
                    late_tx,
                },
            )
        })
    }

    fn unresolved(&self) -> bool {
        self.pid <= 0 && self.bytes.is_some()
    }

    /// Records the owner. When a flow that waited with bytes and no owner is attributed
    /// after the baseline, its rate starts from the bytes seen so far: charging the
    /// whole wait (up to [`DESCRIBE_EVERY_MS`](super::DESCRIBE_EVERY_MS)) to one sample would show a spike many
    /// times the real rate (D-082). A fresh flow's wait still goes to the owner's byte
    /// totals, as `late` (D-089: history keeps it); a flow from before the baseline
    /// drops it, since those bytes may predate the baseline. A different owner than
    /// before needs its identity looked up.
    fn own(&mut self, pid: i32, upid: u64, baselined: bool) {
        if pid <= 0 {
            return;
        }
        if baselined && self.unresolved() {
            if self.fresh
                && let Some((rx, tx)) = self.bytes
            {
                let (rx0, tx0) = self.base.unwrap_or((0, 0));
                self.late = (
                    self.late.0.saturating_add(rx.saturating_sub(rx0)),
                    self.late.1.saturating_add(tx.saturating_sub(tx0)),
                );
            }
            self.base = self.bytes;
        }
        if pid != self.pid || (upid != 0 && upid != self.upid) {
            self.ident = None;
            self.identified = false;
        }
        self.pid = pid;
        if upid != 0 {
            self.upid = upid;
        }
    }

    fn owner(&self) -> Owner {
        Owner {
            pid: self.pid,
            upid: self.upid,
        }
    }
}

/// The app identities a ledger knows: one interned copy of each name and the name of
/// each process seen, by unique id. Names arrive from the framework's callbacks; the
/// settle path only clones the `Arc`s, so it never allocates a string.
#[derive(Debug, Default)]
pub(super) struct Names {
    pub(super) by_owner: HashMap<u64, Arc<str>>,
    pub(super) interned: HashSet<Arc<str>>,
    /// `by_owner` is pruned once it grows past this.
    pub(super) prune_at: usize,
    /// Start of the current hour of [`NEW_NAMES_PER_HOUR`].
    window: Option<Instant>,
    new_in_window: u32,
}

/// Distinct new names a ledger takes per hour; past it, new processes go unnamed
/// ("other apps"), so processes that rewrite their argv cannot grow the name tables
/// (and the store's interned names) without bound.
pub(super) const NEW_NAMES_PER_HOUR: u32 = 128;
pub(super) const HOUR: Duration = Duration::from_secs(3_600);

impl Names {
    pub(super) fn with_room() -> Self {
        Self {
            by_owner: HashMap::with_capacity(PID_ROOM),
            interned: HashSet::with_capacity(PID_ROOM),
            prune_at: PID_ROOM,
            ..Self::default()
        }
    }

    fn intern(&mut self, name: &str, now: Instant) -> Option<Arc<str>> {
        if let Some(a) = self.interned.get(name) {
            return Some(Arc::clone(a));
        }
        if self
            .window
            .is_none_or(|w| now.saturating_duration_since(w) >= HOUR)
        {
            self.window = Some(now);
            self.new_in_window = 0;
        }
        if self.new_in_window >= NEW_NAMES_PER_HOUR {
            return None;
        }
        self.new_in_window += 1;
        let a: Arc<str> = Arc::from(name);
        self.interned.insert(Arc::clone(&a));
        Some(a)
    }
}

/// Per-flow counts as the callbacks deliver them, and the bytes of closed flows not yet
/// handed out. Flows are keyed by the source pointer, which the framework reuses after a
/// removal, so a removal drops the entry before a new source can take the key.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    flows: HashMap<usize, Flow>,
    /// Bytes of flows removed since the last settle, per pid and identity (by
    /// [`ident_addr`]): a process that exits keeps its name for its last bytes.
    closed: HashMap<(i32, usize), Settled>,
    pub(super) names: Names,
    baselined: bool,
}

/// One pid's and identity's bytes since the last settle.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Settled {
    pub ident: Option<Arc<str>>,
    /// Bytes moved over the settle interval: the rate.
    pub rx: u64,
    pub tx: u64,
    /// Bytes fresh flows moved earlier, before their owner was known: in the byte
    /// totals, not the rate.
    pub late_rx: u64,
    pub late_tx: u64,
}

/// What [`Ledger::settle`] fills: [`Settled`] by pid and identity address.
pub(crate) type SettleMap = HashMap<(i32, usize), Settled>;

fn add(map: &mut SettleMap, pid: i32, s: Settled) {
    let e = map
        .entry((pid, ident_addr(&s.ident)))
        .or_insert_with(|| Settled {
            ident: s.ident.clone(),
            ..Settled::default()
        });
    e.rx = e.rx.saturating_add(s.rx);
    e.tx = e.tx.saturating_add(s.tx);
    e.late_rx = e.late_rx.saturating_add(s.late_rx);
    e.late_tx = e.late_tx.saturating_add(s.late_tx);
}

/// Room for this many flows and pids before a map grows. Flows come and go every
/// second (about 130 open on the dev Mac), so a map sized to the first sample would
/// regrow, and allocate, whenever churn crossed its capacity.
const FLOW_ROOM: usize = 1024;
pub(super) const PID_ROOM: usize = 256;

impl Ledger {
    /// A ledger with room for [`FLOW_ROOM`] flows, allocated once when the manager opens.
    pub(super) fn with_room() -> Self {
        Self {
            flows: HashMap::with_capacity(FLOW_ROOM),
            closed: HashMap::with_capacity(PID_ROOM),
            names: Names::with_room(),
            baselined: false,
        }
    }

    pub(crate) fn added(&mut self, src: usize) {
        self.flows.insert(src, Flow::new(self.baselined));
    }

    fn flow(&mut self, src: usize) -> &mut Flow {
        let fresh = self.baselined;
        self.flows.entry(src).or_insert_with(|| Flow::new(fresh))
    }

    /// The flow's owner when it still needs an identity, after taking a cached one.
    fn needs_identity(&mut self, src: usize) -> Option<Owner> {
        let f = self.flows.get_mut(&src)?;
        if f.pid <= 0 || f.identified {
            return None;
        }
        if let Some(a) = self.names.by_owner.get(&f.owner().key()) {
            f.ident = Some(Arc::clone(a));
            f.identified = true;
            return None;
        }
        Some(f.owner())
    }

    /// A description named the flow's owner. Returns the owner when its identity is
    /// not known yet: resolve it, then call [`Ledger::identify`].
    pub(crate) fn described(&mut self, src: usize, pid: i32, upid: u64) -> Option<Owner> {
        let baselined = self.baselined;
        self.flow(src).own(pid, upid, baselined);
        self.needs_identity(src)
    }

    /// New counts for a flow. Returns the owner when its identity is not known yet, as
    /// [`Ledger::described`].
    pub(crate) fn counts(&mut self, src: usize, c: Counts) -> Option<Owner> {
        let baselined = self.baselined;
        let f = self.flow(src);
        // Before the new bytes: what moved since the last counts is still this owner's.
        f.own(c.pid, c.upid, baselined);
        f.bytes = Some((c.rx, c.tx));
        f.counted = c.counted;
        self.needs_identity(src)
    }

    /// Records `owner`'s identity (`None`: nothing named it) for the flow and for the
    /// owner's later flows. `now` paces the cap on new names.
    pub(crate) fn identify(&mut self, src: usize, owner: Owner, name: Option<&str>, now: Instant) {
        let ident = name.and_then(|n| self.names.intern(n, now));
        if let Some(a) = &ident {
            self.names.by_owner.insert(owner.key(), Arc::clone(a));
        }
        if let Some(f) = self.flows.get_mut(&src)
            && f.owner() == owner
        {
            f.ident = ident;
            f.identified = true;
        }
    }

    pub(crate) fn removed(&mut self, src: usize) {
        let Some(mut flow) = self.flows.remove(&src) else {
            return;
        };
        if !self.baselined {
            return;
        }
        if let Some((pid, s)) = flow.take() {
            add(&mut self.closed, pid, s);
        }
    }

    /// Flows with bytes but no known owner: a description query would name them.
    pub(crate) fn unresolved(&self) -> usize {
        self.flows.values().filter(|f| f.unresolved()).count()
    }

    /// Fills `out` with the bytes each pid and identity moved since the last settle,
    /// closed flows included, and marks them accounted. The first call sets the
    /// baseline: it returns `false` and `out` stays empty.
    pub(crate) fn settle(&mut self, out: &mut SettleMap) -> bool {
        out.clear();
        self.prune_names();
        if !self.baselined {
            for f in self.flows.values_mut() {
                // History, owner known or not: never a rate.
                if f.base.is_none() {
                    f.base = f.bytes;
                }
            }
            self.closed.clear();
            self.baselined = true;
            return false;
        }
        for f in self.flows.values_mut() {
            if let Some((pid, s)) = f.take() {
                add(out, pid, s);
            }
        }
        for ((pid, _), s) in self.closed.drain() {
            add(out, pid, s);
        }
        true
    }

    /// Forgets the names of processes no open flow belongs to, once there are more
    /// than [`Names::prune_at`], and then names nothing refers to. Without allocating:
    /// a scan of the open flows per cached process, which runs rarely because the
    /// threshold doubles past what is still in use.
    fn prune_names(&mut self) {
        if self.names.by_owner.len() <= self.names.prune_at {
            return;
        }
        let flows = &self.flows;
        self.names
            .by_owner
            .retain(|&k, _| flows.values().any(|f| f.pid > 0 && f.owner().key() == k));
        // A name only the intern table holds is used by no flow, closed entry or row.
        self.names.interned.retain(|a| Arc::strong_count(a) > 1);
        self.names.prune_at = PID_ROOM.max(self.names.by_owner.len() * 2);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.flows.len()
    }
}

/// Stops reopening a manager that keeps failing: after [`FAIL_LIMIT`] failures in a row,
/// nothing is tried until [`BACKOFF`] has passed on the continuous clock. The count is
/// reset only by a sample that worked, so after a backoff one more failure starts the
/// next one: a framework that stays stuck costs one attempt per backoff.
#[derive(Debug, Default)]
pub(super) struct Breaker {
    failures: u32,
    until_ns: Option<u64>,
}

impl Breaker {
    /// Whether a manager may be opened at `now_ns`.
    pub(super) fn allows(&self, now_ns: u64) -> bool {
        self.until_ns.is_none_or(|until| now_ns >= until)
    }

    pub(super) fn failed(&mut self, now_ns: u64) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= FAIL_LIMIT {
            let backoff = u64::try_from(BACKOFF.as_nanos()).unwrap_or(u64::MAX);
            self.until_ns = Some(now_ns.saturating_add(backoff));
        }
    }

    pub(super) fn succeeded(&mut self) {
        self.failures = 0;
        self.until_ns = None;
    }
}

pub(super) fn relock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // The ledger holds plain counters updated in single steps; a panic elsewhere while
    // holding it leaves it usable.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
