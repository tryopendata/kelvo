//! Per-process rows from `/proc/<pid>/{stat,status,io}`.
//!
//! Each sample lists `/proc`'s pids and reads `/proc/<pid>/stat` for the name, CPU time,
//! thread count, start time and resident set, and `/proc/<pid>/io` for disk bytes. A
//! process is keyed by `(pid, starttime)`, so a reused pid starts fresh. As on macOS, a
//! process's first sample has no previous CPU time, so its row waits for the next one.
//! A pid that exits between the listing and the reads (ENOENT, ESRCH) is skipped.
//!
//! Once per process, when first seen: the owner from `status` (effective uid, named
//! through `/etc/passwd`) and, when `comm` may be truncated (the kernel keeps 15 bytes),
//! the basename of `cmdline`'s argv0 if it extends `comm`.
//!
//! - Memory is the resident set, `stat` field 24 times the page size (`VmRSS`).
//! - Disk is `read_bytes`/`write_bytes`, the bytes that reached the block layer. Only
//!   root (or `CAP_SYS_PTRACE`) reads other users' `io`. The probe tries pid 1's, owned
//!   by root: when that fails, `Capabilities::process_disk_io` is false and the UI hides
//!   the disk columns, so most rows' unknown rates never show as 0 B/s (D-116). Rows
//!   still carry 0 for an unreadable `io`, since `ProcessSample` has no "unknown" for
//!   disk; with the capability true that is only a process that exited mid-sample.
//!   [`Visibility::io_readable`] counts the rows that were measured.
//! - Energy and idle wakeups have no Linux source; `fill_capabilities` says so and the
//!   rows carry 0.
//! - `app` is the unit or container the process runs in (D-116), from
//!   `/proc/<pid>/cgroup` read once per process: `nginx.service` for a service (system
//!   or user manager), `docker:<name>` and `podman:<name>` for containers, named through
//!   [`ContainerNames`] (else the short id; a name being looked up is waited for).
//!   Kind-qualified, so a container never merges with a host process of the same name.
//!   Sessions, slices and `init.scope` give `None`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io;
use std::sync::Arc;

use kelvo_schema::Entitlement;

use super::SysRoot;
use super::cgroup_names::{Kind, Resolved, Unit, classify, proc_cgroup_path, resolve};
use super::procfs::{PidDir, field, read_into, user_name, word};
use crate::{
    Cadence, CollectError, Collector, CollectorId, ContainerNames, Interest, Probe, ProcessSample,
    SampleBuf, Tick,
};

/// The kernel's `TASK_COMM_LEN` minus the NUL: a `comm` this long may be cut short.
const COMM_MAX: usize = 15;

/// The `stat` fields a row needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stat<'a> {
    comm: &'a str,
    /// utime + stime, clock ticks.
    cpu_ticks: u64,
    threads: u32,
    /// Clock ticks after boot.
    start_ticks: u64,
    rss_pages: u64,
}

/// Parses `/proc/<pid>/stat`. `comm` may hold spaces and parentheses, so the fields
/// start after the last `)`.
fn parse_stat(text: &str) -> Option<Stat<'_>> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let comm = text.get(open + 1..close)?;
    // Fields from 3 (state) on; index i here is field i + 3.
    let mut f = text.get(close + 1..)?.split_ascii_whitespace();
    let mut nth = |skip: usize| f.nth(skip)?.parse::<u64>().ok();
    let utime = nth(11)?; // field 14
    let stime = nth(0)?; // 15
    let threads = nth(4)?; // 20
    let start_ticks = nth(1)?; // 22
    let rss_pages = nth(1)?; // 24
    Some(Stat {
        comm,
        cpu_ticks: utime.saturating_add(stime),
        threads: u32::try_from(threads).unwrap_or(u32::MAX),
        start_ticks,
        rss_pages,
    })
}

