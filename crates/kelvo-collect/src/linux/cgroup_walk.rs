//! The bounded walk of the cgroup v2 tree and the per-group file parsers.
//!
//! Visited, under `/sys/fs/cgroup`:
//! - `system.slice/*.service`, `system.slice/system-*.slice/*.service` (template
//!   instances such as `getty@tty1.service`)
//! - `system.slice/docker-<id>.scope` (Docker's systemd driver), `docker/<id>` (its
//!   cgroupfs driver), `machine.slice/libpod-<id>.scope` and `system.slice/libpod-*`
//! - `user.slice/user-<uid>.slice/user@<uid>.service/**`: its `*.service` units and
//!   `libpod-*.scope` containers, through nested slices up to [`USER_DEPTH`] deep
//!
//! Directory listings go through `readdir` into buffers kept across passes, so a pass
//! over an unchanged tree does not allocate.

use std::ffi::CStr;
use std::fmt::Write as _;

use super::cgroup_names::{Unit, bare_id, docker_scope, podman_scope, user_uid};
use super::procfs::{for_each_entry, word};

/// Slices followed below `user@<uid>.service`.
pub(super) const USER_DEPTH: usize = 6;
/// Groups visited per pass at most, so a pathological tree costs a bounded pass.
pub(super) const MAX_GROUPS: usize = 2048;

/// Reusable buffers for one walk.
pub(super) struct Walk {
    /// The cgroup root, as bytes, no trailing slash.
    base: Vec<u8>,
    cpath: Vec<u8>,
    rel: String,
    /// One listing per depth: `[is_dir][name][NUL]` records.
    pool: Vec<Vec<u8>>,
    visited: usize,
}

/// The `(name, may_be_dir)` records of a listing.
fn records(list: &[u8]) -> impl Iterator<Item = (&str, bool)> {
    list.split(|&b| b == 0).filter_map(|r| {
        let (&dir, name) = r.split_first()?;
        Some((std::str::from_utf8(name).ok()?, dir == 1))
    })
}

