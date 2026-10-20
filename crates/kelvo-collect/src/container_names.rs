//! Container names by id, shared between an async refresher and the collectors that run
//! on the engine's std thread (D-116). The Linux cgroup and process collectors see only
//! container ids (`docker-<id>.scope`); a task with Docker socket access (the agent's)
//! fills this map, and they read it.
//!
//! A map nobody fills ([`ContainerNames::default`], as in `kelvo-engine`'s `dump` or the
//! app) is *unattached*: every lookup misses at once and callers fall back to short ids.
//! An [`ContainerNames::attached`] map asks its refresher for a pass when a collector
//! meets an unknown id, and a caller that wants a name, not a short id, waits until a
//! refresh has finished after it asked ([`ContainerNames::settled_since`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use kelvo_schema::lock::RwLockExt;

#[derive(Debug, Default)]
struct Inner {
    /// Full container id to name.
    map: RwLock<HashMap<String, Arc<str>>>,
    attached: bool,
    /// Finished refreshes, successful or not.
    generation: AtomicU64,
    /// A collector met an id the map does not have.
    wanted: AtomicBool,
}

/// A cheap-to-clone handle to one shared map.
#[derive(Clone, Debug, Default)]
pub struct ContainerNames(Arc<Inner>);

impl ContainerNames {
    /// A map a refresher fills: lookups of unknown ids wait for its next pass.
    pub fn attached() -> Self {
        Self(Arc::new(Inner {
            attached: true,
            ..Inner::default()
        }))
    }

    /// The name of the container with full id `id`.
    pub fn get(&self, id: &str) -> Option<Arc<str>> {
        self.0.map.read_ok().get(id).cloned()
    }

    /// Finished refreshes so far. A caller that misses records this and asks
    /// [`ContainerNames::settled_since`] later.
    pub fn generation(&self) -> u64 {
        self.0.generation.load(Ordering::Acquire)
    }

    /// Whether a miss recorded at `generation` is final: nobody fills the map, or a
    /// refresh finished since (and the id was still unknown to it).
    pub fn settled_since(&self, generation: u64) -> bool {
        !self.0.attached || self.generation() > generation
    }

    /// Asks the refresher for a pass (a collector met an unknown id). Cheap; repeats
    /// before the pass collapse into one.
    pub fn request(&self) {
        if self.0.attached {
            self.0.wanted.store(true, Ordering::Release);
        }
    }

    /// Whether a pass was asked for since the last [`ContainerNames::finish`].
    pub fn requested(&self) -> bool {
        self.0.wanted.load(Ordering::Acquire)
    }

    /// Ends a refresh: `Some` replaces the map (the containers running now), `None` is
    /// a pass that could not list them (no socket, permission denied), which leaves the
    /// map as it was. Either way waiting callers are settled.
    pub fn finish(&self, names: Option<HashMap<String, Arc<str>>>) {
        self.0.wanted.store(false, Ordering::Release);
        if let Some(names) = names {
            *self.0.map.write_ok() = names;
        }
        self.0.generation.fetch_add(1, Ordering::AcqRel);
    }
}

/// Fills a [`ContainerNames`] from the local Docker daemon. Async, for a task on the
/// caller's runtime; the policy of when to call [`NameRefresher::refresh`] is the
/// caller's.
#[cfg(unix)]
pub struct NameRefresher {
    names: ContainerNames,
    home: std::path::PathBuf,
    adapter: Option<crate::docker::DockerAdapter>,
}

#[cfg(unix)]
impl NameRefresher {
    /// Docker sockets are looked for as the app does ([`crate::docker::candidates`]),
    /// under `home` and at `/var/run/docker.sock`.
    pub fn new(names: ContainerNames, home: std::path::PathBuf) -> Self {
        Self {
            names,
            home,
            adapter: None,
        }
    }

    pub fn names(&self) -> &ContainerNames {
        &self.names
    }

    /// One pass: connects on first use and after a failure, lists the running
    /// containers and replaces the map. A failure leaves the map as it was; either way
    /// the pass settles collectors waiting on it. The number of names on success.
    pub async fn refresh(&mut self) -> Result<usize, crate::docker::DockerError> {
        let result = self.list().await;
        match result {
            Ok(names) => {
                let n = names.len();
                self.names.finish(Some(names));
                Ok(n)
            }
            Err(e) => {
                self.adapter = None;
                self.names.finish(None);
                Err(e)
            }
        }
    }

    async fn list(&mut self) -> Result<HashMap<String, Arc<str>>, crate::docker::DockerError> {
        if self.adapter.is_none() {
            let mut last = crate::docker::DockerError::Unavailable;
            for engine in crate::docker::candidates(&self.home) {
                match crate::docker::DockerAdapter::connect(engine).await {
                    Ok(adapter) => {
                        self.adapter = Some(adapter);
                        break;
                    }
                    Err(e) => last = e,
                }
            }
            if self.adapter.is_none() {
                return Err(last);
            }
        }
        match &self.adapter {
            Some(adapter) => adapter.running_names().await,
            None => Err(crate::docker::DockerError::Unavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unattached_maps_settle_at_once_and_ignore_requests() {
        let names = ContainerNames::default();
        assert_eq!(names.get("abc"), None);
        assert!(names.settled_since(names.generation()));
        names.request();
        assert!(!names.requested());
    }

    #[test]
    fn attached_misses_wait_for_the_next_refresh() {
        let names = ContainerNames::attached();
        let asked_at = names.generation();
        assert!(!names.settled_since(asked_at));
        names.request();
        assert!(names.requested());
        let shared = names.clone();
        shared.finish(Some(HashMap::from([(
            "abc".to_owned(),
            Arc::from("nginx"),
        )])));
        assert!(!names.requested());
        assert!(names.settled_since(asked_at));
        assert_eq!(names.get("abc").as_deref(), Some("nginx"));
        // A failed pass settles callers and keeps the last good map.
        let asked_at = names.generation();
        names.finish(None);
        assert!(names.settled_since(asked_at));
        assert_eq!(names.get("abc").as_deref(), Some("nginx"));
    }
}
