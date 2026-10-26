//! Shared query projection and source completeness, after durable/recent reconciliation.
use std::collections::BTreeMap;

use super::{UsageFacts, UsageSourceCoverage, cause_mask, causes};
use kelvo_schema::durable_usage::*;

pub(crate) const METRICS: [HistoricalUsageMetric; 5] = [
    HistoricalUsageMetric::Cpu,
    HistoricalUsageMetric::Gpu,
    HistoricalUsageMetric::Memory,
    HistoricalUsageMetric::Energy,
    HistoricalUsageMetric::Network,
];

#[derive(Default)]
pub(crate) struct QueryFacts {
    pub apps: BTreeMap<String, UsageFacts>,
    pub processes: BTreeMap<(i32, i64, String), UsageFacts>,
    pub source: [UsageSourceCoverage; 5],
    pub spans: Vec<UsageHistorySpan>,
    pub boundary: bool,
    pub revision: i64,
    pub recorded_from: Option<i64>,
    pub retained_from: i64,
    pub uncertainty: Vec<UsageNetworkUncertainty>,
}

impl QueryFacts {
    pub fn observe(&mut self, from: i64, to: i64, source: [UsageSourceCoverage; 5]) {
        for (a, b) in self.source.iter_mut().zip(source) {
            a.supported |= b.supported;
            a.measured_ms = a.measured_ms.saturating_add(b.measured_ms);
            a.causes |= b.causes;
        }
        self.spans.push(UsageHistorySpan {
            from_ms: from,
            to_ms: to,
            resolution_ms: (to - from) as u64,
        });
    }

    pub fn range(&self, from: i64, to: i64) -> UsageHistoryRange {
        let mut spans = self.spans.clone();
        spans.sort_by_key(|s| s.from_ms);
        let mut merged: Vec<UsageHistorySpan> = Vec::new();
        for s in spans {
            if let Some(last) = merged.last_mut()
                && last.to_ms == s.from_ms
                && last.resolution_ms == s.resolution_ms
            {
                last.to_ms = s.to_ms;
            } else {
                merged.push(s);
            }
        }
        let coverage = METRICS
            .iter()
            .enumerate()
            .map(|(i, metric)| {
                let c = self.source.get(i).copied().unwrap_or_default();
                let mut mask = c.causes;
                if self.boundary {
                    mask |= cause_mask(UsageIncompleteCause::PartialBoundary);
                }
                if c.measured_ms < (to - from) as u64 {
                    mask |= cause_mask(if self.recorded_from.is_none_or(|s| from < s) {
                        UsageIncompleteCause::NotRecorded
                    } else if from < self.retained_from {
                        UsageIncompleteCause::Pruned
                    } else {
                        UsageIncompleteCause::SamplingGap
                    });
                }
                if *metric == HistoricalUsageMetric::Network && !self.uncertainty.is_empty() {
                    mask |= cause_mask(UsageIncompleteCause::UncertainNetworkTime);
                }
                UsageMetricCoverage {
                    metric: *metric,
                    coverage: UsageResourceCoverage {
                        supported: c.supported,
                        measured_ms: c.measured_ms,
                        complete: c.supported && mask == 0 && c.measured_ms == (to - from) as u64,
                        causes: causes(mask),
                    },
                }
            })
            .collect();
        UsageHistoryRange {
            requested_from_ms: from,
            requested_to_ms: to,
            spans: merged,
            coverage,
        }
    }
}

pub(crate) fn figures(
    facts: Option<&UsageFacts>,
    range: &UsageHistoryRange,
) -> HistoricalUsageFigures {
    let source = |i: usize| range.coverage.get(i).map(|c| &c.coverage);
    let available = |i: usize| {
        source(i).is_some_and(|c| c.supported && c.measured_ms > 0)
            && facts.is_some_and(|f| f.coverage.get(i).is_some_and(|c| c.measured_ms > 0))
    };
    let absent_zero = |i: usize| facts.is_none() && source(i).is_some_and(|c| c.complete);
    let seconds = |i: usize| source(i).map_or(0.0, |c| c.measured_ms as f64 / 1000.0);
    let f = facts.cloned().unwrap_or_default();
    let mut coverage = range.coverage.clone();
    if facts.is_some() {
        for (out, row) in coverage.iter_mut().zip(f.coverage) {
            out.coverage.supported &= row.supported;
            out.coverage.measured_ms = row.measured_ms;
            for cause in causes(row.causes) {
                if !out.coverage.causes.contains(&cause) {
                    out.coverage.causes.push(cause);
                }
            }
            out.coverage.complete &= row.causes == 0;
        }
    }
    HistoricalUsageFigures {
        cpu_avg_pct: if available(0) {
            Some(f.cpu_seconds / seconds(0) * 100.0)
        } else {
            absent_zero(0).then_some(0.0)
        },
        gpu_avg_pct: if available(1) {
            Some(f.gpu_seconds / seconds(1) * 100.0)
        } else {
            absent_zero(1).then_some(0.0)
        },
        mem_peak_bytes: if available(2) {
            Some(f.mem_peak_bytes)
        } else {
            absent_zero(2).then_some(0)
        },
        mem_avg_bytes: if available(2) && f.present_ms > 0 {
            Some((f.mem_byte_ms / f.present_ms as f64).round() as u64)
        } else {
            absent_zero(2).then_some(0)
        },
        energy_j: if available(3) {
            Some(f.energy_j)
        } else {
            absent_zero(3).then_some(0.0)
        },
        cpu_power_w: if available(3) {
            Some(f.energy_j / seconds(3))
        } else {
            absent_zero(3).then_some(0.0)
        },
        network_rx_bytes: if available(4) {
            Some(f.network_rx_bytes)
        } else {
            absent_zero(4).then_some(0)
        },
        network_tx_bytes: if available(4) {
            Some(f.network_tx_bytes)
        } else {
            absent_zero(4).then_some(0)
        },
        coverage,
    }
}

pub(crate) fn rank(f: &HistoricalUsageFigures, by: HistoricalUsageMetric) -> f64 {
    match by {
        HistoricalUsageMetric::Cpu => f.cpu_avg_pct.unwrap_or(0.0),
        HistoricalUsageMetric::Gpu => f.gpu_avg_pct.unwrap_or(0.0),
        HistoricalUsageMetric::Memory => f.mem_peak_bytes.unwrap_or(0) as f64,
        HistoricalUsageMetric::Energy => f.energy_j.unwrap_or(0.0),
        HistoricalUsageMetric::Network => {
            f.network_rx_bytes
                .unwrap_or(0)
                .saturating_add(f.network_tx_bytes.unwrap_or(0)) as f64
        }
    }
}
