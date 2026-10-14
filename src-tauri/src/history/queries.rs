//! Query adapters: validate IPC requests, combine persisted and recent values.
use super::*;
impl History {
    /// `query_network_by_app`: the stored tiers plus `recent` (the engine's in-memory
    /// buckets, which replace stored rows of the same bucket), split into apps, overhead
    /// and the remainder. `recent` is called after the stored rows are read, so its
    /// copies are never older than them; `complete_to_ms` comes from the same call and
    /// stops where the engine's buckets stop being final. With the store unavailable,
    /// the ring alone answers (live-only). Blocking.
    pub fn network_by_app(
        &self,
        host: kelvo_schema::HostId,
        from_ms: i64,
        to_ms: i64,
        recent: impl FnOnce() -> kelvo_engine::RecentNet,
    ) -> Result<NetworkByApp, CommandError> {
        validate_range(from_ms, to_ms, MAX_NET_SPAN_MS)?;
        let (read, complete_to_ms) = self.storage.network_by_app(host, from_ms, to_ms, recent)?;
        let mut out: NetworkByApp = read.into();
        // The engine always has an edge once it has started; before that, nothing is
        // promised final.
        out.complete_to_ms = out
            .complete_to_ms
            .min(complete_to_ms.unwrap_or(out.from_ms));
        Ok(out)
    }

    /// `query_history`: the stored tier plus `recent` (the engine's bucket rows since its
    /// last commit and its open buckets, which replace stored rows of the same bucket and
    /// layout), so history answers through now (D-092). `hold(key, bucket_ms)` is each
    /// series' `hold_ms`. With the store unavailable, the recent rows alone answer
    /// (live-only). Blocking.
    pub fn history(
        &self,
        request: &HistoryRequest,
        recent: impl Fn() -> Vec<BucketRow>,
        hold: impl Fn(&SeriesKey, i64) -> i64,
    ) -> Result<HistoryPage, CommandError> {
        let query = request.to_query()?;
        Ok(history_page(
            self.history_through_now(&query, recent)?,
            hold,
        ))
    }

