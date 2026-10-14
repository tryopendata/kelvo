//! Translation between storage/engine results and shell IPC types.
use super::*;

impl From<kelvo_engine::HistoryHealth> for HistoryHealth {
    fn from(h: kelvo_engine::HistoryHealth) -> Self {
        HistoryHealth {
            low_disk_paused: h.low_disk_paused,
            trimmed_before_ms: h.trimmed_before_ms,
            trimmed_limit_bytes: h.trimmed_limit_bytes,
            cap_met: h.cap_met,
        }
    }
}

impl HistoryRequest {
    pub fn to_query(&self) -> Result<HistoryQuery, CommandError> {
        if self.from_ms >= self.to_ms {
            return Err(CommandError::InvalidArgument {
                message: format!(
                    "from_ms {} is not before to_ms {}",
                    self.from_ms, self.to_ms
                ),
            });
        }
        if self.max_points == 0 {
            return Err(CommandError::InvalidArgument {
                message: "max_points must be at least 1".into(),
            });
        }
        Ok(HistoryQuery {
            host: self.host,
            selectors: self.selectors.clone(),
            from_ms: self.from_ms,
            to_ms: self.to_ms,
            tier: tier_choice(self.tier),
            max_points: self.max_points,
        })
    }
}

fn tier_choice(tier: TierRequest) -> TierChoice {
    match tier {
        TierRequest::Auto => TierChoice::Auto,
        TierRequest::S10 => TierChoice::Fixed(Tier::S10),
        TierRequest::M1 => TierChoice::Fixed(Tier::M1),
    }
}

/// The most days one heatmap request may ask for: the longest retention (90 days) plus
/// slack for the days a range starts and ends in.
pub const MAX_HEATMAP_DAYS: usize = 92;

/// The most hours one `battery_hours` request may ask for: a heatmap's days.
pub const MAX_BATTERY_HOURS: usize = MAX_HEATMAP_DAYS * 24;

impl HeatmapMetric {
    pub fn series(self) -> SeriesKey {
        let metric = match self {
            HeatmapMetric::Cpu => "cpu.total",
            HeatmapMetric::Temp => "thermal.hottest",
        };
        SeriesKey::new(MetricId::from_static(metric), Labels::new())
    }
}

impl HeatmapRequest {
    /// The hour cells, every day's 24 in order, after checking that each day has 25
    /// never-decreasing boundaries and the days do not overlap.
    pub fn cells(&self) -> Result<Vec<(i64, i64)>, CommandError> {
        let invalid = |message: String| CommandError::InvalidArgument { message };
        if self.days.len() > MAX_HEATMAP_DAYS {
            return Err(invalid(format!(
                "{} days asked for, at most {MAX_HEATMAP_DAYS}",
                self.days.len()
            )));
        }
        let mut cells = Vec::with_capacity(self.days.len() * 24);
        let mut prev_end = i64::MIN;
        for day in &self.days {
            if day.hour_starts.len() != 25 {
                return Err(invalid(format!(
                    "{}: {} hour boundaries, expected 25",
                    day.date,
                    day.hour_starts.len()
                )));
            }
            for w in day.hour_starts.windows(2) {
                let (Some(start), Some(end)) = (w.first(), w.get(1)) else {
                    continue;
                };
                if start.0 < prev_end || end.0 < start.0 {
                    return Err(invalid(format!(
                        "{}: hour boundaries go backwards at {}",
                        day.date, start.0
                    )));
                }
                cells.push((start.0, end.0));
                prev_end = end.0;
            }
        }
        Ok(cells)
    }

    /// `hours` (one per cell, from [`HeatmapRequest::cells`]) back into days.
    pub fn days(&self, hours: &[Option<f32>]) -> Vec<HeatmapDay> {
        self.days
            .iter()
            .zip(hours.chunks(24))
            .map(|(day, hours)| HeatmapDay {
                date: day.date.clone(),
                hours: hours.to_vec(),
            })
            .collect()
    }
}

impl ExportRequest {
    pub fn to_query(&self) -> Result<ExportQuery, CommandError> {
        if self.from_ms >= self.to_ms {
            return Err(CommandError::InvalidArgument {
                message: format!(
                    "from_ms {} is not before to_ms {}",
                    self.from_ms, self.to_ms
                ),
            });
        }
        if self.selectors.is_empty() {
            return Err(CommandError::InvalidArgument {
                message: "no series selected".into(),
            });
        }
        Ok(ExportQuery {
            host: self.host,
            selectors: self.selectors.clone(),
            from_ms: self.from_ms,
            to_ms: self.to_ms,
            tier: tier_choice(self.tier),
        })
    }
}

/// The IPC page of `r`, each series with `hold(key, bucket_ms)` as its `hold_ms`.
pub(super) fn history_page(r: HistoryResult, hold: impl Fn(&SeriesKey, i64) -> i64) -> HistoryPage {
    HistoryPage {
        tier: r.tier,
        bucket_ms: r.bucket_ms,
        series: r
            .series
            .into_iter()
            .map(|s| HistorySeries {
                hold_ms: hold(&s.key, r.bucket_ms),
                key: s.key,
                points: s
                    .points
                    .into_iter()
                    .map(|p| HistoryPoint {
                        t: p.t,
                        min: p.min,
                        max: p.max,
                        avg: p.avg,
                    })
                    .collect(),
            })
            .collect(),
        gaps: r.gaps,
    }
}

impl From<kelvo_engine::NetAttribution> for NetworkByApp {
    fn from(a: kelvo_engine::NetAttribution) -> Self {
        NetworkByApp {
            from_ms: a.from_ms,
            to_ms: a.to_ms,
            complete_to_ms: a.to_ms,
            resolution_ms: a.resolution_ms,
            measured_ms: a.measured_ms,
            coverage: a
                .coverage
                .iter()
                .map(|s| NetworkSpan {
                    from_ms: s.from_ms,
                    to_ms: s.to_ms,
                    tier: s.tier,
                })
                .collect(),
            clamped: a.clamped(),
            apps: a
                .apps
                .into_iter()
                .map(|app| crate::ipc::AppBytes {
                    name: app.name,
                    rx_bytes: app.rx_bytes,
                    tx_bytes: app.tx_bytes,
                })
                .collect(),
            other_apps_rx_bytes: a.other_apps_rx_bytes,
            other_apps_tx_bytes: a.other_apps_tx_bytes,
            iface_rx_bytes: a.rx.iface_bytes,
            iface_tx_bytes: a.tx.iface_bytes,
            overhead_rx_bytes: a.rx.overhead_bytes,
            overhead_tx_bytes: a.tx.overhead_bytes,
            system_rx_bytes: a.rx.system_bytes,
            system_tx_bytes: a.tx.system_bytes,
        }
    }
}

impl From<kelvo_store::ProcessesAt> for ProcessesAt {
    fn from(p: kelvo_store::ProcessesAt) -> Self {
        ProcessesAt {
            ts_ms: p.ts_ms,
            resolution: match p.resolution {
                ProcResolution::Snapshot => ProcessResolution::Snapshot,
                ProcResolution::Top5PerMinute => ProcessResolution::Top5PerMinute,
                ProcResolution::Top5Per15Minutes => ProcessResolution::Top5Per15Minutes,
            },
            rows: p
                .rows
                .into_iter()
                .map(|r| StoredProcess {
                    name: r.name,
                    pid: r.pid,
                    cpu_pct: r.cpu_pct,
                    mem_bytes: r.mem_bytes,
                    threads: r.threads,
                    idle_wakeups_per_s: r.idle_wakeups_per_s,
                    energy: r.energy,
                })
                .collect(),
        }
    }
}