/// `read_bytes` and `write_bytes` from `/proc/<pid>/io`.
fn parse_io(text: &str) -> Option<(u64, u64)> {
    Some((
        field(text, "read_bytes")?.parse().ok()?,
        field(text, "write_bytes")?.parse().ok()?,
    ))
}

/// The effective uid from `/proc/<pid>/status`.
fn parse_euid(status: &str) -> Option<u32> {
    field(status, "Uid")?
        .split_ascii_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// The display name: `comm`, or argv0's basename when `comm` was cut short and argv0
/// starts with it (`systemd-journald` for `systemd-journal`).
fn display_name(comm: &str, cmdline: &[u8]) -> String {
    if comm.len() >= COMM_MAX {
        let argv0 = cmdline.split(|&b| b == 0).next().unwrap_or_default();
        let base = argv0.rsplit(|&b| b == b'/').next().unwrap_or_default();
        if let Ok(base) = std::str::from_utf8(base)
            && base.len() > comm.len()
            && base.starts_with(comm)
        {
            return base.to_owned();
        }
    }
    comm.to_owned()
}

#[derive(Clone, Copy)]
struct Prev {
    cpu_ticks: u64,
    /// `None` when `io` was unreadable.
    io: Option<(u64, u64)>,
    at_ns: u64,
}

/// A process's `app`, or the container whose name is being looked up.
enum App {
    Known(Option<Arc<str>>),
    Pending {
        kind: Kind,
        id: Box<str>,
        asked: u64,
    },
}

struct Known {
    name: Arc<str>,
    user: Arc<str>,
    app: App,
    /// `io` was refused (another user's process, not root): not tried again.
    io_denied: bool,
    prev: Option<Prev>,
}

/// How many processes the last sample saw and could read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Visibility {
    /// Every pid listed in `/proc`.
    pub listed: u32,
    /// Processes whose `stat` was read (the rest exited mid-sample).
    pub readable: u32,
    /// Readable processes whose `io` was read too.
    pub io_readable: u32,
}

pub struct Processes {
    last_usage_ns: Option<u64>,
    root: SysRoot,
    dir: Option<PidDir>,
    proc_base: String,
    page_size: u64,
    clk_tck: u64,
    btime_us: i64,
    pids: Vec<i32>,
    path: String,
    buf: String,
    known: HashMap<(i32, u64), Known>,
    seen: HashMap<(i32, u64), Known>,
    users: HashMap<u32, Arc<str>>,
    /// `app` values, shared by every process of one unit.
    apps: HashMap<String, Arc<str>>,
    app_buf: String,
    names: ContainerNames,
    visibility: Visibility,
    /// The probe could read another user's `io` (pid 1's).
    disk_io: bool,
}

impl Processes {
    /// The live host; containers are named through `names`.
    pub fn new(names: ContainerNames) -> Self {
        let mut p = Self::at(SysRoot::host(), super::page_size(), super::clk_tck());
        p.names = names;
        p
    }

