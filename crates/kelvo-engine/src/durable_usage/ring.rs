//! Recent complete and open facts, retained for the writer commit interval plus slack.
use kelvo_store::usage::UsageBucket;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct UsageRecent {
    buckets: BTreeMap<i64, UsageBucket>,
    pub generation: u64,
    pub recorder: super::UsageRecorder,
}
impl UsageRecent {
    pub fn update_owned(&mut self, buckets: impl IntoIterator<Item = UsageBucket>, now: i64) {
        for bucket in buckets {
            self.buckets.insert(bucket.from_ms, bucket);
        }
        self.buckets.retain(|_, b| {
            b.to_ms >= now.saturating_sub(900_000)
                || b.uncertain_network
                    .iter()
                    .any(|u| u.possible_to_ms >= now.saturating_sub(900_000))
        });
    }
    pub fn snapshot(&self, from: i64, to: i64) -> Vec<UsageBucket> {
        self.buckets
            .values()
            .chain(self.recorder.buckets())
            .filter(|b| {
                (b.from_ms < to && b.to_ms > from)
                    || b.uncertain_network
                        .iter()
                        .any(|u| u.possible_from_ms < to && u.possible_to_ms > from)
            })
            .cloned()
            .collect()
    }
    pub fn apps(&self, from: i64, to: i64) -> Vec<UsageBucket> {
        self.project(from, to, None)
    }
    pub fn processes(&self, from: i64, to: i64, app: &str) -> Vec<UsageBucket> {
        self.project(from, to, Some(app))
    }
    fn project(&self, from: i64, to: i64, app: Option<&str>) -> Vec<UsageBucket> {
        self.buckets
            .values()
            .chain(self.recorder.buckets())
            .filter(|b| {
                (b.from_ms < to && b.to_ms > from)
                    || b.uncertain_network
                        .iter()
                        .any(|u| u.possible_from_ms < to && u.possible_to_ms > from)
            })
            .map(|b| UsageBucket {
                from_ms: b.from_ms,
                to_ms: b.to_ms,
                source: b.source,
                apps: b
                    .apps
                    .iter()
                    .filter(|a| app.is_none_or(|name| a.name.as_ref() == name))
                    .cloned()
                    .collect(),
                processes: app.map_or_else(Vec::new, |name| {
                    b.processes
                        .iter()
                        .filter(|p| p.app_name.as_ref() == name)
                        .cloned()
                        .collect()
                }),
                uncertain_network: b.uncertain_network.clone(),
            })
            .collect()
    }
    pub fn clear(&mut self) {
        self.buckets.clear();
        self.recorder.clear(0);
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn drop_after(&mut self, at: i64) {
        self.buckets.retain(|_, b| b.to_ms <= at);
        self.recorder.drop_after(at);
    }
}
