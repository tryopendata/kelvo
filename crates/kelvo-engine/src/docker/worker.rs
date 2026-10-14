//! Connection, retry, idle and connected-session lifecycle.
use super::*;
impl DockerHandle {
    pub async fn run(self) {
        let mut controls = self.control.subscribe();
        let mut retry = 1;
        loop {
            let c = controls.borrow_and_update().clone();
            if c.shutdown {
                break;
            }
            if !c.settings.enabled || c.paused {
                if !self.transition(
                    c.generation,
                    if c.paused {
                        DockerStatus::Paused
                    } else {
                        DockerStatus::Disabled
                    },
                    None,
                    if c.paused { "paused" } else { "disabled" },
                ) {
                    continue;
                }
                if controls.changed().await.is_err() {
                    break;
                }
                continue;
            }
            let generation = c.generation;
            let result = tokio::select! {biased;_=controls.changed()=>continue,result=self.connect_selected(&c.settings)=>result};
            let adapter = match result {
                Ok(a) => a,
                Err(e) => {
                    let status = match e {
                        DockerError::Incompatible => DockerStatus::Incompatible,
                        DockerError::PermissionDenied => DockerStatus::PermissionDenied,
                        _ => DockerStatus::Unavailable,
                    };
                    if !self.transition(generation, status, Some(e.to_string()), "disconnected") {
                        continue;
                    }
                    self.wait_retry(&mut controls, generation, retry).await;
                    retry = (retry * 2).min(30);
                    continue;
                }
            };
            if !self.attach_adapter(&adapter, generation) {
                continue;
            }
            let idle_adapter = adapter.clone();
            let session = self
                .session(adapter, generation, &mut controls, &mut retry)
                .await;
            self.cancel();
            if session == SessionEnd::Disconnected {
                if !self.transition(
                    generation,
                    DockerStatus::Unavailable,
                    Some("Docker stream disconnected; retrying".into()),
                    "disconnected",
                ) {
                    continue;
                }
                self.wait_retry(&mut controls, generation, retry).await;
                retry = (retry * 2).min(30);
            }
            if session == SessionEnd::Idle {
                if !self.transition(
                    generation,
                    DockerStatus::IdleSuspended,
                    Some("No running containers. Monitoring paused to save power.".into()),
                    "idle_suspended",
                ) {
                    continue;
                }
                if !self
                    .wait_idle(&idle_adapter, generation, &mut controls)
                    .await
                {
                    return;
                }
            }
        }
    }
    async fn wait_retry(
        &self,
        controls: &mut watch::Receiver<Control>,
        generation: u32,
        seconds: u64,
    ) {
        let delay = tokio::time::sleep(Duration::from_secs(seconds));
        tokio::pin!(delay);
        loop {
            tokio::select! {biased;
                changed = controls.changed() => {
                    if changed.is_err() || controls.borrow().generation != generation || controls.borrow().shutdown { return; }
                }
                _ = &mut delay => return,
            }
        }
    }
    async fn connect_selected(&self, s: &DockerSettings) -> Result<DockerAdapter, DockerError> {
        let list = self.discover_candidates().await;
        let chosen = if let Some(socket) = &s.selected_engine {
            list.into_iter()
                .find(|e| e.socket == *socket)
                .or_else(|| {
                    Some(DockerEngine {
                        id: socket.clone(),
                        socket: socket.clone(),
                        runtime: "Local Docker (unverified)".into(),
                        context: None,
                        daemon_id: None,
                        reachable: false,
                        preferred: false,
                        error: None,
                        api_version: None,
                        capacity_bytes: None,
                        capacity_observed_ms: None,
                    })
                })
                .ok_or(DockerError::Unavailable)?
        } else {
            let found = self.discover().await;
            found
                .iter()
                .find(|e| e.preferred && e.reachable)
                .cloned()
                .or_else(|| {
                    let mut reachable = found.into_iter().filter(|e| e.reachable);
                    let only = reachable.next()?;
                    reachable.next().is_none().then_some(only)
                })
                .ok_or(DockerError::Unavailable)?
        };
        DockerAdapter::connect(chosen).await
    }
    pub(super) async fn discover_candidates(&self) -> Vec<DockerEngine> {
        let home = self.home.clone();
        tokio::task::spawn_blocking(move || candidates(&home))
            .await
            .unwrap_or_default()
    }
    async fn session(
        &self,
        mut adapter: DockerAdapter,
        generation: u32,
        controls: &mut watch::Receiver<Control>,
        retry: &mut u64,
    ) -> SessionEnd {
        let mut healthy_since = None;
        let mut inventory = Inventory::new(self.snapshot().population_revision);
        let mut samples = Samples::new();
        let since = self.event_since(&adapter.engine.id).await;
        // Docker retains only 256 replay events: reconnect cannot prove complete coverage.
        self.gap_for(generation, "event_replay_uncertain");
        let mut events = Box::pin(adapter.events_since(since));
        let mut event_live = true;
        let mut refresh = true;
        let mut event_rate = (Instant::now(), 0usize);
        let mut restarts: BTreeMap<String, VecDeque<i64>> = BTreeMap::new();
        let mut clock = (now_ms(), Instant::now());
        let mut last_observation: Option<Instant> = None;
        loop {
            let control = controls.borrow_and_update().clone();
            if control.generation != generation || control.shutdown {
                return SessionEnd::Changed;
            }
            let interval =
                control
                    .min_interval_ms
                    .max(if control.visible > 0 { 1000 } else { 10000 });
            self.check_clock(&mut clock, &mut samples, generation);
            let cycle_start = Instant::now();
            let mut event_refresh = false;
            let ok = {
                let cycle = self.cycle(
                    &mut adapter,
                    &mut inventory,
                    &mut samples,
                    refresh,
                    generation,
                );
                tokio::pin!(cycle);
                loop {
                    tokio::select! {biased;
                        _=controls.changed()=>{if controls.borrow().generation!=generation{return SessionEnd::Changed;}},
                        result=&mut cycle=>break result,
                        event=events.next(),if event_live=>{match event{Some(Ok(e))=>{if admit_event(&mut event_rate){self.record_event(e,&mut restarts,generation);event_refresh=true;}else{event_live=false;self.gap_for(generation,"events_dropped_rate_limit");}},_=>{event_live=false;self.gap_for(generation,"events_disconnected");}}}
                    }
                }
            };
            refresh = event_refresh;
            if ok && event_live {
                if healthy_since.get_or_insert_with(Instant::now).elapsed()
                    >= Duration::from_secs(60)
                {
                    *retry = 1;
                }
            } else {
                healthy_since = None;
            }
            if !ok {
                self.gap_for(generation, "partial_cycle");
            }
            if inventory.rows.is_empty()
                && inventory.refreshed_at.elapsed() > Duration::from_secs(30)
            {
                return SessionEnd::Disconnected;
            }
            self.publish_cycle(
                &adapter,
                &control,
                &mut inventory,
                &mut restarts,
                &mut last_observation,
                ok,
            );
            if !inventory
                .rows
                .iter()
                .any(|c| c.lifecycle == DockerLifecycle::Running)
            {
                return SessionEnd::Idle;
            }
            let sleep = tokio::time::sleep(
                Duration::from_millis(interval as u64).saturating_sub(cycle_start.elapsed()),
            );
            tokio::pin!(sleep);
            loop {
                tokio::select! {biased;
                    _=controls.changed()=>{if controls.borrow().generation!=generation{return SessionEnd::Changed;}break;},
                    event=events.next(),if event_live=>{match event{Some(Ok(e))=>{if admit_event(&mut event_rate){self.record_event(e,&mut restarts,generation);}else{event_live=false;self.gap_for(generation,"events_dropped_rate_limit");}refresh=true;},_=>{self.gap_for(generation,"events_disconnected");event_live=false;}}},
                    _=&mut sleep=>break
                }
            }
            if !event_live {
                return SessionEnd::Disconnected;
            }
        }
    }
}