    /// Bounded disk query. Pairing and operation weighting remain in kelvo-store.
    pub fn disk_io(
        &self,
        request: &kelvo_schema::disk_io::DiskIoRequest,
        now_ms: i64,
        recent: impl Fn() -> Vec<BucketRow>,
    ) -> Result<kelvo_schema::disk_io::DiskIoPage, CommandError> {
        if request.from_ms < 0
            || request.to_ms <= request.from_ms
            || request.to_ms > kelvo_schema::JS_SAFE_INT_MAX
            || request.to_ms.saturating_sub(request.from_ms) > MAX_NET_SPAN_MS
            || !(1..=2000).contains(&request.max_points)
            || request.device.as_ref().is_some_and(|dev| {
                dev.is_empty()
                    || dev.len() > 64
                    || !dev
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
        {
            return Err(CommandError::InvalidArgument {
                message: "Disk I/O request exceeds device, range or point bounds".into(),
            });
        }
        let labels = request
            .device
            .as_ref()
            .map_or_else(Labels::new, |dev| Labels::single("dev", dev));
        let query = HistoryQuery {
            host: request.host,
            from_ms: request.from_ms,
            to_ms: request.to_ms,
            tier: TierChoice::Auto,
            max_points: ((request.to_ms - request.from_ms) / 10_000 + 2) as u32,
            selectors: (if request.device.is_some() {
                &kelvo_store::disk_io::DISK_IO_METRICS
            } else {
                &kelvo_schema::disk_io::TOTAL_METRICS
            })
            .iter()
            .map(|id| kelvo_schema::SeriesSelector {
                metric: MetricId::from_static(id),
                labels: labels.clone(),
            })
            .collect(),
        };
        let read = self.history_through_now(&query, recent)?;
        if read.series.len() > 1024 {
            return Err(CommandError::InvalidArgument {
                message: "Select a disk: this range contains more than 128 device identities"
                    .into(),
            });
        }
        Ok(kelvo_store::disk_io::disk_io_page(
            read,
            request.device.as_deref(),
            request.from_ms,
            request.to_ms,
            now_ms,
            request.max_points,
        ))
    }

    /// [`Reader::history_after`] with `recent`, called after the stored rows are read so
    /// its copies are never older than them; the recent rows alone when the store is
    /// unavailable. The store can close between a check and the read, so the read's own
    /// `history_unavailable` is what sends this to the recent rows.
    fn history_through_now(
        &self,
        query: &HistoryQuery,
        recent: impl Fn() -> Vec<BucketRow>,
    ) -> Result<HistoryResult, CommandError> {
        Ok(self.storage.history_through_now(query, recent)?)
    }

    /// `query_series_stats`: each unlabelled metric of `metrics` over `[from_ms, to_ms)`
    /// from its rollups, through now as `query_history` reads them: the span-weighted
    /// average (D-092), the largest sample, and the average times the time measured (bytes
    /// for a bytes-per-second metric). A bucket counts the time it covers, cut at
    /// `now_ms`, less any gap inside it that applies to the metric's module; a bucket with
    /// no reading counts nothing. The tier is the one `query_history` would pick.
    /// Blocking.
    pub fn series_stats(
        &self,
        host: kelvo_schema::HostId,
        metrics: &[String],
        from_ms: i64,
        to_ms: i64,
        now_ms: i64,
        recent: impl Fn() -> Vec<BucketRow>,
    ) -> Result<RangeStats, CommandError> {
        validate_range(from_ms, to_ms, MAX_NET_SPAN_MS)?;
        let catalog = kelvo_schema::Catalog::builtin();
        let defs = metrics
            .iter()
            .map(|m| match catalog.get(m) {
                Some(d) if d.label_keys.is_empty() => Ok(d),
                Some(_) => Err(CommandError::InvalidArgument {
                    message: format!("{m} has labels; series stats take unlabelled metrics"),
                }),
                None => Err(CommandError::InvalidArgument {
                    message: format!("{m} is not in the catalog"),
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        // One point per bucket of the finest tier: no merging.
        let query = per_bucket_query(
            host,
            defs.iter().map(|d| d.id.clone()),
            from_ms,
            to_ms,
            TierChoice::Auto,
            Tier::S10.bucket_ms().unwrap_or(10_000),
        );
        let read = self.history_through_now(&query, recent)?;
        Ok(kelvo_store::range_stats(
            &read, &defs, from_ms, to_ms, now_ms,
        ))
    }

    /// `battery_hours`: for each hour between consecutive `hour_starts` (the
    /// client's local hours in UTC ms, DST applied, as a heatmap day's), the charge in the
    /// hour's last minute that sampled it and whether it charged in any minute. Minutes
    /// come from `tier_1m` and `recent`, as `query_history` reads them. An hour with no
    /// minute left (`tier_1m` keeps a week; older minutes are rolled into `tier_15m`)
    /// takes the average charge of its last 15 minutes that sampled it, and whether it
    /// charged in any of them. Every zone is offset from UTC by a multiple of 15 minutes,
    /// so neither a minute nor a quarter hour straddles a local hour. Blocking.
    pub fn battery_hours(
        &self,
        host: kelvo_schema::HostId,
        hour_starts: &[i64],
        recent: impl Fn() -> Vec<BucketRow>,
    ) -> Result<Vec<BatteryHour>, CommandError> {
        let invalid = |message: String| CommandError::InvalidArgument { message };
        let hours = hour_starts.len().saturating_sub(1);
        if hours == 0 || hours > MAX_BATTERY_HOURS {
            return Err(invalid(format!(
                "{} hour boundaries, expected 2 to {}",
                hour_starts.len(),
                MAX_BATTERY_HOURS + 1
            )));
        }
        let cells: Vec<(i64, i64)> = hour_starts
            .windows(2)
            .filter_map(|w| Some((*w.first()?, *w.get(1)?)))
            .collect();
        if let Some(&(start, _)) = cells.iter().find(|(start, end)| end < start) {
            return Err(invalid(format!("hour boundaries go backwards at {start}")));
        }
        let mut out = vec![BatteryCell::default(); cells.len()];
        let (from_ms, to_ms) = (
            cells.first().map_or(0, |c| c.0),
            cells.last().map_or(0, |c| c.1),
        );
        if to_ms > from_ms {
            let query = |tier: Tier, from_ms: i64, to_ms: i64| {
                per_bucket_query(
                    host,
                    [BATTERY_CHARGE, BATTERY_CHARGING].map(MetricId::from_static),
                    from_ms,
                    to_ms,
                    TierChoice::Fixed(tier),
                    tier.bucket_ms().unwrap_or(60_000),
                )
            };
            let minutes = self.history_through_now(&query(Tier::M1, from_ms, to_ms), &recent)?;
            fill_battery_hours(&cells, &minutes, &mut out);
            // Hours older than the minutes kept: their quarter hours.
            let bare = |i: &usize| out.get(*i).is_some_and(|c| !c.has_charge());
            let first = (0..cells.len()).find(bare);
            let last = (0..cells.len()).rev().find(bare);
            if let (Some(&(q_from, _)), Some(&(_, q_to))) = (
                first.and_then(|i| cells.get(i)),
                last.and_then(|i| cells.get(i)),
            ) && q_to > q_from
            {
                let quarters =
                    self.history_through_now(&query(Tier::M15, q_from, q_to), &recent)?;
                let mut quarter_out = out.clone();
                fill_battery_hours(&cells, &quarters, &mut quarter_out);
                for (hour, q) in out.iter_mut().zip(quarter_out) {
                    if !hour.has_charge() {
                        *hour = q;
                    }
                }
            }
        }
        Ok(cells
            .iter()
            .zip(out)
            .map(|(&(start_ms, _), c)| BatteryHour {
                start_ms,
                charge: c.charge,
                charging: c.charging,
            })
            .collect())
    }
}
/// `invalid_argument` unless `[from_ms, to_ms)` runs forwards and spans at most `max_ms`
/// (whole days).
fn validate_range(from_ms: i64, to_ms: i64, max_ms: i64) -> Result<(), CommandError> {
    if to_ms < from_ms {
        return Err(CommandError::InvalidArgument {
            message: format!("the range ends ({to_ms}) before it starts ({from_ms})"),
        });
    }
    if to_ms - from_ms > max_ms {
        return Err(CommandError::InvalidArgument {
            message: format!(
                "the range is {} days long; history keeps at most {} days",
                (to_ms - from_ms) / Retention::DAY_MS,
                max_ms / Retention::DAY_MS
            ),
        });
    }
    Ok(())
}

/// A read of the unlabelled `metrics` with room for one point per `bucket_ms` bucket of
/// `[from_ms, to_ms)`, so none are merged.
fn per_bucket_query(
    host: kelvo_schema::HostId,
    metrics: impl IntoIterator<Item = MetricId>,
    from_ms: i64,
    to_ms: i64,
    tier: TierChoice,
    bucket_ms: i64,
) -> HistoryQuery {
    HistoryQuery {
        host,
        selectors: metrics
            .into_iter()
            .map(|metric| kelvo_schema::SeriesSelector {
                metric,
                labels: Labels::new(),
            })
            .collect(),
        from_ms,
        to_ms,
        tier,
        max_points: u32::try_from((to_ms - from_ms) / bucket_ms + 2).unwrap_or(u32::MAX),
    }
}
