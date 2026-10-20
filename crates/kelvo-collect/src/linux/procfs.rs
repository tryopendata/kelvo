//! Small parsers and readers shared by the `/proc` and sysfs collectors. Readers fill
//! buffers the caller keeps, so a collector that reads the same files every tick does
//! not allocate once its buffers have grown.

use std::ffi::{CStr, CString};
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::SysRoot;

/// Reads `path` into `buf` (cleared first), reusing its capacity. procfs reports zero
/// sizes, so this reads to the end.
pub fn read_into(path: impl AsRef<Path>, buf: &mut String) -> io::Result<()> {
    buf.clear();
    File::open(path)?.read_to_string(buf).map(drop)
}

/// The value of a `Key: value` line (`/proc/meminfo`, `/proc/<pid>/status`), trimmed.
pub fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':').map(str::trim))
}

/// A `Key:   123 kB` line in bytes.
pub fn kib(text: &str, key: &str) -> Option<u64> {
    let v = field(text, key)?;
    let v = v.strip_suffix("kB").unwrap_or(v).trim();
    v.parse::<u64>().ok()?.checked_mul(1024)
}

/// The value of a `key 123` line (`/proc/vmstat`, `/proc/stat`'s `btime`).
pub fn word(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| {
        let mut it = l.split_ascii_whitespace();
        (it.next()? == key).then(|| it.next()?.parse().ok())?
    })
}

/// A CPU list in either kernel form, `0-3,6` (`online`) or `0 1 2 3` (`related_cpus`),
/// sorted and deduplicated. Malformed parts are skipped.
pub fn cpu_list(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for part in text.split([',', ' ', '\n', '\t']).filter(|p| !p.is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>())
                    && a <= b
                    && b - a < 4096
                {
                    out.extend(a..=b);
                }
            }
            None => out.extend(part.parse::<usize>().ok()),
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The numeric entries of a directory (`/proc`'s pids), without allocating per entry the
/// way `std::fs::read_dir` does.
pub struct PidDir {
    path: CString,
}

impl PidDir {
    /// `/proc` under `root`; `None` if the path has an interior NUL.
    pub fn new(root: &SysRoot) -> Option<Self> {
        let path = root.path("/proc");
        CString::new(path.as_os_str().as_bytes())
            .ok()
            .map(|path| Self { path })
    }

    /// Lists the pids into `out` (cleared first). False if the directory can't be opened.
    pub fn list(&self, out: &mut Vec<i32>) -> bool {
        out.clear();
        // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
        let dir = unsafe { libc::opendir(self.path.as_ptr()) };
        if dir.is_null() {
            return false;
        }
        loop {
            // SAFETY: `dir` is the open stream from opendir above, used by this thread
            // only and not yet closed.
            let ent = unsafe { libc::readdir(dir) };
            if ent.is_null() {
                break;
            }
            // SAFETY: readdir returned a valid entry, live until the next readdir or
            // closedir on `dir`; `d_name` is NUL-terminated (POSIX).
            let name = unsafe { CStr::from_ptr((*ent).d_name.as_ptr()) };
            if let Some(pid) = parse_pid(name.to_bytes()) {
                out.push(pid);
            }
        }
        // SAFETY: `dir` came from opendir and is closed exactly once.
        unsafe { libc::closedir(dir) };
        true
    }
}

fn parse_pid(name: &[u8]) -> Option<i32> {
    if name.is_empty() || !name.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(name).ok()?.parse().ok()
}

/// A user name for `uid` from `/etc/passwd` under `root`, or the number when it has
/// none. Read once per uid by the callers, so the file is not cached here.
pub fn user_name(root: &SysRoot, uid: u32) -> String {
    root.read("/etc/passwd")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|l| {
                let mut f = l.split(':');
                let name = f.next()?;
                let id = f.nth(1)?.parse::<u32>().ok()?;
                (id == uid && !name.is_empty()).then(|| name.to_owned())
            })
        })
        .unwrap_or_else(|| uid.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_and_units() {
        let text = "MemTotal:       16065952 kB\nHugePages_Total:       0\nName:\tsystemd\n";
        assert_eq!(kib(text, "MemTotal"), Some(16_065_952 * 1024));
        assert_eq!(kib(text, "HugePages_Total"), Some(0));
        assert_eq!(field(text, "Name"), Some("systemd"));
        // A key that is a prefix of another does not match it.
        assert_eq!(kib(text, "Mem"), None);
        assert_eq!(kib("MemTotal: lots kB\n", "MemTotal"), None, "garbled");
        assert_eq!(word("pswpin 9176059\npswpout 17\n", "pswpout"), Some(17));
        assert_eq!(word("pswpinx 1\n", "pswpin"), None);
    }

    #[test]
    fn cpu_lists_in_both_forms() {
        assert_eq!(cpu_list("0-3\n"), [0, 1, 2, 3]);
        assert_eq!(cpu_list("4 5 6 7 \n"), [4, 5, 6, 7]);
        assert_eq!(cpu_list("0,2-3,9"), [0, 2, 3, 9]);
        assert_eq!(cpu_list("3-1,x,5"), [5], "malformed parts are skipped");
        assert!(cpu_list("").is_empty());
    }

    #[test]
    fn pid_names() {
        assert_eq!(parse_pid(b"1234"), Some(1234));
        for bad in [&b""[..], b"self", b"12a", b"99999999999"] {
            assert_eq!(parse_pid(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn lists_numeric_entries_only() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["1", "42", "self", "sys", "007x"] {
            std::fs::create_dir_all(dir.path().join("proc").join(name)).unwrap();
        }
        std::fs::write(dir.path().join("proc/stat"), "").unwrap();
        let pd = PidDir::new(&SysRoot::at(dir.path())).unwrap();
        let mut pids = vec![9];
        assert!(pd.list(&mut pids));
        pids.sort_unstable();
        assert_eq!(pids, [1, 42]);
        assert!(
            !PidDir::new(&SysRoot::at("/nonexistent"))
                .unwrap()
                .list(&mut pids)
        );
        assert!(pids.is_empty());
    }

    #[test]
    fn user_names_from_passwd() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("etc")).unwrap();
        std::fs::write(
            dir.path().join("etc/passwd"),
            "root:x:0:0:root:/root:/bin/zsh\nriley:x:1000:1000::/home/riley:/bin/bash\n",
        )
        .unwrap();
        let root = SysRoot::at(dir.path());
        assert_eq!(user_name(&root, 1000), "riley");
        assert_eq!(user_name(&root, 0), "root");
        assert_eq!(user_name(&root, 4242), "4242");
    }
}