impl DockerHandle {
    fn attach_adapter(&self, adapter: &DockerAdapter, generation: u32) -> bool {
        let _gate = self.gate.lock_ok();
        if self.control.borrow().generation != generation {
            return false;
        }
        self.inner.lock_ok().adapter = Some(adapter.clone());
        self.changed.send_modify(|s| {
            s.now_ms = now_ms();
            if s.engine.as_ref().is_none_or(|e| e.id != adapter.engine.id) {
                s.coverage.clear();
                s.containers.clear();
                s.project_aggregates.clear();
                s.aggregate = DockerAggregate {
                    monitoring_limit: 128,
                    ..Default::default()
                };
            }
            s.engine = Some(adapter.engine.clone());
            s.generation = generation;
        });

        true
    }

    async fn wait_idle(
        &self,
        idle_adapter: &DockerAdapter,
        generation: u32,
        controls: &mut watch::Receiver<Control>,
    ) -> bool {
        let mut visible = controls.borrow().visible;
        let mut probe = tokio::time::interval_at(
            Instant::now() + Duration::from_secs(60),
            Duration::from_secs(60),
        );
        probe.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        'idle: loop {
            tokio::select! { biased;
                changed = controls.changed() => {
                    if changed.is_err() { return false; }
                    let next = controls.borrow_and_update().clone();
                    if next.generation != generation || next.shutdown
                        || next.visible > visible {
                        break;
                    }
                    visible = next.visible;
                }
                _ = probe.tick() => {
                    let result = tokio::select! { biased;
                        changed = controls.changed() => {
                            if changed.is_err() { return false; }
                            let next = controls.borrow_and_update().clone();
                            if next.generation != generation || next.shutdown
                                || next.visible > visible {
                                break 'idle;
                            }
                            visible = next.visible;
                            continue 'idle;
                        }
                        result = idle_adapter.has_running_containers() => result,
                    };
                    // Errors return to normal connection/backoff handling.
                    if !matches!(result, Ok(false)) { break; }
                    self.transition(
                        generation, DockerStatus::IdleSuspended,
                        Some("No running containers. Monitoring paused to save power.".into()),
                        "idle_suspended",
                    );
                }
            }
        }
        true
    }
}

