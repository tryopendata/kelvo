//! Window demand, subscription ownership and source replacement.
use super::*;
use std::sync::MutexGuard;
use tokio::task::JoinHandle;

struct StreamSlot {
    id: u32,
    task: JoinHandle<()>,
    procs: Arc<Mutex<Option<ProcessView>>>,
}

struct ProcInterest {
    view: ProcessView,
    /// The stream (page load) it belongs to; `None` from a caller that does not say,
    /// which then counts whatever stream the window has.
    token: Option<u32>,
}

/// One window's state for one host.
struct HostSlot {
    feed: Arc<dyn LiveFeed>,
    stream: Option<StreamSlot>,
    /// The sequence number of the newest `subscribe` for this slot. Only that one
    /// installs its stream; an older one still setting up when a newer one started
    /// aborts its own.
    latest: u64,
    procs: Option<ProcInterest>,
    /// Counted in the engine's detail interest.
    detail_applied: bool,
}

impl HostSlot {
    fn new(feed: Arc<dyn LiveFeed>) -> Self {
        Self {
            feed,
            stream: None,
            latest: 0,
            procs: None,
            detail_applied: false,
        }
    }

    /// The process view in effect: interest whose token matches the current stream.
    fn active_procs(&self) -> Option<&ProcessView> {
        let p = self.procs.as_ref()?;
        match (p.token, &self.stream) {
            (None, _) => Some(&p.view),
            (Some(t), Some(s)) if s.id == t => Some(&p.view),
            _ => None,
        }
    }

    /// Hands the stream its process view and applies detail interest for `visible`.
    fn sync(&mut self, visible: bool) {
        if let Some(s) = &self.stream {
            *s.procs.lock_ok() = self.active_procs().cloned();
        }
        let detail = visible && self.stream.is_some();
        if detail != self.detail_applied {
            self.detail_applied = detail;
        }
    }

    fn release(self) {
        if let Some(s) = self.stream {
            s.task.abort();
        }
    }
}

struct WindowEntry {
    visible: watch::Sender<bool>,
    hosts: HashMap<HostId, HostSlot>,
    visible_now: bool,
}

impl WindowEntry {
    fn new() -> Self {
        // Hidden until the window code says otherwise (D-066): a window it forgot to
        // report must not stream unseen.
        Self {
            visible: watch::Sender::new(false),
            hosts: HashMap::new(),
            visible_now: false,
        }
    }

    fn is_visible(&self) -> bool {
        self.visible_now
    }
}

#[derive(Default)]
struct Inner {
    windows: HashMap<String, WindowEntry>,
    /// The process period and network, GPU and ports flags last given to each host.
    applied: HashMap<HostId, (Option<u32>, bool, bool, bool, bool)>,
    /// Registration owns source identity; None rejects calls racing host removal.
    registered: HashMap<HostId, Option<Arc<dyn LiveFeed>>>,
    /// Numbers `subscribe` calls, registry-wide, so a slot recreated after its window
    /// closed never matches a call that started before.
    next_seq: u64,
}

impl Inner {
    fn window(&mut self, label: &str) -> &mut WindowEntry {
        self.windows
            .entry(label.to_owned())
            .or_insert_with(WindowEntry::new)
    }

    /// Tells `feed`'s host the shortest process period any visible window wants, and
    /// whether any of those windows shows network rates or GPU time.
    fn apply_process_interest(&mut self, feed: &Arc<dyn LiveFeed>) {
        let host = feed.host();
        let mut period = None;
        let mut network = false;
        let mut gpu = false;
        let mut ports = false;
        for v in self
            .windows
            .values()
            .filter(|w| w.is_visible())
            .filter_map(|w| w.hosts.get(&host)?.active_procs())
        {
            let p = v.period_ms.unwrap_or(0);
            period = Some(period.map_or(p, |q: u32| q.min(p)));
            network |= v.network;
            gpu |= v.gpu;
            ports |= v.ports;
        }
        let detail = self
            .windows
            .values()
            .any(|w| w.is_visible() && w.hosts.get(&host).is_some_and(|s| s.detail_applied));
        let applied = self
            .applied
            .entry(host)
            .or_insert((None, false, false, false, false));
        if applied.0 != period {
            applied.0 = period;
            feed.set_process_interest(period);
        }
        if applied.1 != network {
            applied.1 = network;
            feed.set_network_process_interest(network);
        }
        if applied.2 != gpu {
            applied.2 = gpu;
            feed.set_gpu_process_interest(gpu);
        }
        if applied.3 != ports {
            applied.3 = ports;
            feed.set_port_process_interest(ports);
        }
        if applied.4 != detail {
            applied.4 = detail;
            feed.set_detail_interest(detail);
        }
    }
}

