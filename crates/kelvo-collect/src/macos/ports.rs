//! The TCP ports each process listens on, filled into the processes collector's rows.
//!
//! On demand ([`Interest::PortProcesses`]): it runs only on ticks the processes collector
//! sampled while a visible window shows ports, and comes after it in the slot order, so
//! the rows it fills are this tick's. A full read is one `PROC_PIDLISTFDS` per process plus
//! one call per socket, about 2 ms for every readable process on the dev machine, so each
//! process's ports are re-read every [`PORTS_EVERY_NS`] rather than every tick. It reads
//! the processes the rows cover, so it sees the user's own processes only (D-045).

use std::collections::HashMap;
use std::sync::Arc;

use kelvo_schema::Entitlement;

use super::libproc;
use crate::{Cadence, CollectError, Collector, CollectorId, Interest, Probe, SampleBuf, Tick};

/// How often one process's ports are read again.
const PORTS_EVERY_NS: u64 = 5_000_000_000;

/// A process's ports and when they were read, keyed by `(pid, start time)`.
type Read = HashMap<(i32, i64), (Arc<[u16]>, u64)>;

/// Reads ports once per [`PORTS_EVERY_NS`] per process.
#[derive(Default)]
struct PortCache {
    known: Read,
    seen: Read,
}

impl PortCache {
    /// The ports of `key`, read through `read` when none are cached or they are older than
    /// [`PORTS_EVERY_NS`]. Processes not asked about between two [`PortCache::end`] calls
    /// are forgotten.
    fn get(&mut self, key: (i32, i64), now: u64, read: impl FnOnce() -> Arc<[u16]>) -> Arc<[u16]> {
        let entry = match self.known.remove(&key) {
            Some((ports, at)) if now.saturating_sub(at) < PORTS_EVERY_NS => (ports, at),
            _ => (read(), now),
        };
        let ports = entry.0.clone();
        self.seen.insert(key, entry);
        ports
    }

    fn end(&mut self) {
        std::mem::swap(&mut self.known, &mut self.seen);
        self.seen.clear();
    }
}

pub struct ProcessPorts {
    cache: PortCache,
    fds: Vec<libc::proc_fdinfo>,
    ports: Vec<u16>,
    /// Shared by every process that listens on nothing, which is almost all of them.
    none: Arc<[u16]>,
}

impl Default for ProcessPorts {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessPorts {
    pub fn new() -> Self {
        Self {
            cache: PortCache::default(),
            fds: Vec::new(),
            ports: Vec::new(),
            none: Arc::from([]),
        }
    }
}

impl Collector for ProcessPorts {
    fn id(&self) -> CollectorId {
        CollectorId("process.ports")
    }

    fn cadence(&self) -> Cadence {
        Cadence::OnDemand(Interest::PortProcesses)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn probe(&mut self) -> Probe {
        // Process rows are not series; the reads are the processes collector's libproc.
        self.cache = PortCache::default();
        Probe::Supported(Vec::new())
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let now = tick.continuous_ns;
        let Self {
            cache,
            fds,
            ports,
            none,
        } = self;
        for row in out.processes_mut() {
            let found = cache.get((row.pid, row.start_time_us), now, || {
                libproc::listening_ports(row.pid, fds, ports);
                if ports.is_empty() {
                    none.clone()
                } else {
                    Arc::from(ports.as_slice())
                }
            });
            row.ports = Some(found);
        }
        cache.end();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_are_read_every_five_seconds_per_process() {
        let mut c = PortCache::default();
        let s = 1_000_000_000;
        let mut reads = 0;
        let mut get = |c: &mut PortCache, key, now, p: u16| {
            c.get(key, now, || {
                reads += 1;
                Arc::from([p])
            })[0]
        };
        assert_eq!(get(&mut c, (1, 10), 0, 80), 80);
        c.end();
        assert_eq!(get(&mut c, (1, 10), 4 * s, 81), 80, "cached");
        assert_eq!(get(&mut c, (1, 11), 4 * s, 82), 82, "a reused pid is read");
        c.end();
        assert_eq!(get(&mut c, (1, 10), 5 * s, 83), 83, "read again");
        c.end();
        // (1, 11) was not asked about, so it was forgotten.
        assert_eq!(get(&mut c, (1, 11), 6 * s, 84), 84);
        assert_eq!(reads, 4);
    }
}
