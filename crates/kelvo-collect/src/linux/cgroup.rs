//! cgroup v2 units and containers (D-115): `cgroup.{cpu,mem,mem_max,io_read,io_write,
//! mem_psi}{kind,name}` and `cgroup.other_count`, every 5 s.
//!
//! [`cgroup_walk`](super::cgroup_walk) finds the groups: system services, Docker and
//! Podman containers, and services of user managers. Per group:
//! - `cgroup.cpu`: `cpu.stat` `usage_usec` over the span since the previous pass, in
//!   percent of one core (a Mean, like a process row).
//! - `cgroup.mem`: `memory.current`. `cgroup.mem_max`: `memory.max` when it is a
//!   number; a group without a limit has no `mem_max` series. Re-read once a minute.
//! - `cgroup.io_read`/`io_write`: `io.stat` `rbytes`/`wbytes` summed over devices, B/s.
//!   A group without the io controller has none.
//! - `cgroup.mem_psi`: the `some` share of `memory.pressure` over the span, percent.
//!
//! Labels: `kind` (service, docker, podman, user), `name` the unit (`nginx.service`),
//! the container name from [`ContainerNames`] (else its 12-character short id), or
//! `<user>/<unit>` for a user manager's service, bounded by
//! [`bound_label_value`]. Groups that share a label (a container recreated under the
//! same name while the old one is going) are summed, so the series continues.
//!
//! Which groups get their own series is [`Cap`]'s choice: the top 31 by CPU and memory
//! share, changing once a minute. The rest sum into `{kind=other,name=_other}` (no
//! `mem_max` or `mem_psi`: neither adds up), and `cgroup.other_count` says how many
//! labels that was. A member with no group (an exited container) has gaps until it
//! leaves, never zeros. A membership change bumps the series revision; the re-probe
//! keeps every group's counters.
//!
//! On a host whose `/sys/fs/cgroup` is cgroup v1 or hybrid (no `cgroup.controllers` at
//! the root) the collector is `Unsupported`.

use std::collections::HashMap;

use kelvo_schema::{
    Entitlement, Labels, MetricId, Module, SeriesKey, UnsupportedReason, bound_label_value,
};

use super::SysRoot;
use super::cap::{Cap, Id, OWN_MAX};
use super::cgroup_names::{Kind, Resolved, Unit, resolve, user_unit_name};
use super::cgroup_walk::{
    Counters, Rates, Walk, parse_bytes, parse_cpu_stat, parse_io_stat, parse_psi_some, rates,
};
use super::procfs::{kib, read_into};
use crate::{
    Cadence, CollectError, Collector, CollectorId, ContainerNames, Probe, SampleBuf, Tick,
};

const PERIOD_MS: u32 = 5_000;
const ROOT: &str = "/sys/fs/cgroup";
/// The `name` of the folded remainder.
pub const OTHER: &str = "_other";
const MINUTE_MS: i64 = 60_000;

/// What a group is labelled by.
enum What {
    Named(Kind, Box<str>),
    /// A container whose name is looked up; `pending` while a refresh is awaited.
    Container {
        kind: Kind,
        id: Box<str>,
        pending: Option<u64>,
    },
}

struct Group {
    what: What,
    label: Option<Id>,
    seen: u64,
    /// The counters and continuous time of the previous pass.
    prev: Option<(Counters, u64)>,
    mem_max: Option<u64>,
}

struct Keys {
    cpu: SeriesKey,
    mem: SeriesKey,
    mem_max: Option<SeriesKey>,
    io_read: SeriesKey,
    io_write: SeriesKey,
    mem_psi: Option<SeriesKey>,
}

impl Keys {
    fn new(kind: Kind, name: &str, limit: bool, psi: bool) -> Self {
        // Two distinct keys: `from_pairs` cannot fail.
        let labels =
            Labels::from_pairs([("kind", kind.as_str()), ("name", name)]).unwrap_or_default();
        let key = |id| SeriesKey::new(MetricId::from_static(id), labels.clone());
        Self {
            cpu: key("cgroup.cpu"),
            mem: key("cgroup.mem"),
            mem_max: limit.then(|| key("cgroup.mem_max")),
            io_read: key("cgroup.io_read"),
            io_write: key("cgroup.io_write"),
            mem_psi: psi.then(|| key("cgroup.mem_psi")),
        }
    }

