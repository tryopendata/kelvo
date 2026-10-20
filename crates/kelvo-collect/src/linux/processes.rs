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
//!   the disk columns, so most rows' unknown rates never show as 0 B/s (D-115). Rows
//!   still carry 0 for an unreadable `io`, since `ProcessSample` has no "unknown" for
//!   disk; with the capability true that is only a process that exited mid-sample.
//!   [`Visibility::io_readable`] counts the rows that were measured.
//! - Energy and idle wakeups have no Linux source; `fill_capabilities` says so and the
//!   rows carry 0.
//! - `app` is `None` until cgroup grouping (D-115) lands.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io;
use std::sync::Arc;

use kelvo_schema::Entitlement;

use super::SysRoot;
use super::procfs::{PidDir, field, read_into, user_name, word};
use crate::{
    Cadence, CollectError, Collector, CollectorId, Interest, Probe, ProcessSample, SampleBuf, Tick,
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

struct Known {
    name: Arc<str>,
    user: Arc<str>,
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
    visibility: Visibility,
    /// The probe could read another user's `io` (pid 1's).
    disk_io: bool,
}

impl Default for Processes {
    fn default() -> Self {
        Self::new()
    }
}

impl Processes {
    pub fn new() -> Self {
        Self::at(SysRoot::host(), super::page_size(), super::clk_tck())
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
            visibility: Visibility::default(),
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
        Known {
            name: Arc::from(name),
            user,
            io_denied: false,
            prev: None,
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
                app: None,
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

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let listed = self.dir.as_ref().is_some_and(|d| d.list(&mut self.pids));
        if !listed {
            return Err(CollectError::Os {
                call: "opendir /proc",
                code: i64::from(io::Error::last_os_error().raw_os_error().unwrap_or(0)),
            });
        }
        let now = tick.continuous_ns;
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
        let mut p = Processes::new();
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