    /// Reads the tree under `root` with the given page size and `USER_HZ` (tests: the
    /// fixture host's, not this machine's).
    pub fn at(root: SysRoot, page_size: u64, clk_tck: u64) -> Self {
        Self {
            dir: PidDir::new(&root),
            proc_base: root.path("/proc").to_string_lossy().into_owned(),
            root,
            page_size,
            clk_tck: clk_tck.max(1),
            btime_us: 0,
            pids: Vec::new(),
            path: String::new(),
            buf: String::new(),
            known: HashMap::new(),
            seen: HashMap::new(),
            users: HashMap::new(),
            apps: HashMap::new(),
            app_buf: String::new(),
            names: ContainerNames::default(),
            visibility: Visibility::default(),
            last_usage_ns: None,
            disk_io: false,
        }
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    /// Reads `/proc/<pid>/<file>` into `self.buf`.
    fn read_pid(&mut self, pid: i32, file: &str) -> io::Result<()> {
        self.path.clear();
        // Writing to a String cannot fail.
        let _ = write!(self.path, "{}/{pid}/{file}", self.proc_base);
        read_into(&self.path, &mut self.buf)
    }

    /// Name and owner of a process seen for the first time.
    fn identify(&mut self, pid: i32, comm: &str) -> Known {
        let cmdline = if comm.len() >= COMM_MAX {
            self.path.clear();
            let _ = write!(self.path, "{}/{pid}/cmdline", self.proc_base);
            std::fs::read(&self.path).unwrap_or_default()
        } else {
            Vec::new()
        };
        let name = display_name(comm, &cmdline);
        let uid = self
            .read_pid(pid, "status")
            .ok()
            .and_then(|()| parse_euid(&self.buf));
        let user = match uid {
            Some(uid) => self
                .users
                .entry(uid)
                .or_insert_with(|| user_name(&self.root, uid).into())
                .clone(),
            None => Arc::from("?"),
        };
        let app = self.app_of(pid);
        Known {
            name: Arc::from(name),
            user,
            app,
            io_denied: false,
            prev: None,
        }
    }

    /// The shared `app` value `<prefix><name>`.
    fn app_value(&mut self, prefix: &str, name: &str) -> Arc<str> {
        self.app_buf.clear();
        self.app_buf.push_str(prefix);
        self.app_buf.push_str(name);
        if let Some(app) = self.apps.get(self.app_buf.as_str()) {
            return Arc::clone(app);
        }
        if self.apps.len() >= 4096 {
            self.apps.clear();
        }
        let app: Arc<str> = Arc::from(self.app_buf.as_str());
        self.apps.insert(self.app_buf.clone(), Arc::clone(&app));
        app
    }

    /// What `/proc/<pid>/cgroup` says the process belongs to.
    fn app_of(&mut self, pid: i32) -> App {
        if self.read_pid(pid, "cgroup").is_err() {
            return App::Known(None);
        }
        let text = std::mem::take(&mut self.buf);
        let unit = proc_cgroup_path(&text).and_then(classify);
        let app = match unit {
            None => App::Known(None),
            Some(Unit::Service(u) | Unit::User { unit: u, .. }) => {
                App::Known(Some(self.app_value("", u)))
            }
            Some(Unit::Docker(id)) => self.container(Kind::Docker, id, None),
            Some(Unit::Podman(id)) => self.container(Kind::Podman, id, None),
        };
        self.buf = text;
        app
    }

    /// A container's `app`, once its name is known.
    fn container(&mut self, kind: Kind, id: &str, asked: Option<u64>) -> App {
        match resolve(&self.names, id, asked) {
            Resolved::Named(name) => {
                let prefix = if kind == Kind::Docker {
                    "docker:"
                } else {
                    "podman:"
                };
                App::Known(Some(self.app_value(prefix, &name)))
            }
            Resolved::Pending(asked) => App::Pending {
                kind,
                id: id.into(),
                asked,
            },
        }
    }

    /// One process: its row (when it has a previous sample) into `out`, its state into
    /// `self.seen`. False when it exited before its `stat` could be read.
    fn sample_pid(&mut self, pid: i32, now: u64, out: &mut SampleBuf) -> bool {
        if self.read_pid(pid, "stat").is_err() {
            return false;
        }
        let Some(st) = parse_stat(&self.buf) else {
            return false;
        };
        let (cpu_ticks, threads, start, rss) =
            (st.cpu_ticks, st.threads, st.start_ticks, st.rss_pages);
        let key = (pid, start);
        let mut known = match self.known.remove(&key) {
            Some(k) => k,
            None => {
                let comm = st.comm.to_owned();
                self.identify(pid, &comm)
            }
        };
        let io = if known.io_denied {
            None
        } else {
            match self.read_pid(pid, "io") {
                Ok(()) => parse_io(&self.buf),
                Err(e) => {
                    known.io_denied = e.kind() == io::ErrorKind::PermissionDenied;
                    None
                }
            }
        };
        if io.is_some() {
            self.visibility.io_readable += 1;
        }
        let cur = Prev {
            cpu_ticks,
            io,
            at_ns: now,
        };
        if let App::Pending { kind, id, asked } = &known.app {
            known.app = self.container(*kind, id, Some(*asked));
        }
        if let Some(prev) = known.prev
            && now > prev.at_ns
        {
            let secs = (now - prev.at_ns) as f64 / 1e9;
            let cpu_s = cpu_ticks.saturating_sub(prev.cpu_ticks) as f64 / self.clk_tck as f64;
            let (read_bps, write_bps) = match (prev.io, io) {
                (Some((r0, w0)), Some((r1, w1))) => (
                    r1.saturating_sub(r0) as f64 / secs,
                    w1.saturating_sub(w0) as f64 / secs,
                ),
                _ => (0.0, 0.0),
            };
            let start_us = self.btime_us.saturating_add(
                i64::try_from(start.saturating_mul(1_000_000) / self.clk_tck).unwrap_or(0),
            );
            out.push_process(ProcessSample {
                pid,
                start_time_us: start_us,
                name: known.name.clone(),
                cpu_pct: (cpu_s / secs * 100.0) as f32,
                mem_bytes: rss.saturating_mul(self.page_size),
                compressed_bytes: None,
                threads,
                idle_wakeups_per_s: 0.0,
                energy: 0.0,
                energy_j: 0.0,
                interval_s: secs as f32,
                app: match &known.app {
                    App::Known(app) => app.clone(),
                    App::Pending { .. } => None,
                },
                app_main: false,
                disk_read_bps: read_bps as f32,
                disk_write_bps: write_bps as f32,
                net_rx_bps: None,
                net_tx_bps: None,
                gpu_pct: None,
                ports: None,
                user: known.user.clone(),
            });
        }
        known.prev = Some(cur);
        self.seen.insert(key, known);
        true
    }
}

impl Collector for Processes {
    fn id(&self) -> CollectorId {
        CollectorId("processes")
    }

