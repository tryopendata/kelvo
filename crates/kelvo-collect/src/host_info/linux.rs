//! Linux host facts: os-release, uname, the device tree or DMI, cpufreq and
//! `cpu_capacity` (through the CPU collector's topology, so core labels agree),
//! `/proc/meminfo` and `/proc/stat`. Other non-macOS targets get a bare record.

use kelvo_schema::{HostId, HostRecord};

#[cfg(target_os = "linux")]
pub fn host_record(id: HostId) -> HostRecord {
    imp::record(&crate::linux::SysRoot::host(), id, imp::Uname::read())
}

#[cfg(not(target_os = "linux"))]
pub fn host_record(id: HostId) -> HostRecord {
    use kelvo_schema::{HostInfo, OsKind};
    HostRecord {
        id,
        is_local: true,
        display_name: "This computer".into(),
        info: HostInfo {
            os: OsKind::Unknown,
            os_version: String::new(),
            model: None,
            chip: None,
            chip_known: false,
            cpu_topology: Vec::new(),
            mem_total_bytes: 0,
            boot_time_ms: 0,
            gpu_dvfs_mhz: Vec::new(),
            kernel: None,
            boot_mounts: Vec::new(),
        },
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CStr;

    use kelvo_schema::{HostId, HostInfo, HostRecord, OsKind};

    use crate::linux::SysRoot;
    use crate::linux::procfs::kib;
    use crate::linux::topology::{boot_time_s, clusters};

    /// The `uname` fields the record uses.
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct Uname {
        pub nodename: String,
        pub release: String,
    }

    impl Uname {
        pub fn read() -> Option<Self> {
            // SAFETY: an all-zero utsname is valid (arrays of c_char).
            let mut u: libc::utsname = unsafe { std::mem::zeroed() };
            // SAFETY: `u` is a writable utsname; uname fills NUL-terminated fields.
            if unsafe { libc::uname(&mut u) } != 0 {
                return None;
            }
            // SAFETY: uname succeeded, so each field holds a NUL-terminated string
            // within its array.
            let s = |f: &[libc::c_char]| {
                unsafe { CStr::from_ptr(f.as_ptr()) }
                    .to_string_lossy()
                    .into_owned()
            };
            Some(Self {
                nodename: s(&u.nodename),
                release: s(&u.release),
            })
        }
    }

    /// Apple SoCs by device-tree `compatible`, as the Asahi device trees name them.
    const APPLE_SOCS: &[(&str, &str)] = &[
        ("apple,t8103", "Apple M1"),
        ("apple,t6000", "Apple M1 Pro"),
        ("apple,t6001", "Apple M1 Max"),
        ("apple,t6002", "Apple M1 Ultra"),
        ("apple,t8112", "Apple M2"),
        ("apple,t6020", "Apple M2 Pro"),
        ("apple,t6021", "Apple M2 Max"),
        ("apple,t6022", "Apple M2 Ultra"),
    ];

    /// A device-tree or sysfs string without trailing NULs and whitespace; `None` when
    /// unreadable or empty.
    fn read_str(root: &SysRoot, rel: &str) -> Option<String> {
        let text = root.read(rel).ok()?;
        let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        (!text.is_empty()).then(|| text.to_owned())
    }

    /// `PRETTY_NAME` from os-release (`/etc`, else `/usr/lib`), unquoted.
    fn os_version(root: &SysRoot) -> Option<String> {
        let text = root
            .read("/etc/os-release")
            .or_else(|_| root.read("/usr/lib/os-release"))
            .ok()?;
        let v = text.lines().find_map(|l| l.strip_prefix("PRETTY_NAME="))?;
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(v);
        (!v.is_empty()).then(|| v.to_owned())
    }

    fn model(root: &SysRoot) -> Option<String> {
        read_str(root, "/proc/device-tree/model")
            .or_else(|| read_str(root, "/sys/class/dmi/id/product_name"))
    }

    /// The SoC from the device tree's `compatible` list, else the first `model name` in
    /// `/proc/cpuinfo` (x86; ARM cpuinfo has none).
    fn chip(root: &SysRoot) -> Option<String> {
        let compatible = root
            .read("/proc/device-tree/compatible")
            .unwrap_or_default();
        let soc = compatible.split('\0').find_map(|c| {
            APPLE_SOCS
                .iter()
                .find(|(id, _)| *id == c)
                .map(|(_, name)| (*name).to_owned())
        });
        soc.or_else(|| {
            let cpuinfo = root.read("/proc/cpuinfo").ok()?;
            let name = cpuinfo.lines().find_map(|l| {
                let (k, v) = l.split_once(':')?;
                (k.trim() == "model name").then(|| v.trim().to_owned())
            })?;
            (!name.is_empty()).then_some(name)
        })
    }

    pub fn record(root: &SysRoot, id: HostId, uname: Option<Uname>) -> HostRecord {
        let uname = uname.unwrap_or_default();
        let model = model(root);
        let meminfo = root.read("/proc/meminfo").unwrap_or_default();
        let display_name = Some(uname.nodename)
            .filter(|n| !n.is_empty() && n != "(none)")
            .or_else(|| model.clone())
            .unwrap_or_else(|| "Linux host".into());
        HostRecord {
            id,
            is_local: true,
            display_name,
            info: HostInfo {
                os: OsKind::Linux,
                os_version: os_version(root).unwrap_or_default(),
                model,
                chip: chip(root),
                // Nothing on Linux corrects it later (macOS: a sensors probe reporting
                // an unknown chip), and the Linux collectors need no chip tables.
                chip_known: true,
                cpu_topology: clusters(root).into_iter().map(|c| c.info).collect(),
                mem_total_bytes: kib(&meminfo, "MemTotal").unwrap_or(0),
                boot_time_ms: boot_time_s(root)
                    .and_then(|s| i64::try_from(s).ok())
                    .map_or(0, |s| s.saturating_mul(1000)),
                gpu_dvfs_mhz: Vec::new(),
                kernel: Some(uname.release).filter(|r| !r.is_empty()),
                boot_mounts: vec!["/".into()],
            },
        }
    }

    #[cfg(test)]
    mod tests {
        use kelvo_schema::CoreKind;
        use uuid::Uuid;

        use super::*;
        use crate::linux::testdata;

        fn asahi_uname() -> Uname {
            // From the fixture's uname.txt (`uname -a`): nodename and release.
            let text = testdata::asahi_t0().read("/uname.txt").unwrap();
            let mut f = text.split_ascii_whitespace().skip(1);
            Uname {
                nodename: f.next().unwrap().into(),
                release: f.next().unwrap().into(),
            }
        }

        #[test]
        fn asahi_m1_record() {
            let r = record(
                &testdata::asahi_t0(),
                HostId(Uuid::nil()),
                Some(asahi_uname()),
            );
            assert!(r.is_local);
            assert_eq!(r.display_name, "m1-linux");
            let i = &r.info;
            assert_eq!(i.os, OsKind::Linux);
            assert_eq!(i.os_version, "Fedora Linux Asahi Remix 41 (Server Edition)");
            assert_eq!(
                i.kernel.as_deref(),
                Some("6.17.12-400.asahi.fc41.aarch64+16k")
            );
            assert_eq!(i.model.as_deref(), Some("Apple Mac mini (M1, 2020)"));
            assert_eq!(i.chip.as_deref(), Some("Apple M1"));
            assert!(i.chip_known);
            assert_eq!(i.mem_total_bytes, 16_065_952 * 1024);
            assert_eq!(i.boot_time_ms, 1_788_630_414_000);
            assert_eq!(i.boot_mounts, ["/"]);
            assert!(i.gpu_dvfs_mhz.is_empty());
            let kinds: Vec<_> = i
                .cpu_topology
                .iter()
                .map(|c| (c.name.as_str(), c.kind))
                .collect();
            assert_eq!(
                kinds,
                [("E0", CoreKind::Efficiency), ("P0", CoreKind::Performance)]
            );
            assert_eq!(i.cpu_topology[1].dvfs_mhz.last(), Some(&2988));
        }

        #[test]
        fn x86_record() {
            let uname = Uname {
                nodename: "build-01".into(),
                release: "6.8.0-45-generic".into(),
            };
            let r = record(&testdata::x86(), HostId(Uuid::nil()), Some(uname));
            let i = &r.info;
            assert_eq!(r.display_name, "build-01");
            assert_eq!(i.os_version, "Ubuntu 24.04.1 LTS");
            assert_eq!(i.model.as_deref(), Some("Standard PC (Q35 + ICH9, 2009)"));
            assert_eq!(
                i.chip.as_deref(),
                Some("Intel(R) Xeon(R) Silver 4214 CPU @ 2.20GHz")
            );
            assert_eq!(i.cpu_topology.len(), 1);
            assert_eq!(i.cpu_topology[0].kind, CoreKind::Uniform);
            assert_eq!(i.boot_time_ms, 1_700_000_000_000);
        }

        #[test]
        fn empty_tree_still_makes_a_record() {
            let r = record(&SysRoot::at("/nonexistent"), HostId(Uuid::nil()), None);
            assert_eq!(r.display_name, "Linux host");
            assert_eq!(r.info.os, OsKind::Linux);
            assert_eq!(r.info.os_version, "");
            assert_eq!(
                (
                    r.info.model.clone(),
                    r.info.chip.clone(),
                    r.info.kernel.clone()
                ),
                (None, None, None)
            );
            assert!(r.info.cpu_topology.is_empty());
            assert_eq!((r.info.mem_total_bytes, r.info.boot_time_ms), (0, 0));
        }

        #[test]
        fn os_release_quoting_and_unknown_socs() {
            let dir = testdata::tree(&[
                ("usr/lib/os-release", "PRETTY_NAME='Arch Linux'\n"),
                (
                    "proc/device-tree/compatible",
                    "apple,j999\0apple,t9999\0apple,arm-platform\0",
                ),
                ("proc/device-tree/model", "Apple Mystery\0"),
            ]);
            let root = SysRoot::at(dir.path());
            assert_eq!(os_version(&root).as_deref(), Some("Arch Linux"));
            assert_eq!(chip(&root), None, "an SoC not in the table is not guessed");
            assert_eq!(model(&root).as_deref(), Some("Apple Mystery"));
            let dir = testdata::tree(&[("etc/os-release", "PRETTY_NAME=Gentoo\n")]);
            assert_eq!(
                os_version(&SysRoot::at(dir.path())).as_deref(),
                Some("Gentoo")
            );
        }

        #[test]
        fn live_uname() {
            let u = Uname::read().unwrap();
            assert!(!u.release.is_empty());
        }

        #[test]
        #[ignore = "prints this host's record; run by hand on Linux"]
        fn live_record() {
            let r = super::super::host_record(HostId(Uuid::nil()));
            println!("{r:#?}");
            assert!(r.info.mem_total_bytes > 0 && r.info.boot_time_ms > 0);
        }
    }
}