    fn all(&self) -> impl Iterator<Item = &SeriesKey> {
        [Some(&self.cpu), Some(&self.mem), self.mem_max.as_ref()]
            .into_iter()
            .chain([
                Some(&self.io_read),
                Some(&self.io_write),
                self.mem_psi.as_ref(),
            ])
            .flatten()
    }
}

/// A sum over the groups of one label on one pass.
#[derive(Clone, Copy, Debug, Default)]
struct Sum {
    value: f64,
    any: bool,
    gap: bool,
}

impl Sum {
    fn add(&mut self, v: Option<f64>) {
        match v {
            Some(v) => {
                self.value += v;
                self.any = true;
            }
            None => self.gap = true,
        }
    }

    /// A member's value: a gap when any of its groups had none.
    fn whole(self) -> Option<f32> {
        (self.any && !self.gap).then_some(self.value as f32)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Agg {
    groups: u32,
    cpu: Sum,
    mem: Sum,
    io_read: Sum,
    io_write: Sum,
    /// The highest group's.
    psi: Option<f64>,
    /// Only for a label of exactly one group.
    mem_max: Option<u64>,
}

struct Label {
    kind: Kind,
    name: Box<str>,
    /// Groups with this label, as of the last walk.
    groups: u32,
    keys: Option<Keys>,
    agg: Agg,
}

/// The cgroup collector.
pub struct Cgroups {
    root: SysRoot,
    base: String,
    walk: Walk,
    names: ContainerNames,
    groups: HashMap<Box<str>, Group>,
    labels: HashMap<Id, Label>,
    index: HashMap<(Kind, Box<str>), Id>,
    next_id: Id,
    cap: Cap,
    scores: Vec<(Id, f64)>,
    pass: u64,
    minute: Option<i64>,
    path: String,
    buf: String,
    cpus: f64,
    mem_total: f64,
    supported: bool,
    revision: u64,
    keep: bool,
    other: Keys,
    other_count: SeriesKey,
}

impl Cgroups {
    pub fn new(names: ContainerNames) -> Self {
        Self::at(SysRoot::host(), names)
    }

    pub fn at(root: SysRoot, names: ContainerNames) -> Self {
        Self {
            base: root.path(ROOT).to_string_lossy().into_owned(),
            walk: Walk::new(&root.path(ROOT).to_string_lossy()),
            root,
            names,
            groups: HashMap::new(),
            labels: HashMap::new(),
            index: HashMap::new(),
            next_id: 0,
            cap: Cap::new(OWN_MAX),
            scores: Vec::new(),
            pass: 0,
            minute: None,
            path: String::new(),
            buf: String::new(),
            cpus: 1.0,
            mem_total: 1.0,
            supported: false,
            revision: 0,
            keep: false,
            other: Keys::new(Kind::Other, OTHER, false, false),
            other_count: SeriesKey::bare(MetricId::from_static("cgroup.other_count")),
        }
    }

    /// Walks the tree: adds new groups, forgets gone ones, names the unnamed.
    fn scan(&mut self) {
        self.pass += 1;
        let (pass, root, groups) = (self.pass, &self.root, &mut self.groups);
        self.walk.run(&mut |rel, unit| {
            if let Some(g) = groups.get_mut(rel) {
                g.seen = pass;
                return;
            }
            let what = match unit {
                Unit::Service(u) => What::Named(Kind::Service, bound_label_value(u).into()),
                Unit::User { uid, unit } => {
                    let name = user_unit_name(root, uid, unit);
                    What::Named(Kind::User, bound_label_value(&name).into())
                }
                Unit::Docker(id) | Unit::Podman(id) => What::Container {
                    kind: if matches!(unit, Unit::Docker(_)) {
                        Kind::Docker
                    } else {
                        Kind::Podman
                    },
                    id: id.into(),
                    pending: None,
                },
            };
            groups.insert(
                rel.into(),
                Group {
                    what,
                    label: None,
                    seen: pass,
                    prev: None,
                    mem_max: None,
                },
            );
        });
        let labels = &mut self.labels;
        self.groups.retain(|_, g| {
            let keep = g.seen == pass;
            if !keep && let Some(l) = g.label.and_then(|id| labels.get_mut(&id)) {
                l.groups = l.groups.saturating_sub(1);
            }
            keep
        });
        for g in self.groups.values_mut().filter(|g| g.label.is_none()) {
            let (kind, name) = match &mut g.what {
                What::Named(kind, name) => (*kind, name.clone()),
                What::Container { kind, id, pending } => match resolve(&self.names, id, *pending) {
                    Resolved::Named(name) => (*kind, bound_label_value(&name).into()),
                    Resolved::Pending(asked) => {
                        *pending = Some(asked);
                        continue;
                    }
                },
            };
            let id = *self.index.entry((kind, name.clone())).or_insert_with(|| {
                self.next_id += 1;
                self.next_id
            });
            let label = self.labels.entry(id).or_insert_with(|| Label {
                kind,
                name,
                groups: 0,
                keys: None,
                agg: Agg::default(),
            });
            label.groups += 1;
            g.label = Some(id);
        }
    }

    /// Reads every named group's files and sums them per label; `now_ns` is `None` on
    /// the probe, which reads but keeps no baseline.
    fn read(&mut self, now_ns: Option<u64>, limits: bool) {
        for l in self.labels.values_mut() {
            l.agg = Agg::default();
        }
        for (rel, g) in &mut self.groups {
            let Some(label) = g.label.and_then(|id| self.labels.get_mut(&id)) else {
                continue;
            };
            let mut file = |name: &str, buf: &mut String| {
                self.path.clear();
                self.path.push_str(&self.base);
                self.path.push('/');
                self.path.push_str(rel);
                self.path.push('/');
                self.path.push_str(name);
                read_into(&self.path, buf).ok()
            };
            let buf = &mut self.buf;
            let cur = Counters {
                usage_us: file("cpu.stat", buf).and_then(|()| parse_cpu_stat(buf)),
                mem: file("memory.current", buf).and_then(|()| parse_bytes(buf)),
                io: file("io.stat", buf).and_then(|()| parse_io_stat(buf)),
                psi_some_us: file("memory.pressure", buf).and_then(|()| parse_psi_some(buf)),
            };
            if limits || g.prev.is_none() {
                g.mem_max = file("memory.max", buf).and_then(|()| parse_bytes(buf));
            }
            let r = match (g.prev, now_ns) {
                (Some((prev, at)), Some(now)) if now > at => {
                    rates(&prev, &cur, (now - at) as f64 / 1e9)
                }
                _ => Rates::default(),
            };
            if let Some(now) = now_ns {
                g.prev = Some((cur, now));
            }
            let a = &mut label.agg;
            a.groups += 1;
            a.cpu.add(r.cpu_pct);
            a.mem.add(cur.mem.map(|m| m as f64));
            a.io_read.add(r.io_read);
            a.io_write.add(r.io_write);
            a.psi = match (a.psi, r.psi_pct) {
                (Some(x), Some(y)) => Some(x.max(y)),
                (x, y) => x.or(y),
            };
            a.mem_max = if a.groups == 1 { g.mem_max } else { None };
        }
        self.scores.clear();
        for (&id, l) in &self.labels {
            if l.agg.groups > 0 {
                let cpu = l.agg.cpu.value / (100.0 * self.cpus);
                let mem = l.agg.mem.value / self.mem_total;
                self.scores.push((id, cpu + mem));
            }
        }
    }

    /// Builds or drops keys to match the membership; true when a member's `mem_max`
    /// or `mem_psi` presence changed, which changes the series set too.
    fn sync_keys(&mut self) -> bool {
        let mut changed = false;
        for (&id, l) in &mut self.labels {
            if !self.cap.is_member(id) {
                l.keys = None;
                continue;
            }
            let limit = l.agg.mem_max.is_some();
            match &l.keys {
                // A member without a group keeps its keys: its series have gaps.
                Some(k) if l.agg.groups == 0 || k.mem_max.is_some() == limit => {}
                had => {
                    changed |= had.is_some();
                    l.keys = Some(Keys::new(l.kind, &l.name, limit, true));
                }
            }
        }
        changed
    }

    fn series(&self) -> Vec<SeriesKey> {
        let mut out: Vec<SeriesKey> = self
            .cap
            .members()
            .iter()
            .filter_map(|id| self.labels.get(id)?.keys.as_ref())
            .flat_map(|k| k.all().cloned())
            .collect();
        out.extend(self.other.all().cloned());
        out.push(self.other_count.clone());
        out
    }

    fn emit(&self, out: &mut SampleBuf) {
        let mut other = Agg::default();
        let mut folded = 0u32;
        for (&id, l) in &self.labels {
            let a = &l.agg;
            if a.groups == 0 {
                continue;
            }
            match l.keys.as_ref().filter(|_| self.cap.is_member(id)) {
                Some(k) => {
                    let mut put = |key: &SeriesKey, v: Option<f32>| {
                        if let Some(v) = v {
                            out.push(key, v);
                        }
                    };
                    put(&k.cpu, a.cpu.whole());
                    put(&k.mem, a.mem.whole());
                    if let Some(key) = &k.mem_max {
                        put(key, a.mem_max.map(|m| m as f32));
                    }
                    put(&k.io_read, a.io_read.whole());
                    put(&k.io_write, a.io_write.whole());
                    if let Some(key) = &k.mem_psi {
                        put(key, a.psi.map(|p| p as f32));
                    }
                }
                None => {
                    folded += 1;
                    // The fold sums what its groups measured; a group still without a
                    // baseline is left out rather than gapping everyone else.
                    for (sum, part) in [
                        (&mut other.cpu, a.cpu),
                        (&mut other.mem, a.mem),
                        (&mut other.io_read, a.io_read),
                        (&mut other.io_write, a.io_write),
                    ] {
                        if part.any {
                            sum.add(Some(part.value));
                        }
                    }
                }
            }
        }
        // Nothing folded is a measured zero: the fold is always in the series set.
        for (key, s) in [
            (&self.other.cpu, other.cpu),
            (&self.other.mem, other.mem),
            (&self.other.io_read, other.io_read),
            (&self.other.io_write, other.io_write),
        ] {
            out.push(key, s.value as f32);
        }
        out.push(&self.other_count, folded as f32);
    }

    /// Forgets labels with no group that are not members (once a minute).
    fn collect_garbage(&mut self) {
        let cap = &self.cap;
        let index = &mut self.index;
        self.labels.retain(|&id, l| {
            let keep = l.groups > 0 || cap.is_member(id);
            if !keep {
                index.remove(&(l.kind, l.name.clone()));
            }
            keep
        });
    }

    fn host_shares(&mut self) {
        let meminfo = self.root.read("/proc/meminfo").unwrap_or_default();
        // Without meminfo (a fixture tree) the score still ranks, on a nominal 16 GiB.
        self.mem_total = kib(&meminfo, "MemTotal").map_or(16.0 * (1u64 << 30) as f64, |b| b as f64);
        let stat = self.root.read("/proc/stat").unwrap_or_default();
        let cpus = stat
            .lines()
            .filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit))
            .count();
        self.cpus = cpus.max(1) as f64;
    }
}