/// Every live channel and process interest, by window label.
#[derive(Default)]
pub struct LiveRegistry {
    inner: Mutex<Inner>,
}

impl LiveRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock_ok()
    }

    /// Starts (or restarts, after a page reload) the stream for `label` and the feed's
    /// host. If the window is visible, `Caps`, `Status`, the layouts and the recent
    /// backfill are sent before this returns; frames and older history follow from a
    /// task. Process interest tagged with another stream (the page before a reload) ends
    /// here. Must run inside the tokio runtime.
    pub fn subscribe(
        &self,
        label: &str,
        feed: Arc<dyn LiveFeed>,
        sink: Arc<dyn LiveSink>,
        req: LiveRequest,
    ) -> Option<SubscriptionInfo> {
        let host = feed.host();
        let id = sink.id();
        let mut info = SubscriptionInfo {
            host,
            stream: id,
            backfill_start_ms: None,
            backfill_rows: 0,
            earlier_start_ms: None,
            earlier_rows: 0,
        };
        let procs = Arc::new(Mutex::new(None));
        let (seq, mut visible) = {
            let mut inner = self.lock();
            if inner.registered.get(&host).is_some_and(|current| {
                current
                    .as_ref()
                    .is_none_or(|current| !Arc::ptr_eq(current, &feed))
            }) {
                return None;
            }
            inner.next_seq += 1;
            let seq = inner.next_seq;
            let w = inner.window(label);
            let vis = w.is_visible();
            let slot = w
                .hosts
                .entry(host)
                .or_insert_with(|| HostSlot::new(Arc::clone(&feed)));
            slot.latest = seq;
            // A visible window's first status must carry the visible tick, not the
            // background's (D-094): its detail interest goes in before the catch-up.
            if vis && !slot.detail_applied {
                slot.detail_applied = true;
            }
            if let Some(old) = slot.stream.take() {
                old.task.abort();
            }
            if slot
                .procs
                .as_ref()
                .is_some_and(|p| p.token.is_some_and(|t| t != id))
            {
                slot.procs = None;
            }
            let visible = w.visible.subscribe();
            inner.apply_process_interest(&feed);
            (seq, visible)
        };
        let mut stream = Stream::new(
            Arc::clone(&feed),
            sink.clone(),
            req.clone(),
            Arc::clone(&procs),
        );
        let mut sub = None;
        if *visible.borrow_and_update() {
            // Subscribe before reading the ring, so nothing published in between is lost;
            // frames the backfill already covered are skipped by timestamp.
            let s = feed.hub().subscribe();
            match stream.catch_up() {
                Ok(c) => {
                    info.backfill_start_ms = c.first;
                    info.backfill_rows = c.rows;
                    info.earlier_start_ms = c.earlier_start;
                    info.earlier_rows = c.earlier_rows;
                }
                Err(Closed) => {
                    self.cancel_setup(label, host, seq, &feed);
                    return None;
                }
            }
            sub = Some(s);
        }
        let task = tokio::spawn(stream.run(visible, sub));
        let mut inner = self.lock();
        let current = inner.windows.get_mut(label).and_then(|w| {
            let vis = w.is_visible();
            w.hosts
                .get_mut(&host)
                .filter(|slot| slot.latest == seq)
                .map(|slot| (slot, vis))
        });
        match current {
            Some((slot, vis)) => {
                if let Some(old) = slot.stream.replace(StreamSlot { id, task, procs }) {
                    old.task.abort();
                }
                slot.sync(vis);
            }
            // A newer subscribe for this window and host started while this one was
            // setting up (React StrictMode subscribes twice), or the window closed.
            None => {
                task.abort();
                tracing::debug!(%host, label, stream = id, "live channel superseded");
                return None;
            }
        }
        inner.apply_process_interest(&feed);
        tracing::debug!(%host, label, stream = id, "live channel subscribed");
        Some(info)
    }

    fn cancel_setup(&self, label: &str, host: HostId, seq: u64, feed: &Arc<dyn LiveFeed>) {
        let mut inner = self.lock();
        if let Some(w) = inner.windows.get_mut(label)
            && w.hosts.get(&host).is_some_and(|slot| slot.latest == seq)
        {
            if let Some(slot) = w.hosts.remove(&host) {
                slot.release();
            }
            inner.apply_process_interest(feed);
        }
    }

    /// Sets (`Some`) or withdraws (`None`) the process rows `label` wants for the feed's
    /// host. `token` is the stream id from `subscribe_live`: the interest counts only
    /// while that stream is the window's current one, so a reloaded page's interest ends
    /// with it. Without a token it counts for whatever stream the window has.
    pub fn set_process_interest(
        &self,
        label: &str,
        feed: Arc<dyn LiveFeed>,
        view: Option<ProcessView>,
        token: Option<u32>,
    ) {
        let host = feed.host();
        let mut inner = self.lock();
        if inner.registered.get(&host).is_some_and(|current| {
            current
                .as_ref()
                .is_none_or(|current| !Arc::ptr_eq(current, &feed))
        }) {
            return;
        }
        let w = inner.window(label);
        let vis = w.is_visible();
        let slot = w
            .hosts
            .entry(host)
            .or_insert_with(|| HostSlot::new(Arc::clone(&feed)));
        slot.procs = view.map(|view| ProcInterest { view, token });
        slot.sync(vis);
        inner.apply_process_interest(&feed);
    }

    /// The window was shown (`true`) or hidden, minimized or occluded (`false`). Streams
    /// stop receiving while hidden and resume with a backfill of the missed span; process
    /// and detail interest leave the hosts' counts while hidden and return on show.
    ///
    /// On show, detail interest goes in before the streams resume, and the engine has
    /// published the visible tick by then (D-094), so a resumed stream's status never
    /// carries the background's 2 s. On hide the streams stop first.
    pub fn window_visible(&self, label: &str, visible: bool) {
        let mut inner = self.lock();
        let w = inner.window(label);
        w.visible_now = visible;
        let send = |w: &WindowEntry| {
            w.visible.send_if_modified(|v| {
                let changed = *v != visible;
                *v = visible;
                changed
            });
        };
        if !visible {
            send(w);
        }
        let mut feeds = Vec::with_capacity(w.hosts.len());
        for slot in w.hosts.values_mut() {
            slot.sync(visible);
            feeds.push(Arc::clone(&slot.feed));
        }
        for f in &feeds {
            inner.apply_process_interest(f);
        }
        if visible && let Some(w) = inner.windows.get(label) {
            send(w);
        }
        tracing::debug!(label, visible, "window visibility");
    }

    /// The window is gone: drop its streams and its interest.
    pub fn window_closed(&self, label: &str) {
        let mut inner = self.lock();
        let Some(w) = inner.windows.remove(label) else {
            return;
        };
        let mut feeds = Vec::with_capacity(w.hosts.len());
        for (_, slot) in w.hosts {
            feeds.push(Arc::clone(&slot.feed));
            slot.release();
        }
        for f in &feeds {
            inner.apply_process_interest(f);
        }
        tracing::debug!(label, "window closed; live channels dropped");
    }

    /// Release all channels and demand before removing or replacing a source.
    pub fn remove_host(&self, host: HostId) {
        self.replace_host_slots(host, None);
    }

    /// Register the source instance that subsequent subscriptions must use.
    /// Existing channels reconnect through the frontend's normal retry/backfill path.
    pub fn replace_host(&self, feed: Arc<dyn LiveFeed>) {
        self.replace_host_slots(feed.host(), Some(feed));
    }

    fn replace_host_slots(&self, host: HostId, feed: Option<Arc<dyn LiveFeed>>) {
        let mut inner = self.lock();
        let mut old = inner.registered.insert(host, feed).flatten();
        for window in inner.windows.values_mut() {
            if let Some(slot) = window.hosts.remove(&host) {
                old = Some(slot.feed.clone());
                slot.release();
            }
        }
        if let Some(old) = old {
            inner.apply_process_interest(&old);
        }
        inner.applied.remove(&host);
    }

    /// Whether `label` currently counts as visible. Unknown labels do not.
    pub fn is_visible(&self, label: &str) -> bool {
        self.lock()
            .windows
            .get(label)
            .is_some_and(WindowEntry::is_visible)
    }
}