impl DockerHandle {
    async fn event_since(&self, engine: &str) -> Option<String> {
        let live_since = self
            .inner
            .lock_ok()
            .events
            .iter()
            .rev()
            .find(|e| e.engine_id == engine)
            .map(|e| {
                format!(
                    "{}.{:09}",
                    e.time_ms / 1000,
                    e.time_ns.parse::<i64>().unwrap_or(0) % 1_000_000_000
                )
            });
        let recorder = Arc::clone(&self.recorder);
        let daemon = engine.to_owned();
        let durable = tokio::task::spawn_blocking(move || recorder.event_cursor(&daemon))
            .await
            .ok()
            .flatten();
        live_since.or_else(|| {
            durable
                .and_then(|s| s.parse::<i64>().ok())
                .map(|ns| format!("{}.{:09}", ns / 1_000_000_000, ns % 1_000_000_000))
        })
    }

    fn check_clock(&self, clock: &mut (i64, Instant), samples: &mut Samples, generation: u32) {
        let wall = now_ms();
        let mono = Instant::now();
        if wall
            .saturating_sub(clock.0)
            .saturating_sub(mono.duration_since(clock.1).as_millis() as i64)
            .abs()
            > 2000
        {
            samples.baseline.clear();
            {
                let _gate = self.gate.lock_ok();
                if self.control.borrow().generation == generation {
                    self.recorder.clock_step(wall);
                    self.gap("clock_step");
                }
            }
        }
        *clock = (wall, mono);
    }
}