impl Walk {
    pub(super) fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').as_bytes().to_vec(),
            cpath: Vec::new(),
            rel: String::new(),
            pool: Vec::new(),
            visited: 0,
        }
    }

    /// Lists `self.rel` into the depth-`depth` buffer, which the caller hands back
    /// with [`Walk::give`].
    fn list(&mut self, depth: usize) -> Vec<u8> {
        while self.pool.len() <= depth {
            self.pool.push(Vec::new());
        }
        let mut out = std::mem::take(&mut self.pool[depth]);
        out.clear();
        self.cpath.clear();
        self.cpath.extend_from_slice(&self.base);
        if !self.rel.is_empty() {
            self.cpath.push(b'/');
            self.cpath.extend_from_slice(self.rel.as_bytes());
        }
        self.cpath.push(0);
        if let Ok(path) = CStr::from_bytes_with_nul(&self.cpath) {
            for_each_entry(path, |name, dir| {
                if !name.contains(&0) {
                    out.push(u8::from(dir));
                    out.extend_from_slice(name);
                    out.push(0);
                }
            });
        }
        out
    }

    fn give(&mut self, depth: usize, list: Vec<u8>) {
        if let Some(slot) = self.pool.get_mut(depth) {
            *slot = list;
        }
    }

    /// Appends `/name` to `self.rel`; returns the length to truncate back to.
    fn push(&mut self, name: &str) -> usize {
        let len = self.rel.len();
        if !self.rel.is_empty() {
            self.rel.push('/');
        }
        self.rel.push_str(name);
        len
    }

    fn emit(&mut self, unit: Unit<'_>, visit: &mut impl FnMut(&str, Unit<'_>)) {
        if self.visited < MAX_GROUPS {
            self.visited += 1;
            visit(&self.rel, unit);
        }
    }

    /// Visits every reported group: `visit(path under the root, what it is)`.
    pub(super) fn run(&mut self, visit: &mut impl FnMut(&str, Unit<'_>)) {
        self.rel.clear();
        self.visited = 0;
        self.top("system.slice", visit, Self::system);
        self.top("machine.slice", visit, |w, name, visit| {
            if let Some(id) = podman_scope(name) {
                w.emit(Unit::Podman(id), visit);
            }
        });
        self.top("docker", visit, |w, name, visit| {
            if let Some(id) = bare_id(name) {
                w.emit(Unit::Docker(id), visit);
            }
        });
        self.top("user.slice", visit, |w, name, visit| {
            let Some(uid) = user_uid(name, "user-", ".slice") else {
                return;
            };
            let back = w.rel.len();
            // Writing to a String cannot fail.
            let _ = write!(w.rel, "/user@{uid}.service");
            w.user(uid, 1, visit);
            w.rel.truncate(back);
        });
    }

    /// Lists top-level directory `top` and calls `each` with every subdirectory, with
    /// `self.rel` set to it.
    fn top<V: FnMut(&str, Unit<'_>)>(
        &mut self,
        top: &str,
        visit: &mut V,
        each: impl Fn(&mut Self, &str, &mut V),
    ) {
        let back = self.push(top);
        let list = self.list(0);
        for (name, dir) in records(&list) {
            if dir {
                let inner = self.push(name);
                each(self, name, visit);
                self.rel.truncate(inner);
            }
        }
        self.give(0, list);
        self.rel.truncate(back);
    }

    fn system(&mut self, name: &str, visit: &mut impl FnMut(&str, Unit<'_>)) {
        if let Some(id) = docker_scope(name) {
            self.emit(Unit::Docker(id), visit);
        } else if let Some(id) = podman_scope(name) {
            self.emit(Unit::Podman(id), visit);
        } else if name.ends_with(".service") {
            self.emit(Unit::Service(name), visit);
        } else if name.starts_with("system-") && name.ends_with(".slice") {
            let list = self.list(1);
            for (unit, dir) in records(&list) {
                if dir && unit.ends_with(".service") {
                    let back = self.push(unit);
                    self.emit(Unit::Service(unit), visit);
                    self.rel.truncate(back);
                }
            }
            self.give(1, list);
        }
    }

    /// Inside `user@<uid>.service` (at `self.rel`), `depth` levels below the root.
    fn user(&mut self, uid: u32, depth: usize, visit: &mut impl FnMut(&str, Unit<'_>)) {
        let list = self.list(depth);
        for (name, dir) in records(&list) {
            if !dir {
                continue;
            }
            let back = self.push(name);
            if let Some(id) = podman_scope(name) {
                self.emit(Unit::Podman(id), visit);
            } else if name.ends_with(".service") {
                self.emit(Unit::User { uid, unit: name }, visit);
            } else if name.ends_with(".slice") && depth < USER_DEPTH {
                self.user(uid, depth + 1, visit);
            }
            self.rel.truncate(back);
        }
        self.give(depth, list);
    }
}

/// The counters read from one group on one pass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Counters {
    /// `cpu.stat` `usage_usec`.
    pub(super) usage_us: Option<u64>,
    /// `memory.current`, bytes.
    pub(super) mem: Option<u64>,
    /// `io.stat` `rbytes` and `wbytes`, summed over devices.
    pub(super) io: Option<(u64, u64)>,
    /// `memory.pressure` `some` `total`, microseconds.
    pub(super) psi_some_us: Option<u64>,
}

/// `usage_usec` from `cpu.stat`.
pub(super) fn parse_cpu_stat(text: &str) -> Option<u64> {
    word(text, "usage_usec")
}

/// `memory.current` or a numeric `memory.max`; `max` (no limit) is `None`.
pub(super) fn parse_bytes(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// `rbytes` and `wbytes` summed over the devices of `io.stat`. An empty file (a group
/// that has done no I/O) is zero.
pub(super) fn parse_io_stat(text: &str) -> Option<(u64, u64)> {
    let mut sum = (0u64, 0u64);
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        fields.next()?; // the device, `259:0`
        for f in fields {
            if let Some(v) = f.strip_prefix("rbytes=") {
                sum.0 = sum.0.saturating_add(v.parse().ok()?);
            } else if let Some(v) = f.strip_prefix("wbytes=") {
                sum.1 = sum.1.saturating_add(v.parse().ok()?);
            }
        }
    }
    Some(sum)
}

/// The `some` line's `total=` from a PSI file, microseconds.
pub(super) fn parse_psi_some(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix("some "))?
        .split_ascii_whitespace()
        .find_map(|f| f.strip_prefix("total="))?
        .parse()
        .ok()
}

