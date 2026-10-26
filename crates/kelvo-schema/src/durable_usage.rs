//! Durable interval attribution (D-119). Separate from the recent live usage ring.
//! Storage integration must pass the independent feasibility and lifecycle gates.

use serde::{Deserialize, Serialize};

use crate::HostId;

/// Resource used to rank the whole recorded app population before returning ten.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalUsageMetric {
    Cpu,
    Gpu,
    Memory,
    Energy,
    Network,
}

/// Why observations cannot prove a complete attribution for a resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UsageIncompleteCause {
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
}

/// Completeness is a source fact. Covered time alone never proves it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UsageResourceCoverage {
    pub supported: bool,
    #[specta(type = crate::JsSafeInt)]
    pub measured_ms: u64,
    /// Complete only if the source observed the whole requested interval and population.
    pub complete: bool,
    pub causes: Vec<UsageIncompleteCause>,
}

/// Coverage of one resource, independently of the other resources in the reply.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UsageMetricCoverage {
    pub metric: HistoricalUsageMetric,
    pub coverage: UsageResourceCoverage,
}

/// The requested interval and the retained whole-bucket spans actually used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UsageHistoryRange {
    #[specta(type = crate::JsSafeInt)]
    pub requested_from_ms: i64,
    #[specta(type = crate::JsSafeInt)]
    pub requested_to_ms: i64,
    pub spans: Vec<UsageHistorySpan>,
    pub coverage: Vec<UsageMetricCoverage>,
}

/// A contiguous observation span; holes are not silently added to its duration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UsageHistorySpan {
    #[specta(type = crate::JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = crate::JsSafeInt)]
    pub to_ms: i64,
    #[specta(type = crate::JsSafeInt)]
    pub resolution_ms: u64,
}

/// Resource values for an app or process. None means unavailable, never zero use.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct HistoricalUsageFigures {
    pub cpu_avg_pct: Option<f64>,
    pub gpu_avg_pct: Option<f64>,
    #[specta(type = Option<crate::JsSafeInt>)]
    pub mem_peak_bytes: Option<u64>,
    #[specta(type = Option<crate::JsSafeInt>)]
    pub mem_avg_bytes: Option<u64>,
    pub energy_j: Option<f64>,
    pub cpu_power_w: Option<f64>,
    #[specta(type = Option<crate::JsSafeInt>)]
    pub network_rx_bytes: Option<u64>,
    #[specta(type = Option<crate::JsSafeInt>)]
    pub network_tx_bytes: Option<u64>,
    pub coverage: Vec<UsageMetricCoverage>,
}

/// Query ten app groups, or values for explicit identities during comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct HistoricalUsageRequest {
    pub host: HostId,
    #[specta(type = crate::JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = crate::JsSafeInt)]
    pub to_ms: i64,
    pub by: HistoricalUsageMetric,
    /// Empty means rank all observed apps; otherwise return these identities without
    /// choosing an independently ranked population. An absent row is zero only when
    /// its resource coverage proves complete observation.
    pub app_names: Vec<String>,
}

/// One app group. Its peak is a simultaneous sum, not a sum of process peaks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct HistoricalUsageApp {
    pub name: String,
    pub figures: HistoricalUsageFigures,
    /// False means expansion has no retained process-instance evidence.
    pub process_details_available: bool,
}

/// Source-history answer. No live process controls or Quit targets are exposed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct HistoricalUsageReply {
    pub range: UsageHistoryRange,
    pub by: HistoricalUsageMetric,
    /// Ordered largest first after interval aggregation. UI reversal reverses these ten.
    pub apps: Vec<HistoricalUsageApp>,
    /// False means this is a ranking of observed use, with explicit missing evidence.
    pub ranking_complete: bool,
    /// Late ownership can identify bytes without identifying their transfer interval.
    /// These are excluded from exact interval figures; overlap is only uncertainty.
    /// At most one summary for each returned app plus one combined unnamed/other-app
    /// summary: eleven records maximum. Raw uncertainty stays durable, not in this reply.
    pub network_uncertainty: Vec<UsageNetworkUncertainty>,
}

/// Lazy expansion of one app within the exact same requested interval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UsageProcessRequest {
    pub host: HostId,
    #[specta(type = crate::JsSafeInt)]
    pub from_ms: i64,
    #[specta(type = crate::JsSafeInt)]
    pub to_ms: i64,
    pub app_name: String,
    pub by: HistoricalUsageMetric,
    /// 1..=100, rejected rather than silently clamped.
    pub page_size: u16,
    /// Opaque source-generated continuation, bound to host, app, interval, ranking,
    /// history epoch and the interval's snapshot revision. Stale tokens are rejected.
    pub cursor: Option<String>,
}

/// A historical process instance, safe against PID reuse. Never a current action target.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct HistoricalUsageProcess {
    pub pid: i32,
    #[specta(type = crate::JsSafeInt)]
    pub start_time_us: i64,
    pub name: String,
    pub figures: HistoricalUsageFigures,
}

/// A bounded page of retained process instances, ranked after full interval aggregation.
/// Ordering is metric descending, then PID and start time ascending for stable ties.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct UsageProcessReply {
    pub range: UsageHistoryRange,
    pub app_name: String,
    pub processes: Vec<HistoricalUsageProcess>,
    /// Network is app-level unless identity-safe per-instance evidence was recorded.
    pub network_app_level_only: bool,
    /// Present while further process instances remain. No retained instances are dropped.
    pub next_cursor: Option<String>,
}

