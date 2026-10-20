//! Which cgroups keep their own series (D-115). Pure: the cgroup collector feeds it the
//! groups present on each pass with a score, and wall time; it answers with a membership
//! that changes at most once a minute, so container churn cannot mint layouts faster.
//!
//! - The top [`OWN_MAX`] groups by mean score over the last minute are members. Everyone
//!   else folds into `{kind=other,name=_other}`, which is always in the series set, so
//!   with the members it fills the catalog's cap of 32 series per metric exactly.
//! - Membership changes only when wall time crosses a minute boundary.
//! - A member that falls out of the top (or whose group went away) stays one more
//!   minute, so a group hovering at the edge or restarting does not flap. While it stays
//!   and has no group, its series have gaps, never zeros.
//! - The first pass after [`Cap::seed`] only starts the minute.

use std::collections::HashMap;

/// Series per `cgroup.*` metric, `_other` included: the catalog's `max_cardinality`.
pub(super) const SERIES_MAX: usize = 32;
/// Groups that keep their own series; the last series of each metric is `_other`.
pub(super) const OWN_MAX: usize = SERIES_MAX - 1;
const MINUTE_MS: i64 = 60_000;

/// A group's identity across passes (the collector's label id).
pub(super) type Id = u32;

#[derive(Debug)]
pub(super) struct Cap {
    own_max: usize,
    /// Sorted.
    members: Vec<Id>,
    /// Members kept one extra minute after leaving the top. Sorted.
    departing: Vec<Id>,
    minute: Option<i64>,
    /// Score sum and passes per group this minute.
    acc: HashMap<Id, (f64, u32)>,
    ranked: Vec<(Id, f64)>,
    next: Vec<Id>,
}

impl Cap {
    pub(super) fn new(own_max: usize) -> Self {
        Self {
            own_max,
            members: Vec::new(),
            departing: Vec::new(),
            minute: None,
            acc: HashMap::new(),
            ranked: Vec::new(),
            next: Vec::new(),
        }
    }

    /// The members, sorted.
    pub(super) fn members(&self) -> &[Id] {
        &self.members
    }

    pub(super) fn is_member(&self, id: Id) -> bool {
        self.members.binary_search(&id).is_ok()
    }

    /// Starts over with the top groups of `present` (at a probe, before any minute).
    pub(super) fn seed(&mut self, present: &[(Id, f64)]) {
        self.minute = None;
        self.acc.clear();
        self.departing.clear();
        self.rank(present.iter().map(|&(id, s)| (id, s)));
        self.members.clear();
        self.members
            .extend(self.ranked.iter().take(self.own_max).map(|&(id, _)| id));
        self.members.sort_unstable();
    }

    /// One pass: the groups present now and their scores at `wall_ms`. True when the
    /// membership changed (only ever on the first pass of a new minute).
    pub(super) fn observe(&mut self, wall_ms: i64, present: &[(Id, f64)]) -> bool {
        let minute = wall_ms.div_euclid(MINUTE_MS);
        for &(id, score) in present {
            let e = self.acc.entry(id).or_insert((0.0, 0));
            e.0 += score;
            e.1 += 1;
        }
        let boundary = self.minute.is_some_and(|m| m != minute);
        self.minute = Some(minute);
        if !boundary {
            return false;
        }
        let acc = std::mem::take(&mut self.acc);
        self.rank(present.iter().map(|&(id, _)| {
            let (sum, n) = acc.get(&id).copied().unwrap_or((0.0, 1));
            (id, sum / f64::from(n.max(1)))
        }));
        self.acc = acc;
        self.acc.clear();
        self.choose()
    }

    /// `self.ranked`: `scores` by score descending, then id, so ties are stable.
    fn rank(&mut self, scores: impl Iterator<Item = (Id, f64)>) {
        self.ranked.clear();
        self.ranked.extend(scores);
        self.ranked
            .sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    }

