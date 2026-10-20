//! Fixture roots for the Linux collector tests: the trees under `linux/fixtures/` and
//! small synthetic trees written to a temp dir.

use std::path::PathBuf;

use super::SysRoot;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/linux/fixtures")
        .join(name)
}

/// linux-tailscale (M1 Mac mini, Fedora Asahi, 16 KiB pages, `USER_HZ` 100).
pub fn asahi_t0() -> SysRoot {
    SysRoot::at(fixture("asahi-m1/t0"))
}

/// The same host one second after [`asahi_t0`]: `proc/stat`, `vmstat`, pressure.
pub fn asahi_t1() -> SysRoot {
    SysRoot::at(fixture("asahi-m1/t1"))
}

/// linux-tailscale's cgroup v2 tree, a subset: three Docker scopes (nginx,
/// opendata-redis, cadvisor), `sshd`, `docker` and `getty@tty1` services, root's user
/// manager with `dbus-broker`, a session scope, `init.scope`s, and four
/// `/proc/<pid>/cgroup` files.
pub fn cgroup_t0() -> SysRoot {
    SysRoot::at(fixture("asahi-m1-cgroup/t0"))
}

/// The same files about five seconds after [`cgroup_t0`].
pub fn cgroup_t1() -> SysRoot {
    SysRoot::at(fixture("asahi-m1-cgroup/t1"))
}

/// A synthetic 4-CPU x86 VM: acpi-cpufreq steps, no `cpu_capacity`, DMI, no device tree.
pub fn x86() -> SysRoot {
    SysRoot::at(fixture("x86-synthetic"))
}

/// A temp tree of `(path, contents)` files laid out like `/`.
pub fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix("kelvo-linux-")
        .tempdir()
        .expect("temp dir");
    for (path, text) in files {
        write(&dir, path, text);
    }
    dir
}

/// Writes (or replaces) one file in a [`tree`].
pub fn write(dir: &tempfile::TempDir, path: &str, text: &str) {
    let path = dir.path().join(path);
    std::fs::create_dir_all(path.parent().expect("a file path")).expect("mkdir");
    std::fs::write(path, text).expect("write");
}