/// Summary of bytes whose owner became known after transfer, with the union's
/// conservative possible-time envelope. It never implies exact transfer timestamps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct UsageNetworkUncertainty {
    pub app_name: Option<String>,
    #[specta(type = crate::JsSafeInt)]
    pub possible_from_ms: i64,
    #[specta(type = crate::JsSafeInt)]
    pub possible_to_ms: i64,
    #[specta(type = crate::JsSafeInt)]
    pub rx_bytes: u64,
    #[specta(type = crate::JsSafeInt)]
    pub tx_bytes: u64,
    #[specta(type = crate::JsSafeInt)]
    pub records: u64,
}

/// Durable usage is limited to the existing longest Timeline window.
pub const USAGE_HISTORY_MAX_RANGE_MS: i64 = 30 * 86_400_000;
pub const USAGE_PROCESS_PAGE_MAX: u16 = 100;

fn validate_range(from: i64, to: i64) -> Result<(), &'static str> {
    if !crate::is_js_safe(from)
        || !crate::is_js_safe(to)
        || to <= from
        || to.saturating_sub(from) > USAGE_HISTORY_MAX_RANGE_MS
    {
        return Err("usage interval must be nonempty, JS-safe and at most thirty days");
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 1024 && !name.contains('\0')
}

impl HistoricalUsageRequest {
    /// Validate at each command/remote boundary before a store read or allocation.
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_range(self.from_ms, self.to_ms)?;
        if self.app_names.len() > 10 || self.app_names.iter().any(|n| !valid_name(n)) {
            return Err("comparison membership must contain at most ten valid app identities");
        }
        if self
            .app_names
            .iter()
            .enumerate()
            .any(|(i, name)| self.app_names[..i].contains(name))
        {
            return Err("comparison membership contains duplicate app identities");
        }
        Ok(())
    }
}

impl UsageProcessRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        validate_range(self.from_ms, self.to_ms)?;
        if !valid_name(&self.app_name) || !(1..=USAGE_PROCESS_PAGE_MAX).contains(&self.page_size) {
            return Err(
                "process expansion needs a valid app identity and page size between one and one hundred",
            );
        }
        if self
            .cursor
            .as_ref()
            .is_some_and(|c| c.is_empty() || c.len() > 4096)
        {
            return Err("process continuation is invalid or too large");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_bytes_preserve_possible_interval_instead_of_becoming_transfer_time() {
        let late = UsageNetworkUncertainty {
            app_name: Some("Browser".into()),
            possible_from_ms: 1000,
            possible_to_ms: 90000,
            rx_bytes: 700,
            tx_bytes: 20,
            records: 1,
        };
        let mut bytes = Vec::new();
        ciborium::into_writer(&late, &mut bytes).unwrap();
        let decoded: UsageNetworkUncertainty = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(decoded, late);
        assert!(decoded.possible_from_ms < 30000 && decoded.possible_to_ms > 60000);
    }

    #[test]
    fn full_duration_does_not_discard_source_incompleteness() {
        let source = UsageResourceCoverage {
            supported: true,
            measured_ms: 60000,
            complete: false,
            causes: vec![
                UsageIncompleteCause::UnreadableProcess,
                UsageIncompleteCause::IdentityLimit,
            ],
        };
        let encoded = serde_json::to_value(&source).unwrap();
        assert_eq!(encoded["complete"], false);
        let decoded: UsageResourceCoverage = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, source);
    }

    #[test]
    fn invalid_membership_or_intervals_are_rejected_before_history_reads() {
        let mut request = HistoricalUsageRequest {
            host: HostId(uuid::Uuid::nil()),
            from_ms: 0,
            to_ms: USAGE_HISTORY_MAX_RANGE_MS,
            by: HistoricalUsageMetric::Cpu,
            app_names: Vec::new(),
        };
        assert_eq!(request.validate(), Ok(()));
        request.to_ms += 1;
        assert!(request.validate().is_err());
        request.to_ms = 0;
        assert!(request.validate().is_err());
        request.to_ms = 60000;
        request.app_names = vec!["Browser".into(), "Browser".into()];
        assert!(request.validate().is_err());
        request.app_names = (0..11).map(|i| format!("App {i}")).collect();
        assert!(request.validate().is_err());
        request.app_names.pop();
        assert_eq!(request.validate(), Ok(()));
    }

    #[test]
    fn process_expansion_requires_bounded_pages_and_continuations() {
        let mut request = UsageProcessRequest {
            host: HostId(uuid::Uuid::nil()),
            from_ms: 0,
            to_ms: 60000,
            app_name: "Compiler".into(),
            by: HistoricalUsageMetric::Cpu,
            page_size: 100,
            cursor: None,
        };
        assert_eq!(request.validate(), Ok(()));
        request.page_size = 101;
        assert!(request.validate().is_err());
        request.page_size = 0;
        assert!(request.validate().is_err());
        request.page_size = 1;
        request.cursor = Some("x".repeat(4097));
        assert!(request.validate().is_err());
        request.cursor = Some("opaque snapshot continuation".into());
        assert_eq!(request.validate(), Ok(()));
    }
}
