//! Durable interval usage facts (D-120). Recorder inputs precede any floor or ranking.
//! Resource arrays consistently use CPU, GPU, RAM, CPU energy, network order.

use std::sync::Arc;

use kelvo_schema::durable_usage::{UsageIncompleteCause, UsageNetworkUncertainty};

pub const RESOURCE_COUNT: usize = 5;
pub const USAGE_BUCKET_MS: i64 = 10_000;

/// Source observation for one resource. A full duration does not imply completeness.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsageSourceCoverage {
    pub supported: bool,
    pub measured_ms: u64,
    /// Union of [`cause_mask`] values; zero only when the source can prove complete.
    pub causes: u32,
}

/// Mergeable observations; no display averages, holds or inferred absent zeros.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsageFacts {
    pub cpu_seconds: f64,
    pub gpu_seconds: f64,
    pub energy_j: f64,
    /// For apps, maximum of the processes summed at the same source sample.
    pub mem_peak_bytes: u64,
    pub mem_byte_ms: f64,
    pub present_ms: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub coverage: [UsageSourceCoverage; RESOURCE_COUNT],
}

impl UsageFacts {
    /// Fold disjoint time intervals. Callers compute concurrent app RAM before this.
    pub fn merge(&mut self, other: &Self) {
        self.cpu_seconds += other.cpu_seconds;
        self.gpu_seconds += other.gpu_seconds;
        self.energy_j += other.energy_j;
        self.mem_peak_bytes = self.mem_peak_bytes.max(other.mem_peak_bytes);
        self.mem_byte_ms += other.mem_byte_ms;
        self.present_ms = self.present_ms.saturating_add(other.present_ms);
        self.network_rx_bytes = self.network_rx_bytes.saturating_add(other.network_rx_bytes);
        self.network_tx_bytes = self.network_tx_bytes.saturating_add(other.network_tx_bytes);
        for (a, b) in self.coverage.iter_mut().zip(other.coverage) {
            a.supported |= b.supported;
            a.measured_ms = a.measured_ms.saturating_add(b.measured_ms);
            a.causes |= b.causes;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageAppFacts {
    pub name: Arc<str>,
    pub facts: UsageFacts,
}

/// Original process-instance identity, retained independently from app totals.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageProcessFacts {
    pub app_name: Arc<str>,
    pub pid: i32,
    pub start_time_us: i64,
    pub name: Arc<str>,
    pub facts: UsageFacts,
}

/// One finest-resolution host bucket. Partial observation is represented by coverage,
/// never by manufacturing a complete ten seconds. Source facts include every observed
/// identity; queries, not recording, select the ten leaders.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageBucket {
    pub from_ms: i64,
    pub to_ms: i64,
    pub source: [UsageSourceCoverage; RESOURCE_COUNT],
    pub apps: Vec<UsageAppFacts>,
    pub processes: Vec<UsageProcessFacts>,
    /// Raw uncertainty records, normally records=1. Exact totals exclude these bytes.
    pub uncertain_network: Vec<UsageNetworkUncertainty>,
}

pub const fn cause_mask(cause: UsageIncompleteCause) -> u32 {
    1 << match cause {
        UsageIncompleteCause::NotRecorded => 0,
        UsageIncompleteCause::Pruned => 1,
        UsageIncompleteCause::SamplingGap => 2,
        UsageIncompleteCause::ReadFailed => 3,
        UsageIncompleteCause::NoBaseline => 4,
        UsageIncompleteCause::UnreadableProcess => 5,
        UsageIncompleteCause::UnresolvedIdentity => 6,
        UsageIncompleteCause::IdentityLimit => 7,
        UsageIncompleteCause::UncertainNetworkTime => 8,
        UsageIncompleteCause::PartialBoundary => 9,
        UsageIncompleteCause::ResourceLimit => 10,
    }
}

pub fn causes(mask: u32) -> Vec<UsageIncompleteCause> {
    use UsageIncompleteCause::*;
    [
        NotRecorded,
        Pruned,
        SamplingGap,
        ReadFailed,
        NoBaseline,
        UnreadableProcess,
        UnresolvedIdentity,
        IdentityLimit,
        UncertainNetworkTime,
        PartialBoundary,
        ResourceLimit,
    ]
    .into_iter()
    .filter(|cause| mask & cause_mask(*cause) != 0)
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollups_keep_integrals_and_concurrent_peak_without_summing_peaks() {
        let mut a = UsageFacts {
            cpu_seconds: 3.0,
            energy_j: 2.0,
            mem_peak_bytes: 100,
            mem_byte_ms: 80000.0,
            present_ms: 1000,
            ..UsageFacts::default()
        };
        let b = UsageFacts {
            cpu_seconds: 5.0,
            energy_j: 4.0,
            mem_peak_bytes: 150,
            mem_byte_ms: 120000.0,
            present_ms: 1000,
            ..UsageFacts::default()
        };
        a.merge(&b);
        assert_eq!(a.cpu_seconds, 8.0);
        assert_eq!(a.energy_j, 6.0);
        assert_eq!(a.mem_peak_bytes, 150);
        assert_eq!(a.mem_byte_ms / a.present_ms as f64, 100.0);
    }

    #[test]
    fn a_full_duration_retains_source_incompleteness_when_rolled_up() {
        let mut a = UsageFacts::default();
        a.coverage[0] = UsageSourceCoverage {
            supported: true,
            measured_ms: 10000,
            causes: 0,
        };
        let mut b = a.clone();
        b.coverage[0].causes = cause_mask(UsageIncompleteCause::UnreadableProcess);
        a.merge(&b);
        assert_eq!(a.coverage[0].measured_ms, 20000);
        assert_eq!(
            causes(a.coverage[0].causes),
            vec![UsageIncompleteCause::UnreadableProcess]
        );
    }
}