/// Rates over `secs` from two counter readings.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Rates {
    pub(super) cpu_pct: Option<f64>,
    pub(super) io_read: Option<f64>,
    pub(super) io_write: Option<f64>,
    pub(super) psi_pct: Option<f64>,
}

pub(super) fn rates(prev: &Counters, cur: &Counters, secs: f64) -> Rates {
    let d = |a: Option<u64>, b: Option<u64>| b?.checked_sub(a?).map(|d| d as f64);
    let us = secs * 1e6;
    Rates {
        cpu_pct: d(prev.usage_us, cur.usage_us).map(|d| d / us * 100.0),
        io_read: d(prev.io.map(|i| i.0), cur.io.map(|i| i.0)).map(|d| d / secs),
        io_write: d(prev.io.map(|i| i.1), cur.io.map(|i| i.1)).map(|d| d / secs),
        psi_pct: d(prev.psi_some_us, cur.psi_some_us).map(|d| (d / us * 100.0).min(100.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::testdata;

    #[test]
    fn parses_cgroup_files() {
        assert_eq!(
            parse_cpu_stat("usage_usec 15866\nuser_usec 5015\n"),
            Some(15866)
        );
        assert_eq!(parse_cpu_stat("user_usec 5015\n"), None);
        assert_eq!(parse_bytes("11059200\n"), Some(11_059_200));
        assert_eq!(parse_bytes("max\n"), None);
        assert_eq!(
            parse_io_stat(
                "259:0 rbytes=3829760 wbytes=10 rios=57 wios=0 dbytes=0 dios=0\n\
                 8:0 rbytes=1 wbytes=2 rios=1 wios=1\n"
            ),
            Some((3_829_761, 12))
        );
        assert_eq!(parse_io_stat(""), Some((0, 0)), "no I/O yet");
        assert_eq!(parse_io_stat("259:0 rbytes=x\n"), None, "garbled");
        assert_eq!(
            parse_psi_some(
                "some avg10=0.00 avg60=0.00 avg300=0.00 total=63910280\n\
                 full avg10=0.00 avg60=0.00 avg300=0.00 total=63051390\n"
            ),
            Some(63_910_280)
        );
        assert_eq!(parse_psi_some("full avg10=0.00 total=5\n"), None);
    }

    #[test]
    fn walks_only_units_and_containers() {
        let root = testdata::cgroup_t0();
        let mut walk = Walk::new(root.path("/sys/fs/cgroup").to_str().unwrap());
        let mut seen = Vec::new();
        walk.run(&mut |rel, unit| seen.push((rel.to_owned(), format!("{unit:?}"))));
        seen.sort();
        let rels: Vec<&str> = seen.iter().map(|(r, _)| r.as_str()).collect();
        assert_eq!(
            rels,
            [
                "system.slice/docker-47d70c0c2814b54a3bf9cfc143c8c4ec5632efd4732a4c9afa4f7d831daf270b.scope",
                "system.slice/docker-b2bca07ec6d22b036c50863871d403876ec89e95a4b4a01148e6a5131a0bf39b.scope",
                "system.slice/docker-ba1d8caf4daec08a0dfd56c5d1e80e270631bedc443634cc398d78deb1769013.scope",
                "system.slice/docker.service",
                "system.slice/sshd.service",
                "system.slice/system-getty.slice/getty@tty1.service",
                "user.slice/user-0.slice/user@0.service/session.slice/dbus-broker.service",
            ]
        );
        assert!(seen.iter().any(|(_, u)| u.contains("User { uid: 0")));
        // A second pass over the same tree visits the same groups.
        let mut again = 0;
        walk.run(&mut |_, _| again += 1);
        assert_eq!(again, rels.len());
    }
}