impl Collector for Cgroups {
    fn id(&self) -> CollectorId {
        CollectorId("cgroups")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Every(PERIOD_MS)
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Containers]
    }

    fn probe(&mut self) -> Probe {
        if std::mem::take(&mut self.keep) {
            return Probe::Supported(self.series());
        }
        self.supported = self
            .root
            .path(&format!("{ROOT}/cgroup.controllers"))
            .is_file();
        if !self.supported {
            return Probe::Unsupported {
                reason: UnsupportedReason::NoHardware,
            };
        }
        self.host_shares();
        self.groups.clear();
        self.labels.clear();
        self.index.clear();
        self.minute = None;
        self.scan();
        self.read(None, true);
        self.cap.seed(&self.scores);
        self.sync_keys();
        Probe::Supported(self.series())
    }

    fn series_revision(&self) -> u64 {
        self.revision
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        if !self.supported {
            return Err(CollectError::NotProbed);
        }
        let minute = tick.wall_ms.div_euclid(MINUTE_MS);
        let boundary = self.minute.is_some_and(|m| m != minute);
        self.minute = Some(minute);
        self.scan();
        self.read(Some(tick.continuous_ns), boundary);
        let mut changed = self.cap.observe(tick.wall_ms, &self.scores);
        if changed || boundary {
            changed |= self.sync_keys();
            self.collect_garbage();
        }
        if changed {
            self.revision += 1;
            self.keep = true;
        }
        self.emit(out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::sync::Arc;

    use kelvo_schema::Catalog;

    use super::*;
    use crate::linux::testdata;

    const S: u64 = 1_000_000_000;
    const NGINX: &str = "ba1d8caf4daec08a0dfd56c5d1e80e270631bedc443634cc398d78deb1769013";
    const REDIS: &str = "47d70c0c2814b54a3bf9cfc143c8c4ec5632efd4732a4c9afa4f7d831daf270b";

    /// A tick at continuous `secs` and wall time `wall_s`.
    fn tick(secs: u64, wall_s: i64) -> Tick {
        Tick {
            n: 0,
            wall_ms: wall_s * 1000,
            continuous_ns: secs * S,
            interval_ms: 1_000,
        }
    }

    impl Cgroups {
        /// Reads the same groups from another tree (the fixture's next snapshot).
        fn repoint(&mut self, root: &SysRoot) {
            self.base = root.path(ROOT).to_string_lossy().into_owned();
            self.walk = Walk::new(&self.base);
        }
    }

    fn get(buf: &SampleBuf, key: &str) -> Option<f32> {
        buf.values()
            .iter()
            .find(|s| s.key.to_string() == key)
            .map(|s| s.value)
    }

    fn series(p: Probe) -> Vec<SeriesKey> {
        match p {
            Probe::Supported(s) => s,
            other => panic!("{other:?}"),
        }
    }

    fn names(pairs: &[(&str, &str)]) -> HashMap<String, Arc<str>> {
        pairs
            .iter()
            .map(|&(id, n)| (id.to_owned(), Arc::from(n)))
            .collect()
    }

    /// One synthetic group: `cpu.stat`, `memory.current`, `memory.max`, an empty
    /// `io.stat` and a zero `memory.pressure`.
    fn group(dir: &tempfile::TempDir, rel: &str, usage_us: u64, mem: u64, max: &str) {
        let base = format!("sys/fs/cgroup/{rel}");
        let files = [
            ("cpu.stat", format!("usage_usec {usage_us}\n")),
            ("memory.current", format!("{mem}\n")),
            ("memory.max", format!("{max}\n")),
            ("io.stat", String::new()),
            (
                "memory.pressure",
                "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n".to_owned(),
            ),
        ];
        for (name, text) in files {
            testdata::write(dir, &format!("{base}/{name}"), &text);
        }
    }

    fn tree() -> tempfile::TempDir {
        testdata::tree(&[("sys/fs/cgroup/cgroup.controllers", "cpu io memory pids\n")])
    }

    /// Container `n`'s scope; its short id is `<n + 0x100 in hex>000000000`.
    fn docker(n: usize) -> String {
        format!(
            "system.slice/docker-{:x}{}.scope",
            n + 0x100,
            "0".repeat(61)
        )
    }

    /// linux-tailscale, two snapshots 5 s apart. opendata-redis: usage_usec
    /// 33966266312 -> 33966317530 (+51218 us), rbytes +20480, wbytes +401408, PSI some
    /// total +8 us, memory.current 72843264 -> 67846144, memory.max 536870912. nginx:
    /// +181 us, no limit. cadvisor (59768832 B at t1) is not in the name map.
    #[test]
    fn fixture_values_from_two_snapshots() {
        let map = ContainerNames::attached();
        map.finish(Some(names(&[(NGINX, "nginx"), (REDIS, "opendata-redis")])));
        let mut c = Cgroups::at(testdata::cgroup_t0(), map.clone());
        let listed = series(c.probe());
        let labels: BTreeSet<String> = listed
            .iter()
            .filter(|k| k.metric.as_str() == "cgroup.cpu")
            .map(|k| {
                let l = |n| k.labels.get(n).unwrap_or_default();
                format!("{}/{}", l("kind"), l("name"))
            })
            .collect();
        assert_eq!(
            labels,
            BTreeSet::from(
                [
                    "docker/nginx",
                    "docker/opendata-redis",
                    "other/_other",
                    "service/docker.service",
                    "service/getty@tty1.service",
                    "service/sshd.service",
                    "user/0/dbus-broker.service",
                ]
                .map(String::from)
            ),
            "cadvisor waits for a refresh; sessions and init.scope are not groups"
        );
        assert!(map.requested(), "the unknown id asked for a refresh");
        assert!(Catalog::builtin().over_cap(&listed).is_empty());
        let limits: Vec<String> = listed
            .iter()
            .filter(|k| k.metric.as_str() == "cgroup.mem_max")
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            limits,
            ["cgroup.mem_max{kind=docker,name=opendata-redis}"],
            "mem_max only where memory.max is a number"
        );

        let mut buf = SampleBuf::new();
        c.sample(&tick(10, 600), &mut buf).unwrap();
        let redis = |m: &str| format!("cgroup.{m}{{kind=docker,name=opendata-redis}}");
        assert_eq!(get(&buf, &redis("cpu")), None, "baseline");
        assert_eq!(get(&buf, &redis("mem")), Some(72_843_264.0));
        assert_eq!(get(&buf, &redis("mem_max")), Some(536_870_912.0));

        // The refresh did not find cadvisor: its short id from now on.
        map.finish(Some(names(&[(NGINX, "nginx"), (REDIS, "opendata-redis")])));
        c.repoint(&testdata::cgroup_t1());
        buf.clear();
        c.sample(&tick(15, 605), &mut buf).unwrap();
        let close = |buf: &SampleBuf, k: &str, want: f64| {
            let got = f64::from(get(buf, k).unwrap_or_else(|| panic!("{k} missing")));
            assert!(
                (got - want).abs() <= want.abs() * 1e-5,
                "{k}: {got} vs {want}"
            );
        };
        close(&buf, &redis("cpu"), 51_218.0 / 5e6 * 100.0);
        close(&buf, &redis("mem"), 67_846_144.0);
        close(&buf, &redis("io_read"), 20_480.0 / 5.0);
        close(&buf, &redis("io_write"), 401_408.0 / 5.0);
        close(&buf, &redis("mem_psi"), 8.0 / 5e6 * 100.0);
        close(
            &buf,
            "cgroup.cpu{kind=docker,name=nginx}",
            181.0 / 5e6 * 100.0,
        );
        close(&buf, "cgroup.io_read{kind=service,name=sshd.service}", 0.0);
        // No io controller below user@0.service: absent, not zero.
        assert_eq!(
            get(&buf, "cgroup.io_read{kind=user,name=0/dbus-broker.service}"),
            None
        );
        close(
            &buf,
            "cgroup.mem{kind=user,name=0/dbus-broker.service}",
            1_556_480.0,
        );
        // cadvisor has its short id now but joins only at the next minute: folded.
        close(&buf, "cgroup.other_count", 1.0);
        close(&buf, "cgroup.mem{kind=other,name=_other}", 59_768_832.0);
        close(&buf, "cgroup.cpu{kind=other,name=_other}", 0.0);
        assert_eq!(c.series_revision(), 0);

        // At the minute boundary it joins: a new revision, and the re-probe lists it.
        buf.clear();
        c.sample(&tick(20, 660), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 1);
        let after = series(c.probe());
        assert!(
            after
                .iter()
                .any(|k| k.to_string() == "cgroup.cpu{kind=docker,name=b2bca07ec6d2}")
        );
        // Baselines survive the revision's re-probe.
        buf.clear();
        c.sample(&tick(25, 665), &mut buf).unwrap();
        close(&buf, &redis("cpu"), 0.0);
        close(&buf, "cgroup.cpu{kind=docker,name=b2bca07ec6d2}", 0.0);
        close(&buf, "cgroup.other_count", 0.0);
    }

    /// A counter that went backwards (a container restarted under the same id, so its
    /// scope is new) gives no rate rather than a huge or negative one.
    #[test]
    fn counters_that_go_back_give_no_rate() {
        let prev = Counters {
            usage_us: Some(5_000_000),
            mem: Some(1),
            io: Some((1000, 1000)),
            psi_some_us: Some(900),
        };
        let cur = Counters {
            usage_us: Some(1_000),
            mem: Some(1),
            io: Some((10, 3000)),
            psi_some_us: Some(1_000),
        };
        let r = rates(&prev, &cur, 2.0);
        assert_eq!(r.cpu_pct, None);
        assert_eq!(r.io_read, None);
        assert_eq!(r.io_write, Some(1000.0));
        assert_eq!(r.psi_pct, Some(100.0 / 2e6 * 100.0));
        let gone = Counters::default();
        assert_eq!(
            rates(&prev, &gone, 2.0),
            Rates::default(),
            "unreadable files"
        );
    }

    #[test]
    fn cgroup_v1_is_unsupported() {
        let dir = testdata::tree(&[
            ("sys/fs/cgroup/cpu,cpuacct/cpuacct.usage", "1\n"),
            ("sys/fs/cgroup/memory/memory.usage_in_bytes", "1\n"),
            ("sys/fs/cgroup/unified/cgroup.controllers", "\n"),
        ]);
        let mut c = Cgroups::at(SysRoot::at(dir.path()), ContainerNames::default());
        assert_eq!(
            c.probe(),
            Probe::Unsupported {
                reason: UnsupportedReason::NoHardware
            }
        );
        assert!(c.sample(&tick(1, 1), &mut SampleBuf::new()).is_err());
        let mut none = Cgroups::at(SysRoot::at("/nonexistent"), ContainerNames::default());
        assert!(matches!(none.probe(), Probe::Unsupported { .. }));
    }

    /// 40 services: the 31 largest keep series, 9 fold into `_other` with their sums,
    /// and every metric stays within the catalog's cap.
    #[test]
    fn the_rest_fold_into_other() {
        let dir = tree();
        for i in 0..40u64 {
            let max = if i == 39 { "1048576" } else { "max" };
            let rel = format!("system.slice/s{i:02}.service");
            group(&dir, &rel, 0, (i + 1) << 20, max);
        }
        let mut c = Cgroups::at(SysRoot::at(dir.path()), ContainerNames::default());
        let listed = series(c.probe());
        assert!(Catalog::builtin().over_cap(&listed).is_empty());
        let cpu: Vec<&str> = listed
            .iter()
            .filter(|k| k.metric.as_str() == "cgroup.cpu")
            .filter_map(|k| k.labels.get("name"))
            .collect();
        assert_eq!(cpu.len(), 32);
        assert!(cpu.contains(&OTHER));
        assert!(!cpu.contains(&"s08.service"), "the 9 smallest fold");
        assert!(cpu.contains(&"s09.service"));
        assert!(!listed.iter().any(|k| k.labels.get("name") == Some(OTHER)
            && matches!(k.metric.as_str(), "cgroup.mem_max" | "cgroup.mem_psi")));

        let mut buf = SampleBuf::new();
        c.sample(&tick(1, 1), &mut buf).unwrap();
        // s00..s08 hold 1..9 MiB.
        assert_eq!(
            get(&buf, "cgroup.mem{kind=other,name=_other}"),
            Some((45u64 << 20) as f32)
        );
        assert_eq!(get(&buf, "cgroup.other_count"), Some(9.0));
        assert_eq!(
            get(&buf, "cgroup.cpu{kind=other,name=_other}"),
            Some(0.0),
            "no rates yet"
        );
        assert_eq!(
            get(&buf, "cgroup.mem_max{kind=service,name=s39.service}"),
            Some(1_048_576.0)
        );
        // 0.5 s of CPU in a folded group and 0.25 s in a member, over 5 s.
        group(&dir, "system.slice/s00.service", 500_000, 1 << 20, "max");
        group(&dir, "system.slice/s20.service", 250_000, 21 << 20, "max");
        buf.clear();
        c.sample(&tick(6, 6), &mut buf).unwrap();
        assert_eq!(get(&buf, "cgroup.cpu{kind=other,name=_other}"), Some(10.0));
        assert_eq!(
            get(&buf, "cgroup.cpu{kind=service,name=s20.service}"),
            Some(5.0)
        );
    }

    /// A container that exits leaves gaps in its series, never zeros, until it has had
    /// its extra minute; then the series set drops it.
    #[test]
    fn an_exited_container_is_a_gap_then_leaves() {
        let dir = tree();
        group(&dir, &docker(1), 1_000, 4 << 20, "max");
        group(&dir, &docker(2), 1_000, 8 << 20, "max");
        let mut c = Cgroups::at(SysRoot::at(dir.path()), ContainerNames::default());
        let before = series(c.probe());
        let key = |m: &str| format!("cgroup.{m}{{kind=docker,name=101000000000}}");
        assert!(before.iter().any(|k| k.to_string() == key("mem")));
        let mut buf = SampleBuf::new();
        c.sample(&tick(1, 30), &mut buf).unwrap();
        assert_eq!(get(&buf, &key("mem")), Some((4u32 << 20) as f32));
        std::fs::remove_dir_all(dir.path().join(format!("sys/fs/cgroup/{}", docker(1)))).unwrap();
        for (secs, wall) in [(6, 35), (11, 60), (16, 65)] {
            buf.clear();
            c.sample(&tick(secs, wall), &mut buf).unwrap();
            for m in ["cpu", "mem", "io_read", "io_write", "mem_psi"] {
                assert_eq!(get(&buf, &key(m)), None, "{m} at {wall} s");
            }
            assert_eq!(get(&buf, "cgroup.other_count"), Some(0.0));
            assert_eq!(
                get(&buf, "cgroup.mem{kind=docker,name=102000000000}"),
                Some((8u32 << 20) as f32)
            );
        }
        assert_eq!(c.series_revision(), 0, "kept its extra minute");
        buf.clear();
        c.sample(&tick(21, 120), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 1);
        let after: Vec<String> = series(c.probe()).iter().map(ToString::to_string).collect();
        assert!(!after.iter().any(|k| k.contains("101000000000")));
        assert!(after.contains(&"cgroup.mem{kind=docker,name=102000000000}".to_owned()));
    }

    #[test]
    #[ignore = "reads the live cgroup tree; run by hand on Linux"]
    fn live_pass() {
        let mut c = Cgroups::new(ContainerNames::default());
        let listed = series(c.probe());
        let mut buf = SampleBuf::new();
        c.sample(&tick(1, 1), &mut buf).unwrap();
        let started = std::time::Instant::now();
        for i in 0..10 {
            buf.clear();
            c.sample(&tick(2 + i, 2), &mut buf).unwrap();
        }
        println!(
            "[perf] live: {} groups, {} series, {:?}/pass",
            c.groups.len(),
            listed.len(),
            started.elapsed() / 10
        );
    }

    /// 200 containers: one pass reads them all within the budget.
    #[test]
    fn a_pass_over_200_groups_is_fast() {
        let dir = tree();
        for i in 0..200 {
            group(
                &dir,
                &docker(i),
                i as u64 * 1000,
                (i as u64 + 1) << 20,
                "max",
            );
        }
        let mut c = Cgroups::at(SysRoot::at(dir.path()), ContainerNames::default());
        let listed = series(c.probe());
        assert!(Catalog::builtin().over_cap(&listed).is_empty());
        let mut buf = SampleBuf::new();
        c.sample(&tick(1, 1), &mut buf).unwrap();
        let passes = 20u32;
        let started = std::time::Instant::now();
        for i in 0..passes {
            buf.clear();
            let t = 2 + u64::from(i) * 5;
            c.sample(&tick(t, t as i64), &mut buf).unwrap();
        }
        let per_pass = started.elapsed() / passes;
        println!("[perf] cgroup pass over 200 groups: {per_pass:?}");
        assert_eq!(get(&buf, "cgroup.other_count"), Some(169.0));
        // A debug build on a container filesystem; the release figure is measured on
        // linux-tailscale.
        assert!(
            per_pass < std::time::Duration::from_millis(50),
            "{per_pass:?}"
        );
    }
}