    fn cadence(&self) -> Cadence {
        Cadence::Adaptive {
            idle_ms: crate::IDLE_MS,
            interest: Interest::Processes,
        }
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn probe(&mut self) -> Probe {
        self.last_usage_ns = None;
        self.known.clear();
        let btime = self
            .root
            .read("/proc/stat")
            .ok()
            .and_then(|s| word(&s, "btime"));
        let listed = self.dir.as_ref().is_some_and(|d| d.list(&mut self.pids));
        self.disk_io = self.read_pid(1, "io").is_ok() && parse_io(&self.buf).is_some();
        match btime {
            Some(btime) if listed => {
                self.btime_us = i64::try_from(btime).unwrap_or(0).saturating_mul(1_000_000);
                // Process rows are not series.
                Probe::Supported(Vec::new())
            }
            _ => Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            },
        }
    }

    fn fill_capabilities(&self, caps: &mut kelvo_schema::Capabilities) {
        caps.process_energy = false;
        caps.process_wakeups = false;
        caps.process_network = false;
        caps.process_gpu = false;
        caps.process_disk_io = self.disk_io;
    }

    fn release(&mut self) {
        self.last_usage_ns = None;
        self.known.clear();
        self.seen.clear();
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let listed = self.dir.as_ref().is_some_and(|d| d.list(&mut self.pids));
        if !listed {
            return Err(CollectError::Os {
                call: "opendir /proc",
                code: i64::from(io::Error::last_os_error().raw_os_error().unwrap_or(0)),
            });
        }
        let now = tick.continuous_ns;
        let span_ms = self
            .last_usage_ns
            .replace(now)
            .map_or(0, |prev| now.saturating_sub(prev) / 1_000_000);
        self.visibility = Visibility {
            listed: u32::try_from(self.pids.len()).unwrap_or(u32::MAX),
            ..Visibility::default()
        };
        self.seen.clear();
        for i in 0..self.pids.len() {
            let Some(&pid) = self.pids.get(i) else {
                continue;
            };
            if self.sample_pid(pid, now, out) {
                self.visibility.readable += 1;
            }
        }
        // Processes not seen this time have exited; dropping them forgets them.
        std::mem::swap(&mut self.known, &mut self.seen);
        out.observe_processes(
            span_ms,
            self.visibility.listed,
            self.visibility.readable,
            false,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::testdata;
    use super::*;

    const S: u64 = 1_000_000_000;

    fn tick(at_ns: u64) -> Tick {
        Tick {
            n: 0,
            wall_ms: 0,
            continuous_ns: at_ns,
            interval_ms: 1_000,
        }
    }

    fn fixture(rel: &str) -> String {
        testdata::asahi_t0().read(rel).unwrap()
    }

    /// The fixture's pid 1 (systemd) with its CPU and io counters moved by `cpu` ticks
    /// and `bytes`.
    fn pid1(dir: &tempfile::TempDir, cpu: u64, bytes: u64) {
        let stat = fixture("/proc/1/stat").replacen(
            " 496368 373557 ",
            &format!(" {} 373557 ", 496_368 + cpu),
            1,
        );
        testdata::write(dir, "proc/1/stat", &stat);
        let io = fixture("/proc/1/io").replace(
            "read_bytes: 179834478932480",
            &format!("read_bytes: {}", 179_834_478_932_480 + bytes),
        );
        testdata::write(dir, "proc/1/io", &io);
    }

    fn host() -> tempfile::TempDir {
        let dir = testdata::tree(&[
            ("proc/stat", "cpu  1 1 1 1\nbtime 1788630414\n"),
            ("proc/1/status", &fixture("/proc/1/status")),
            ("etc/passwd", "root:x:0:0:root:/root:/bin/zsh\n"),
        ]);
        pid1(&dir, 0, 0);
        dir
    }

    #[test]
    fn parses_the_fixture_stat() {
        let text = fixture("/proc/1/stat");
        let st = parse_stat(&text).unwrap();
        assert_eq!(
            st,
            Stat {
                comm: "systemd",
                cpu_ticks: 496_368 + 373_557,
                threads: 1,
                start_ticks: 0,
                rss_pages: 1124,
            }
        );
        assert_eq!(
            parse_io(&fixture("/proc/1/io")),
            Some((179_834_478_932_480, 6_836_233_699_328))
        );
        assert_eq!(parse_euid(&fixture("/proc/1/status")), Some(0));
        // A comm with spaces and parentheses.
        let st = parse_stat("7 (a) (b c) S 0 0 0 0 0 0 0 0 0 0 5 6 0 0 0 0 3 0 99 0 8").unwrap();
        assert_eq!((st.comm, st.cpu_ticks, st.threads), ("a) (b c", 11, 3));
        assert_eq!((st.start_ticks, st.rss_pages), (99, 8));
        assert_eq!(parse_stat("7 (x) S 0 0"), None, "truncated");
        assert_eq!(parse_stat(""), None);
    }

    #[test]
    fn rows_from_two_samples() {
        let dir = host();
        let mut p = Processes::at(SysRoot::at(dir.path()), 16_384, 100);
        assert_eq!(p.probe(), Probe::Supported(Vec::new()));
        let mut buf = SampleBuf::new();
        p.sample(&tick(10 * S), &mut buf).unwrap();
        assert!(
            buf.processes().is_empty(),
            "first sample of a process: no row"
        );

        assert!(
            buf.usage_observations()[0]
                .as_ref()
                .unwrap()
                .causes
                .contains(&Some(
                    kelvo_schema::durable_usage::UsageIncompleteCause::NoBaseline
                ))
        );
        // 100 ticks of CPU (1 s at USER_HZ 100) and 4 MB read over 2 s.
        pid1(&dir, 100, 4_000_000);
        buf.clear();
        p.sample(&tick(12 * S), &mut buf).unwrap();
        let [row] = buf.processes() else {
            panic!("{:?}", buf.processes())
        };
        assert_eq!(row.pid, 1);
        assert_eq!(&*row.name, "systemd");
        assert_eq!(&*row.user, "root");
        assert!((row.cpu_pct - 50.0).abs() < 1e-4, "{}", row.cpu_pct);
        assert_eq!(row.mem_bytes, 1124 * 16_384);
        assert_eq!(row.threads, 1);
        assert_eq!(row.disk_read_bps, 2_000_000.0);
        assert_eq!(row.disk_write_bps, 0.0);
        assert_eq!(row.start_time_us, 1_788_630_414_000_000);
        assert_eq!(row.interval_s, 2.0);
        let observation = buf.usage_observations()[0].as_ref().unwrap();
        assert_eq!(observation.span_ms, 2000);
        assert!(!observation.causes.contains(&Some(
            kelvo_schema::durable_usage::UsageIncompleteCause::NoBaseline
        )));
        assert!(observation.causes.contains(&Some(
            kelvo_schema::durable_usage::UsageIncompleteCause::UnresolvedIdentity
        )));
        assert!(
            buf.usage_observations()[3].is_none(),
            "Linux process energy is unsupported"
        );
        assert_eq!(row.app, None);
        assert_eq!(
            p.visibility(),
            Visibility {
                listed: 1,
                readable: 1,
                io_readable: 1
            }
        );
    }

    #[test]
    fn vanished_pids_and_missing_io() {
        let dir = host();
        // 99 is listed but exited before its stat was read; 2 has no io file.
        std::fs::create_dir_all(dir.path().join("proc/99")).unwrap();
        let stat2 = "2 (kthreadd) S 0 0 0 0 -1 0 0 0 0 0 7 3 0 0 20 0 1 0 5 0 0";
        testdata::write(&dir, "proc/2/stat", stat2);
        testdata::write(&dir, "proc/2/status", "Name:\tkthreadd\nUid:\t0\t0\t0\t0\n");
        let mut p = Processes::at(SysRoot::at(dir.path()), 4096, 100);
        assert!(matches!(p.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        p.sample(&tick(S), &mut buf).unwrap();
        buf.clear();
        p.sample(&tick(2 * S), &mut buf).unwrap();
        let mut pids: Vec<_> = buf.processes().iter().map(|r| r.pid).collect();
        pids.sort_unstable();
        assert_eq!(pids, [1, 2]);
        let row2 = buf.processes().iter().find(|r| r.pid == 2).unwrap();
        assert_eq!((row2.disk_read_bps, row2.disk_write_bps), (0.0, 0.0));
        assert_eq!(
            p.visibility(),
            Visibility {
                listed: 3,
                readable: 2,
                io_readable: 1
            }
        );

        // pid 2 is reused between two samples by a process with a later start and less
        // CPU time: a new key, so a baseline rather than a row with a bogus delta.
        testdata::write(
            &dir,
            "proc/2/stat",
            &stat2
                .replacen(" 7 3 ", " 1 0 ", 1)
                .replacen(" 5 0 0", " 700 0 0", 1),
        );
        buf.clear();
        p.sample(&tick(5 * S / 2), &mut buf).unwrap();
        assert!(
            buf.processes().iter().all(|r| r.pid != 2),
            "reused pid: baseline"
        );

        // pid 1 exits: forgotten. A new process takes pid 1 (a later start): it is a new
        // key and waits a sample for its row.
        std::fs::remove_file(dir.path().join("proc/1/stat")).unwrap();
        buf.clear();
        p.sample(&tick(3 * S), &mut buf).unwrap();
        assert!(buf.processes().iter().all(|r| r.pid != 1));
        assert!(!p.known.keys().any(|k| k.0 == 1));
        testdata::write(
            &dir,
            "proc/1/stat",
            stat2
                .replacen("2 (kthreadd)", "1 (init)", 1)
                .replacen(" 5 0 0", " 900 0 0", 1)
                .as_str(),
        );
        buf.clear();
        p.sample(&tick(4 * S), &mut buf).unwrap();
        assert!(
            buf.processes().iter().all(|r| r.pid != 1),
            "new process: baseline"
        );
    }

    #[test]
    fn truncated_comm_takes_argv0() {
        assert_eq!(
            display_name("systemd-journal", b"/usr/lib/systemd/systemd-journald\0"),
            "systemd-journald"
        );
        // argv0 that does not extend comm (a renamed process) leaves comm.
        assert_eq!(
            display_name("kworker/u16:3-ev", b"other\0"),
            "kworker/u16:3-ev"
        );
        assert_eq!(display_name("systemd-journal", b""), "systemd-journal");
        // A short comm is never replaced.
        assert_eq!(display_name("sshd", b"/usr/sbin/sshd-session\0"), "sshd");
    }

    /// `app` from `/proc/<pid>/cgroup`: the unit or container, kind-qualified, waiting
    /// for a container's name while a refresh is due and falling back to its short id.
    #[test]
    fn app_is_the_unit_or_container() {
        const NGINX: &str = "ba1d8caf4daec08a0dfd56c5d1e80e270631bedc443634cc398d78deb1769013";
        const REDIS: &str = "47d70c0c2814b54a3bf9cfc143c8c4ec5632efd4732a4c9afa4f7d831daf270b";
        let real = |pid: &str| {
            testdata::cgroup_t0()
                .read(&format!("/proc/{pid}/cgroup"))
                .unwrap()
        };
        let dir = host();
        testdata::write(&dir, "proc/1/cgroup", &real("1"));
        let procs = [
            (2, real("3176")), // an nginx worker
            (3, real("1053")), // sshd
            (4, format!("0::/system.slice/docker-{REDIS}.scope\n")),
            (
                5,
                "0::/user.slice/user-0.slice/session-23.scope\n".to_owned(),
            ),
            (6, real("4741")), // user@0.service/init.scope
            (
                7,
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/pipewire.service\n"
                    .to_owned(),
            ),
            (8, "0::/system.slice/sshd.service\n".to_owned()),
        ];
        for (pid, cgroup) in &procs {
            let stat = format!("{pid} (p{pid}) S 0 0 0 0 -1 0 0 0 0 0 7 3 0 0 20 0 1 0 5 0 0");
            testdata::write(&dir, &format!("proc/{pid}/stat"), &stat);
            testdata::write(&dir, &format!("proc/{pid}/cgroup"), cgroup);
        }
        let names = ContainerNames::attached();
        names.finish(Some([(NGINX.to_owned(), Arc::from("nginx"))].into()));
        let mut p = Processes::at(SysRoot::at(dir.path()), 4096, 100);
        p.names = names.clone();
        p.probe();
        let mut buf = SampleBuf::new();
        let mut apps = |p: &mut Processes, at: u64| {
            buf.clear();
            p.sample(&tick(at * S), &mut buf).unwrap();
            let mut rows: Vec<(i32, Option<Arc<str>>)> = buf
                .processes()
                .iter()
                .map(|r| (r.pid, r.app.clone()))
                .collect();
            rows.sort_by_key(|r| r.0);
            rows
        };
        apps(&mut p, 1);
        let rows = apps(&mut p, 2);
        let app = |rows: &[(i32, Option<Arc<str>>)], pid| {
            rows.iter()
                .find(|r| r.0 == pid)
                .and_then(|r| r.1.as_deref().map(str::to_owned))
        };
        assert_eq!(app(&rows, 1), None, "init.scope");
        assert_eq!(app(&rows, 2).as_deref(), Some("docker:nginx"));
        assert_eq!(app(&rows, 3).as_deref(), Some("sshd.service"));
        assert_eq!(app(&rows, 4), None, "waiting for the refresh it asked for");
        assert!(names.requested());
        assert_eq!(app(&rows, 5), None, "a session");
        assert_eq!(app(&rows, 6), None, "the user manager itself");
        assert_eq!(app(&rows, 7).as_deref(), Some("pipewire.service"));
        let shared = |pid| {
            rows.iter()
                .find(|r| r.0 == pid)
                .and_then(|r| r.1.clone())
                .unwrap()
        };
        assert!(Arc::ptr_eq(&shared(3), &shared(8)), "one value per unit");
        // The refresh finds redis.
        names.finish(Some(
            [
                (NGINX.to_owned(), Arc::from("nginx")),
                (REDIS.to_owned(), Arc::from("opendata-redis")),
            ]
            .into(),
        ));
        let rows = apps(&mut p, 3);
        assert_eq!(app(&rows, 4).as_deref(), Some("docker:opendata-redis"));

        // Without a name map: short ids at once.
        let mut p = Processes::at(SysRoot::at(dir.path()), 4096, 100);
        p.probe();
        apps(&mut p, 1);
        let rows = apps(&mut p, 2);
        assert_eq!(app(&rows, 2).as_deref(), Some("docker:ba1d8caf4dae"));
        assert_eq!(app(&rows, 4).as_deref(), Some("docker:47d70c0c2814"));
    }

    #[test]
    fn capabilities_hide_energy_and_wakeups() {
        let p = Processes::at(SysRoot::at("/nonexistent"), 4096, 100);
        let mut caps = kelvo_schema::Capabilities {
            process_energy: true,
            process_wakeups: true,
            process_disk_io: true,
            ..Default::default()
        };
        p.fill_capabilities(&mut caps);
        assert!(!caps.process_energy && !caps.process_wakeups);
        assert!(!caps.process_network && !caps.process_gpu);
        assert!(!caps.process_disk_io, "not probed: nothing read");
    }

    /// Disk columns only when the probe could read another user's `io`: pid 1's.
    #[test]
    fn disk_io_capability_follows_pid_1() {
        let caps = |p: &Processes| {
            let mut caps = kelvo_schema::Capabilities::default();
            p.fill_capabilities(&mut caps);
            caps.process_disk_io
        };
        let dir = host();
        let mut p = Processes::at(SysRoot::at(dir.path()), 4096, 100);
        assert!(matches!(p.probe(), Probe::Supported(_)));
        assert!(caps(&p));

        // Unreadable (a non-root agent gets EACCES; here the file is gone).
        std::fs::remove_file(dir.path().join("proc/1/io")).unwrap();
        assert!(matches!(p.probe(), Probe::Supported(_)));
        assert!(!caps(&p));

        // Readable but not the shape `io` has.
        testdata::write(&dir, "proc/1/io", "rchar: 1\n");
        p.probe();
        assert!(!caps(&p));
    }

    #[test]
    fn no_proc_is_unsupported() {
        let mut p = Processes::at(SysRoot::at("/nonexistent"), 4096, 100);
        assert!(matches!(p.probe(), Probe::Unsupported { .. }));
        assert!(p.sample(&tick(S), &mut SampleBuf::new()).is_err());
    }

    #[test]
    #[ignore = "reads every live process; run by hand on Linux"]
    fn live_smoke() {
        let mut p = Processes::new(ContainerNames::default());
        assert_eq!(p.probe(), Probe::Supported(Vec::new()));
        let mut buf = SampleBuf::new();
        p.sample(&tick(S), &mut buf).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        buf.clear();
        p.sample(&tick(2 * S), &mut buf).unwrap();
        println!("{:?}, rows {}", p.visibility(), buf.processes().len());
        assert!(!buf.processes().is_empty());
    }
}
