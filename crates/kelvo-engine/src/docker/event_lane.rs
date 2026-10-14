//! Bounded event admission and observed restart evidence.
use super::*;
impl DockerHandle {
    pub(super) fn record_event(
        &self,
        event: DockerEvent,
        restarts: &mut BTreeMap<String, VecDeque<i64>>,
        generation: u32,
    ) {
        let _gate = self.gate.lock_ok();
        if generation != self.control.borrow().generation {
            return;
        }
        let cutoff = now_ms() - 300_000;
        restarts.retain(|_, q| {
            q.retain(|t| *t >= cutoff);
            !q.is_empty()
        });
        let mut state = self.inner.lock_ok();
        if state.events.iter().any(|e| e.id == event.id) {
            return;
        }
        let mut dropped = false;
        let restarted = event.action == "restart"
            || (event.action == "start"
                && state
                    .events
                    .iter()
                    .rev()
                    .find(|e| {
                        e.engine_id == event.engine_id
                            && e.container_id == event.container_id
                            && matches!(e.action.as_str(), "start" | "die" | "stop" | "restart")
                    })
                    .is_some_and(|e| {
                        matches!(e.action.as_str(), "die" | "stop")
                            && event.time_ms.saturating_sub(e.time_ms) <= 300_000
                    }));
        if restarted && let Some(id) = &event.container_id {
            if restarts.contains_key(id) || restarts.len() < 4096 {
                let q = restarts.entry(id.clone()).or_default();
                if event.action != "restart"
                    || q.back()
                        .is_none_or(|last| event.time_ms.saturating_sub(*last) > 2000)
                {
                    q.push_back(event.time_ms);
                }
                while q.len() > 128 {
                    q.pop_front();
                }
            } else {
                dropped = true;
            }
        }
        state.events.push_back(event.clone());
        while state.events.len() > 4096 {
            state.events.pop_front();
        }
        drop(state);
        if dropped {
            self.gap("restart_evidence_dropped");
        }
        self.recorder.event(&event, generation);
        let _ = self.events.send(event);
    }
}
pub(super) fn admit_event(rate: &mut (Instant, usize)) -> bool {
    if rate.0.elapsed() >= Duration::from_secs(1) {
        *rate = (Instant::now(), 0);
    }
    rate.1 += 1;
    rate.1 <= 1000
}
