use kelvo_collect::docker::DockerCounters;
use kelvo_schema::docker::DockerMetrics;

pub fn normalize(
    now: &DockerCounters,
    previous: Option<&DockerCounters>,
    elapsed_ms: u32,
) -> DockerMetrics {
    let rate = |value: Option<u64>, prev: Option<u64>| -> Option<f64> {
        let delta = value?.checked_sub(prev?)?;
        (elapsed_ms > 0).then_some(delta as f64 * 1000.0 / elapsed_ms as f64)
    };
    let cpu_pct = previous.and_then(|old| {
        let cpu = now.cpu?.checked_sub(old.cpu?)?;
        let system = now.system?.checked_sub(old.system?)?;
        let cpus = now.online_cpus?;
        if system == 0 || cpus == 0 || elapsed_ms == 0 {
            return None;
        }
        Some(cpu as f64 / system as f64 * cpus as f64 * 100.0)
    });
    DockerMetrics {
        cpu_pct,
        working_set_bytes: now
            .raw_memory
            .zip(now.inactive_file)
            .map(|(usage, cache)| usage.saturating_sub(cache) as f64),
        raw_memory_bytes: now.raw_memory.map(|v| v as f64),
        effective_limit_bytes: now.effective_limit.map(|v| v as f64),
        network_rx_bps: rate(now.rx, previous.and_then(|s| s.rx)),
        network_tx_bps: rate(now.tx, previous.and_then(|s| s.tx)),
        block_read_bps: rate(now.read, previous.and_then(|s| s.read)),
        block_write_bps: rate(now.write, previous.and_then(|s| s.write)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counters_preserve_unknown_reset_and_multicore_semantics() {
        let old = DockerCounters {
            cpu: Some(100),
            system: Some(1000),
            rx: Some(10),
            ..Default::default()
        };
        let next = DockerCounters {
            cpu: Some(400),
            system: Some(2000),
            online_cpus: Some(8),
            rx: Some(2010),
            raw_memory: Some(100),
            inactive_file: Some(120),
            ..Default::default()
        };
        let first = normalize(&next, None, 1000);
        assert_eq!(first.cpu_pct, None);
        assert_eq!(first.network_rx_bps, None);
        assert_eq!(first.working_set_bytes, Some(0.));
        let second = normalize(&next, Some(&old), 2000);
        assert_eq!(second.cpu_pct, Some(240.));
        assert_eq!(second.network_rx_bps, Some(1000.));
        assert_eq!(normalize(&old, Some(&next), 1000).cpu_pct, None);
        assert_eq!(normalize(&old, None, 1000).working_set_bytes, None);
        assert_eq!(normalize(&next, Some(&old), 0).network_rx_bps, None);
    }
}