    /// The next membership from `self.ranked`; true when it differs.
    fn choose(&mut self) -> bool {
        let top: Vec<Id> = self
            .ranked
            .iter()
            .take(self.own_max)
            .map(|&(id, _)| id)
            .collect();
        self.next.clear();
        let mut departing = Vec::new();
        // Members that stay in the top keep their place; the others stay one more
        // minute unless they already had it.
        for &id in &self.members {
            if top.contains(&id) {
                self.next.push(id);
            } else if self.departing.binary_search(&id).is_err() {
                self.next.push(id);
                departing.push(id);
            }
        }
        for &id in &top {
            if self.next.len() >= self.own_max {
                break;
            }
            if !self.next.contains(&id) {
                self.next.push(id);
            }
        }
        self.next.sort_unstable();
        departing.sort_unstable();
        self.departing = departing;
        let changed = self.next != self.members;
        std::mem::swap(&mut self.members, &mut self.next);
        changed
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const MIN: i64 = 60_000;

    #[test]
    fn membership_changes_only_on_minute_boundaries() {
        let mut cap = Cap::new(2);
        cap.seed(&[(1, 0.5), (2, 0.4), (3, 0.1)]);
        assert_eq!(cap.members(), [1, 2]);
        // 3 becomes the busiest mid-minute: nothing moves until the minute ends.
        let busy = [(1, 0.5), (2, 0.4), (3, 0.9)];
        for s in (0..60).step_by(5) {
            assert!(!cap.observe(10 * MIN + s * 1000, &busy), "{s}");
        }
        // At the boundary 3 joins; 2 left the top but stays one more minute, so only
        // a free slot could take 3, and there is none: 2 stays, 3 waits.
        assert!(!cap.observe(11 * MIN, &busy));
        assert_eq!(cap.members(), [1, 2]);
        // One minute later 2 has had its extra minute.
        for s in (5..60).step_by(5) {
            assert!(!cap.observe(11 * MIN + s * 1000, &busy));
        }
        assert!(cap.observe(12 * MIN, &busy));
        assert_eq!(cap.members(), [1, 3]);
    }

    #[test]
    fn a_departed_group_is_kept_one_minute_then_dropped() {
        let mut cap = Cap::new(4);
        cap.seed(&[(1, 0.5), (2, 0.4)]);
        cap.observe(0, &[(1, 0.5), (2, 0.4)]);
        // 2 exits.
        assert!(!cap.observe(MIN, &[(1, 0.5)]), "kept one more minute");
        assert!(cap.is_member(2));
        assert!(cap.observe(2 * MIN, &[(1, 0.5)]));
        assert_eq!(cap.members(), [1]);
        // A newcomer with a free slot joins at the next boundary.
        cap.observe(2 * MIN + 5000, &[(1, 0.5), (7, 0.0)]);
        assert!(!cap.is_member(7));
        assert!(cap.observe(3 * MIN, &[(1, 0.5), (7, 0.0)]));
        assert_eq!(cap.members(), [1, 7]);
    }

    #[test]
    fn ranks_by_the_mean_over_the_minute() {
        let mut cap = Cap::new(1);
        cap.seed(&[(1, 0.0), (2, 0.0)]);
        assert_eq!(cap.members(), [1], "ties go to the lower id");
        // 2 spikes once; 1 is steadily higher on average.
        cap.observe(0, &[(1, 0.3), (2, 1.0)]);
        for s in (5..60).step_by(5) {
            cap.observe(s * 1000, &[(1, 0.3), (2, 0.0)]);
        }
        assert!(!cap.observe(MIN, &[(1, 0.3), (2, 0.0)]));
        assert_eq!(cap.members(), [1]);
    }

    /// 200 containers start and stop over an hour, sampled every 5 s: no more than one
    /// membership per minute, and every one fits the cap.
    #[test]
    fn churn_mints_at_most_one_membership_a_minute() {
        let mut cap = Cap::new(OWN_MAX);
        // Container c lives from (c * 17) % 3600 s for 30 + (c * 37) % 900 s; its score
        // drifts with time so the ranking keeps changing.
        let alive = |c: u32, t: i64| {
            let start = i64::from(c * 17 % 3600);
            let life = 30 + i64::from(c * 37 % 900);
            (start..start + life).contains(&t)
        };
        let score = |c: u32, t: i64| f64::from((c * 7919 + (t / 10) as u32 * 31) % 1000) / 1000.0;
        let present = |t: i64| -> Vec<(Id, f64)> {
            (0..200)
                .filter(|&c| alive(c, t))
                .map(|c| (c, score(c, t)))
                .collect()
        };
        cap.seed(&present(0));
        let mut sets = BTreeSet::from([cap.members().to_vec()]);
        let mut layouts = 1;
        let started = std::time::Instant::now();
        for t in (0..3600).step_by(5) {
            let p = present(t);
            if cap.observe(t * 1000, &p) {
                layouts += 1;
                sets.insert(cap.members().to_vec());
            }
            assert!(cap.members().len() <= OWN_MAX);
        }
        let per_pass = started.elapsed() / 720;
        println!(
            "layouts {layouts}, distinct sets {}, {per_pass:?}/pass",
            sets.len()
        );
        assert!(layouts <= 60, "{layouts}");
        assert!(sets.len() <= 60);
        assert!(layouts > 10, "the scenario must churn: {layouts}");
        assert!(
            per_pass < std::time::Duration::from_millis(5),
            "{per_pass:?}"
        );
    }
}
