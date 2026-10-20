//! Docker container names for the cgroup and process collectors (D-116). The collectors
//! run on the engine's std thread and see only container ids; this task lists the
//! running containers through the read-only Docker adapter and fills the shared map
//! they read.
//!
//! A pass runs at start, then when a collector met an id the map does not have, at most
//! every [`MIN_GAP`]. Renames of a running container are not followed (a recreated
//! container has a new id, so it is). Without socket access every pass fails, the
//! collectors stop waiting and use short ids.
use std::path::PathBuf;
use std::time::Duration;

use kelvo_engine::{ContainerNames, NameRefresher};
use tokio::time::Instant;

/// Passes run at most this often.
const MIN_GAP: Duration = Duration::from_secs(30);
/// How often the task looks for a request. The cgroup collector samples every 5 s.
const POLL: Duration = Duration::from_secs(5);
/// How long the daemon waits for the first pass before starting the engine, so the
/// first layout already has names.
pub const FIRST_PASS_WITHIN: Duration = Duration::from_secs(3);

/// Whether a pass is due: the first one, or one asked for once [`MIN_GAP`] has passed.
fn due(last: Option<Instant>, requested: bool, now: Instant) -> bool {
    match last {
        None => true,
        Some(at) => requested && now.duration_since(at) >= MIN_GAP,
    }
}

/// Starts the task on the current runtime and returns the map it fills.
pub fn spawn() -> ContainerNames {
    let names = ContainerNames::attached();
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    tokio::spawn(run(NameRefresher::new(names.clone(), home)));
    names
}

async fn run(mut refresher: NameRefresher) {
    let mut last = None;
    let mut reachable = None;
    loop {
        let now = Instant::now();
        if due(last, refresher.names().requested(), now) {
            last = Some(now);
            match refresher.refresh().await {
                Ok(n) => {
                    if reachable != Some(true) {
                        tracing::info!(containers = n, "Docker container names available");
                    }
                    reachable = Some(true);
                }
                Err(e) => {
                    if reachable != Some(false) {
                        tracing::info!(error = %e, detail = ?e, "no Docker container names; using short ids");
                    }
                    reachable = Some(false);
                }
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

/// Waits until the first pass finished, or `within`.
pub async fn first_pass(names: &ContainerNames, within: Duration) {
    let deadline = Instant::now() + within;
    while names.generation() == 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_run_first_then_on_request_at_most_every_30_s() {
        let t0 = Instant::now();
        assert!(due(None, false, t0), "the first pass");
        assert!(!due(Some(t0), false, t0 + MIN_GAP * 2), "nothing asked");
        assert!(!due(Some(t0), true, t0 + MIN_GAP / 2), "too soon");
        assert!(due(Some(t0), true, t0 + MIN_GAP));
    }

    #[tokio::test]
    async fn first_pass_waits_for_a_finished_refresh_or_gives_up() {
        let names = ContainerNames::attached();
        let started = Instant::now();
        first_pass(&names, Duration::from_millis(60)).await;
        assert!(started.elapsed() >= Duration::from_millis(60));
        names.finish(None);
        let started = Instant::now();
        first_pass(&names, Duration::from_secs(5)).await;
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
