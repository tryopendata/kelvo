//! Safe wrappers over libproc: pid listing, task/BSD info, `proc_pid_rusage` v6 and the
//! responsible-pid lookup. Shared by the processes and `self.cpu` collectors.

use std::borrow::Cow;
use std::ffi::{CStr, c_char, c_int, c_void};

/// `struct rusage_info_v6` from `<sys/resource.h>` (macOS 14+ SDK). libc stops at v4.
/// CPU times are in Mach absolute time units; `ri_energy_nj` is nanojoules.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct RusageV6 {
    pub ri_uuid: [u8; 16],
    pub ri_user_time: u64,
    pub ri_system_time: u64,
    pub ri_pkg_idle_wkups: u64,
    pub ri_interrupt_wkups: u64,
    pub ri_pageins: u64,
    pub ri_wired_size: u64,
    pub ri_resident_size: u64,
    pub ri_phys_footprint: u64,
    pub ri_proc_start_abstime: u64,
    pub ri_proc_exit_abstime: u64,
    pub ri_child_user_time: u64,
    pub ri_child_system_time: u64,
    pub ri_child_pkg_idle_wkups: u64,
    pub ri_child_interrupt_wkups: u64,
    pub ri_child_pageins: u64,
    pub ri_child_elapsed_abstime: u64,
    pub ri_diskio_bytesread: u64,
    pub ri_diskio_byteswritten: u64,
    pub ri_cpu_time_qos: [u64; 7],
    pub ri_billed_system_time: u64,
    pub ri_serviced_system_time: u64,
    pub ri_logical_writes: u64,
    pub ri_lifetime_max_phys_footprint: u64,
    pub ri_instructions: u64,
    pub ri_cycles: u64,
    pub ri_billed_energy: u64,
    pub ri_serviced_energy: u64,
    pub ri_interval_max_phys_footprint: u64,
    pub ri_runnable_time: u64,
    pub ri_flags: u64,
    pub ri_user_ptime: u64,
    pub ri_system_ptime: u64,
    pub ri_pinstructions: u64,
    pub ri_pcycles: u64,
    pub ri_energy_nj: u64,
    pub ri_penergy_nj: u64,
    pub ri_secure_time_in_system: u64,
    pub ri_secure_ptime_in_system: u64,
    pub ri_neural_footprint: u64,
    pub ri_lifetime_max_neural_footprint: u64,
    pub ri_interval_max_neural_footprint: u64,
    pub ri_conclave_footprint: u64,
    pub ri_page_wait_time_mach: u64,
    pub ri_page_cache_hits: u64,
    pub ri_reserved: [u64; 6],
}

const RUSAGE_INFO_V6: c_int = 6;

/// All pids, into `out` (reused). Returns false if the call failed.
pub(crate) fn list_pids(out: &mut Vec<i32>) -> bool {
    crate::calls::count(crate::calls::Api::Libproc);
    // SAFETY: a size query with a null buffer returns the current pid count.
    let n = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if n <= 0 {
        return false;
    }
    // Headroom for processes spawned between the two calls.
    let cap = n as usize + 64;
    out.clear();
    out.resize(cap, 0);
    let bytes = (cap * size_of::<i32>()) as c_int;
    crate::calls::count(crate::calls::Api::Libproc);
    // SAFETY: `out` holds `cap` i32s, which is `bytes` bytes.
    let n = unsafe { libc::proc_listallpids(out.as_mut_ptr().cast(), bytes) };
    if n <= 0 {
        out.clear();
        return false;
    }
    out.truncate(n as usize);
    true
}

/// `PROC_PIDTASKALLINFO`. Fails for other users' processes unless root.
pub(crate) fn task_all_info(pid: i32) -> Option<libc::proc_taskallinfo> {
    // SAFETY: an all-zero proc_taskallinfo is valid (integers and char arrays).
    let mut info: libc::proc_taskallinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_taskallinfo>() as c_int;
    crate::calls::count(crate::calls::Api::Libproc);
    // SAFETY: `info` is `size` writable bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKALLINFO,
            0,
            (&mut info as *mut libc::proc_taskallinfo).cast::<c_void>(),
            size,
        )
    };
    (n == size).then_some(info)
}

/// `PROC_PIDTBSDINFO`: name, uid and start time. Like the calls above, it fails for
/// other users' processes when not root (measured on macOS 27).
pub(crate) fn bsd_info(pid: i32) -> Option<libc::proc_bsdinfo> {
    // SAFETY: an all-zero proc_bsdinfo is valid.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = size_of::<libc::proc_bsdinfo>() as c_int;
    crate::calls::count(crate::calls::Api::Libproc);
    // SAFETY: `info` is `size` writable bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast::<c_void>(),
            size,
        )
    };
    (n == size).then_some(info)
}

/// `proc_pid_rusage(RUSAGE_INFO_V6)`.
pub(crate) fn rusage(pid: i32) -> Option<RusageV6> {
    let mut ri = std::mem::MaybeUninit::<RusageV6>::zeroed();
    crate::calls::count(crate::calls::Api::Libproc);
    // SAFETY: `ri` is a writable rusage_info_v6, which is what flavor 6 fills.
    let rc = unsafe { libc::proc_pid_rusage(pid, RUSAGE_INFO_V6, ri.as_mut_ptr().cast()) };
    // SAFETY: zero-initialized and fully written on success; all fields are integers.
    (rc == 0).then(|| unsafe { ri.assume_init() })
}

/// The process name: `pbi_name` (up to 32 bytes), falling back to `pbi_comm`. Borrowed
/// from `bsd` when it is valid UTF-8, so a caller that keeps it pays one allocation for
/// its own copy (the processes collector's `Arc<str>`), not three.
pub(crate) fn name_of(bsd: &libc::proc_bsdinfo) -> Cow<'_, str> {
    let field = |buf: &[c_char]| {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        // SAFETY: `c_char` and `u8` have the same size and alignment, and `len` is within
        // `buf`.
        let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), len) };
        String::from_utf8_lossy(bytes)
    };
    let name = field(&bsd.pbi_name);
    if name.is_empty() {
        field(&bsd.pbi_comm)
    } else {
        name
    }
}

/// Whether the process name (as [`name_of`] gives it) starts with `prefix`, without
/// building the name.
pub(crate) fn name_starts_with(bsd: &libc::proc_bsdinfo, prefix: &[u8]) -> bool {
    let field: &[c_char] = if bsd.pbi_name.first().is_some_and(|&c| c != 0) {
        &bsd.pbi_name
    } else {
        &bsd.pbi_comm
    };
    let n = field.iter().position(|&c| c == 0).unwrap_or(field.len());
    let name = field.get(..n).unwrap_or_default();
    name.len() >= prefix.len() && name.iter().zip(prefix).all(|(&c, &p)| c as u8 == p)
}

/// Process start in microseconds since the Unix epoch.
pub(crate) fn start_time_us(bsd: &libc::proc_bsdinfo) -> i64 {
    (bsd.pbi_start_tvsec as i64)
        .saturating_mul(1_000_000)
        .saturating_add(bsd.pbi_start_tvusec as i64)
}

type ResponsibleFn = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;

/// `responsibility_get_pid_responsible_for_pid`, an undocumented libSystem export, looked
/// up with `dlsym` so a release that drops it degrades to `None` instead of failing to
/// load. Works without entitlements on macOS 27 for every pid (verified with the live
/// test in `self_cpu`).
///
/// An `appstore` build compiles the lookup out: App Review allows only public APIs, and
/// this symbol is not one. There `self.cpu` counts the app's own process without its
/// WebKit helpers, and Quit is unavailable anyway (D-065).
#[cfg(feature = "appstore")]
fn responsible_fn() -> Option<ResponsibleFn> {
    None
}

#[cfg(not(feature = "appstore"))]
fn responsible_fn() -> Option<ResponsibleFn> {
    static FN: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let addr = *FN.get_or_init(|| {
        let name: &CStr = c"responsibility_get_pid_responsible_for_pid";
        // SAFETY: RTLD_DEFAULT search for a NUL-terminated symbol name.
        let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
        (!p.is_null()).then_some(p as usize)
    });
    // SAFETY: the symbol has the C signature `pid_t (pid_t)`.
    addr.map(|a| unsafe { std::mem::transmute::<usize, ResponsibleFn>(a) })
}

/// The pid macOS holds responsible for `pid` (the app for its XPC helpers), or `None`
/// when the lookup is unavailable or fails.
pub(crate) fn responsible_pid(pid: i32) -> Option<i32> {
    let f = responsible_fn()?;
    crate::calls::count(crate::calls::Api::Libproc);
    // SAFETY: plain value-in, value-out call.
    let r = unsafe { f(pid) };
    (r > 0).then_some(r)
}

/// `struct proc_uniqidentifierinfo` from `<sys/proc_info.h>`; libc does not bind it.
#[repr(C)]
#[derive(Clone, Copy)]
struct UniqIdentifierInfo {
    p_uuid: [u8; 16],
    p_uniqueid: u64,
    p_puniqueid: u64,
    p_idversion: i32,
    p_orig_ppidversion: i32,
    p_reserve2: u64,
    p_reserve3: u64,
}

const PROC_PIDUNIQIDENTIFIERINFO: c_int = 17;

/// The process's 64-bit unique id (`p_uniqueid`, never reused within a boot), the value
/// NetworkStatistics reports as `uniqueProcessID`.
pub(crate) fn unique_id(pid: i32) -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<UniqIdentifierInfo>::zeroed();
    let size = size_of::<UniqIdentifierInfo>() as c_int;
    crate::calls::count(crate::calls::Api::Libproc);
    // SAFETY: `info` is `size` writable bytes, the struct flavor 17 fills.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDUNIQIDENTIFIERINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    // SAFETY: zero-initialized and, on success, fully written; all fields are integers.
    (n == size).then(|| unsafe { info.assume_init() }.p_uniqueid)
}

/// The executable's path (`proc_pidpath`).
fn pid_path(pid: i32) -> Option<String> {
    let mut buf = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    crate::calls::count(crate::calls::Api::Libproc);
    // SAFETY: `buf` is PROC_PIDPATHINFO_MAXSIZE writable bytes, the size the call needs.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let bytes = buf.get(..usize::try_from(n).ok()?)?;
    (!bytes.is_empty()).then(|| String::from_utf8_lossy(bytes).into_owned())
}

/// Most bytes [`argv0`] reads: macOS's `ARGMAX`.
const ARGS_MAX: usize = 1 << 20;

/// `argv[0]` from `KERN_PROCARGS2`: an `int argc`, the exec path, NUL padding, then
/// argv and the environment. The buffer is sized by a size query first: given less than
/// the whole area, the call still succeeds but fills the buffer from the area's end, so
/// a short buffer holds zeros or environment, not the exec path (measured on macOS 27
/// with a 4 KiB buffer and a 5.6 KiB area). Allocates the area's size once per process.
fn argv0(pid: i32) -> Option<String> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size = 0usize;
    crate::calls::count(crate::calls::Api::Kernel);
    // SAFETY: a size query: null output buffer, `size` receives the needed size.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size == 0 || size > ARGS_MAX {
        return None;
    }
    let mut buf = vec![0u8; size];
    crate::calls::count(crate::calls::Api::Kernel);
    // SAFETY: `buf` has `size` writable bytes; the kernel writes at most `size`.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    parse_argv0(buf.get(..size)?).map(str::to_owned)
}

/// `argv[0]` from a `KERN_PROCARGS2` buffer.
fn parse_argv0(buf: &[u8]) -> Option<&str> {
    let rest = buf.get(size_of::<c_int>()..)?;
    let exec_end = rest.iter().position(|&b| b == 0)?;
    let rest = rest.get(exec_end..)?;
    let start = rest.iter().position(|&b| b != 0)?;
    let rest = rest.get(start..)?;
    let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
    std::str::from_utf8(rest.get(..end)?).ok()
}

/// Longest identity kept, in characters; a longer one is cut.
pub(crate) const IDENTITY_MAX_CHARS: usize = 64;

/// The name of the outermost `X.app` bundle in `path`, unless the path runs through an
/// Xcode toolchain (`.app/Contents/Developer/`) or a Python framework, whose binaries
/// are command-line tools that happen to live inside an app (Xcode-shim `git`,
/// `/usr/bin/python3`).
fn outer_app(path: &str) -> Option<&str> {
    if path.contains(".app/Contents/Developer/") || in_python_framework(path) {
        return None;
    }
    path.split('/')
        .find_map(|c| c.strip_suffix(".app"))
        .filter(|n| !n.is_empty())
}

/// Whether `path` runs through a `Python*.framework` (Xcode's `Python3.framework`, the
/// python.org `Python.framework`).
fn in_python_framework(path: &str) -> bool {
    path.split('/')
        .any(|c| c.starts_with("Python") && c.ends_with(".framework"))
}

fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

/// The app a network flow is charged to, from what libproc says about its process:
/// `path` (`proc_pidpath`), `argv0` (`KERN_PROCARGS2`), `name` (`pbi_name`) and the
/// executable path of the process macOS holds responsible for it (`None` when the
/// lookup is unavailable, as in an `appstore` build). Verified against the step-0
/// identity probe on macOS 27 (D-089):
///
/// 1. An XPC service (`.xpc/` in its path, like `com.apple.WebKit.Networking`) is
///    charged to its responsible process's app. Only XPC services: every command-line
///    tool's responsible process is the terminal.
/// 2. Inside an `.app`: the outermost app's name (Chrome's helpers are "Google
///    Chrome", an app extension is its app), except through the Xcode toolchain or a
///    Python framework (see [`outer_app`]).
/// 3. `argv[0]` up to its first whitespace, basename (`npm exec cowsay hi` is `npm`,
///    Claude Code's `2.1.290` binary is `claude`). When `argv[0]` is the exec path
///    itself, the whole basename, so a path with spaces is not cut.
/// 4. `pbi_name`.
///
/// Then aliases: `git-remote-http(s)` and anything under `/git-core/` are `git`, and a
/// Python framework's `Python` is `python3`. The result is trimmed and cut to
/// [`IDENTITY_MAX_CHARS`]; `None` when nothing names it.
pub(crate) fn identity_rule(
    path: &str,
    argv0: &str,
    name: &str,
    responsible_path: Option<&str>,
) -> Option<String> {
    let via_responsible = path
        .contains(".xpc/")
        .then_some(responsible_path)
        .flatten()
        .and_then(outer_app);
    let chosen = via_responsible
        .or_else(|| outer_app(path))
        .or_else(|| {
            let a = if argv0 == path {
                argv0
            } else {
                argv0.split_whitespace().next().unwrap_or_default()
            };
            Some(basename(a)).filter(|b| !b.trim().is_empty())
        })
        .unwrap_or(name);
    let aliased = if chosen == "git-remote-http"
        || chosen == "git-remote-https"
        || path.contains("/git-core/")
    {
        "git"
    } else if chosen == "Python" && in_python_framework(path) {
        "python3"
    } else {
        chosen
    };
    let trimmed = aliased.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(IDENTITY_MAX_CHARS).collect())
}

/// Whether `path` is an app bundle's main executable: the outermost `X.app` (as
/// [`outer_app`] finds it) followed directly by `Contents/MacOS/`, with no bundle nested
/// further in. Chrome's helpers (`…/Helpers/Google Chrome Helper.app/…`), XPC services
/// and app extensions are not.
pub(crate) fn is_app_main(path: &str) -> bool {
    if outer_app(path).is_none() {
        return false;
    }
    let parts: Vec<&str> = path.split('/').collect();
    let Some(i) = parts
        .iter()
        .position(|c| c.len() > 4 && c.ends_with(".app"))
    else {
        return false;
    };
    parts.get(i + 1) == Some(&"Contents")
        && parts.get(i + 2) == Some(&"MacOS")
        && parts.len() == i + 4
}

/// The app a process belongs to and whether it is that app's main executable
/// ([`identity_rule`] and [`is_app_main`] over what libproc reads), `None` when its path
/// cannot be read. `name` is its `pbi_name`, already read by the caller. Allocates;
/// called once per process, when the processes collector first sees it.
pub(crate) fn process_app(pid: i32, name: &str) -> Option<(String, bool)> {
    let path = pid_path(pid)?;
    let responsible = if path.contains(".xpc/") {
        responsible_pid(pid)
            .filter(|&r| r != pid)
            .and_then(pid_path)
    } else {
        None
    };
    // argv only decides for a process outside any app bundle; skip the read otherwise.
    let bundled =
        responsible.as_deref().and_then(outer_app).is_some() || outer_app(&path).is_some();
    let argv0 = if bundled {
        String::new()
    } else {
        argv0(pid).unwrap_or_default()
    };
    let app = identity_rule(&path, &argv0, name, responsible.as_deref())?;
    Some((app, is_app_main(&path)))
}

/// The app identity of a live `pid` ([`identity_rule`] over what libproc reads), or
/// `None` when the process is gone or unreadable. When `upid` is non-zero and the
/// process now at `pid` has another unique id, the pid was reused and the answer is
/// `None`: the caller falls back to what NetworkStatistics recorded. The unique id is
/// checked again after the reads, so a pid that exits and is reused while they run
/// cannot lend the new process's path or argv to the old one. Allocates; called once per
/// process from a NetworkStatistics callback, not per tick.
pub(crate) fn app_identity(pid: i32, upid: u64) -> Option<String> {
    let same = || upid == 0 || unique_id(pid) == Some(upid);
    if !same() {
        return None;
    }
    let path = pid_path(pid)?;
    let argv0 = argv0(pid).unwrap_or_default();
    let name = bsd_info(pid)
        .map(|b| name_of(&b).into_owned())
        .unwrap_or_default();
    let responsible = if path.contains(".xpc/") {
        responsible_pid(pid)
            .filter(|&r| r != pid)
            .and_then(pid_path)
    } else {
        None
    };
    if !same() {
        return None;
    }
    identity_rule(&path, &argv0, &name, responsible.as_deref())
}

/// User name for `uid`, or the number when it has none.
pub(crate) fn user_name(uid: u32) -> String {
    let mut pw = std::mem::MaybeUninit::<libc::passwd>::zeroed();
    let mut buf = [0 as c_char; 1024];
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers are valid for the call and `buf.len()` is the buffer size.
    let rc = unsafe {
        libc::getpwuid_r(
            uid,
            pw.as_mut_ptr(),
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
    if rc == 0 && !result.is_null() {
        // SAFETY: on success `result` points at `pw`, whose pw_name points into `buf`.
        let name = unsafe { CStr::from_ptr((*result).pw_name) };
        if let Ok(s) = name.to_str() {
            return s.to_owned();
        }
    }
    uid.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rusage_struct_matches_sdk_size() {
        // sizeof and offsetof from a C program against the macOS 27 SDK.
        assert_eq!(size_of::<RusageV6>(), 464);
        assert_eq!(std::mem::offset_of!(RusageV6, ri_energy_nj), 336);
    }

    /// One row per process the step-0 identity probe saw on macOS 27 (scratchpad
    /// `identity-probe/`, D-089): path, argv[0], pbi_name, responsible path.
    #[test]
    fn identity_rule_matches_the_probe_table() {
        let chrome = "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/154.0.8037.98/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper";
        let webkit = "/System/Volumes/Preboot/Cryptexes/Incoming/OS/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.Networking.xpc/Contents/MacOS/com.apple.WebKit.Networking";
        let safari = "/System/Volumes/Preboot/Cryptexes/App/System/Applications/Safari.app/Contents/MacOS/Safari";
        let orb = "/Applications/OrbStack.app/Contents/Frameworks/OrbStack Helper.app/Contents/MacOS/OrbStack Helper";
        let news = "/System/Applications/News.app/Contents/PlugIns/NewsTodayIntents.appex/Contents/MacOS/NewsTodayIntents";
        let xcode_git =
            "/Applications/Xcode.app/Contents/Developer/usr/libexec/git-core/git-remote-http";
        let brew_git = "/opt/homebrew/Cellar/git/2.56.0/libexec/git-core/git-remote-http";
        let xcode_py = "/Applications/Xcode.app/Contents/Developer/Library/Frameworks/Python3.framework/Versions/3.9/Resources/Python.app/Contents/MacOS/Python";
        let ghostty = "/Applications/Ghostty.app/Contents/MacOS/ghostty";
        let rows: &[(&str, &str, &str, Option<&str>, &str)] = &[
            (
                chrome,
                chrome,
                "Google Chrome Helper",
                None,
                "Google Chrome",
            ),
            (
                webkit,
                "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.Networking.xpc/Contents/MacOS/com.apple.WebKit.Networking",
                "com.apple.WebKit.Networking",
                Some(safari),
                "Safari",
            ),
            (
                "/Users/me/.local/share/claude/versions/2.1.290",
                "claude",
                "2.1.290",
                Some(ghostty),
                "claude",
            ),
            (orb, orb, "OrbStack Helper", None, "OrbStack"),
            (news, news, "NewsTodayIntents", None, "News"),
            (
                xcode_git,
                "/Applications/Xcode.app/Contents/Developer/usr/libexec/git-core/git-remote-https",
                "git-remote-http",
                Some(ghostty),
                "git",
            ),
            (
                brew_git,
                "/opt/homebrew/opt/git/libexec/git-core/git-remote-https",
                "git-remote-http",
                None,
                "git",
            ),
            (xcode_py, xcode_py, "Python", Some(ghostty), "python3"),
            (
                "/Users/me/.local/share/mise/installs/python/3.13.16/bin/python3.13",
                "python3",
                "python3.13",
                None,
                "python3",
            ),
            (
                "/Users/me/.local/share/mise/installs/node/24.21.0/bin/node",
                "npm exec cowsay hi",
                "node",
                None,
                "npm",
            ),
            (
                "/usr/bin/curl",
                "/usr/bin/curl",
                "curl",
                Some(ghostty),
                "curl",
            ),
            // An XPC service responsible for itself keeps its own name.
            (
                "/System/Library/PrivateFrameworks/Categories.framework/Versions/A/XPCServices/CategoriesService.xpc/Contents/MacOS/CategoriesService",
                "/System/Library/PrivateFrameworks/Categories.framework/Versions/A/XPCServices/CategoriesService.xpc/Contents/MacOS/CategoriesService",
                "CategoriesService",
                None,
                "CategoriesService",
            ),
        ];
        for &(path, argv0, name, resp, want) in rows {
            assert_eq!(
                identity_rule(path, argv0, name, resp).as_deref(),
                Some(want),
                "{path}"
            );
        }
    }

    #[test]
    fn app_main_is_the_outer_bundles_own_executable() {
        assert!(is_app_main(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        ));
        assert!(is_app_main(
            "/System/Applications/Mail.app/Contents/MacOS/Mail"
        ));
        // Helpers, XPC services and extensions sit in nested bundles.
        assert!(!is_app_main(
            "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Versions/1/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)"
        ));
        assert!(!is_app_main(
            "/Applications/Slack.app/Contents/Library/LoginItems/Slack Login.app/Contents/MacOS/Slack Login"
        ));
        assert!(!is_app_main(
            "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.Networking.xpc/Contents/MacOS/com.apple.WebKit.Networking"
        ));
        // A tool inside an app's bundle, and the Xcode toolchain, are not the app.
        assert!(!is_app_main(
            "/Applications/Docker.app/Contents/Resources/bin/docker"
        ));
        assert!(!is_app_main(
            "/Applications/Xcode.app/Contents/Developer/usr/bin/git"
        ));
        assert!(!is_app_main("/usr/bin/curl"));
    }

    #[test]
    fn identity_rule_falls_back_and_caps() {
        // appstore: no responsible lookup, so an XPC service keeps its own name.
        let wk = "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.Networking.xpc/Contents/MacOS/com.apple.WebKit.Networking";
        assert_eq!(
            identity_rule(wk, wk, "com.apple.WebKit.Networking", None).as_deref(),
            Some("com.apple.WebKit.Networking")
        );
        // An exec path with spaces is not cut at the space.
        let spaced = "/Users/me/My Tools/sync agent";
        assert_eq!(
            identity_rule(spaced, spaced, "sync agent", None).as_deref(),
            Some("sync agent")
        );
        // No argv[0]: pbi_name. Nothing at all: none.
        assert_eq!(
            identity_rule("/usr/libexec/x", "", "xd", None).as_deref(),
            Some("xd")
        );
        assert_eq!(identity_rule("", "", "  ", None), None);
        let long = "a".repeat(200);
        assert_eq!(
            identity_rule("", &long, "", None).map(|s| s.chars().count()),
            Some(IDENTITY_MAX_CHARS)
        );
    }

    #[test]
    fn parses_argv0_from_procargs2() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend(b"/usr/bin/curl\0\0\0\0curl\0-s\0PATH=/bin\0");
        assert_eq!(parse_argv0(&buf), Some("curl"));
        // Truncated inside argv[0]: what fits.
        assert_eq!(parse_argv0(&buf[..buf.len() - 15]), Some("cur"));
        assert_eq!(parse_argv0(&buf[..10]), None);
        assert_eq!(parse_argv0(&[]), None);
    }

    #[test]
    fn identifies_own_process() {
        let me = std::process::id() as i32;
        let upid = unique_id(me).unwrap();
        assert!(upid > 0);
        let id = app_identity(me, upid).unwrap();
        assert!(id.starts_with("kelvo_collect"), "{id}");
        assert_eq!(
            app_identity(me, upid + 1),
            None,
            "another process's unique id"
        );
        // The test runner's environment is several KiB, more than a fixed small buffer
        // would hold, which then reads zeros instead of the exec path.
        let env: usize = std::env::vars_os()
            .map(|(k, v)| k.len() + v.len() + 2)
            .sum();
        assert_eq!(
            argv0(me),
            std::env::args().next(),
            "argv[0] with {env} bytes of environment"
        );
    }

    #[test]
    fn reads_own_process() {
        let me = std::process::id() as i32;
        let mut pids = Vec::new();
        assert!(list_pids(&mut pids));
        assert!(pids.contains(&me));
        let bsd = bsd_info(me).unwrap();
        let name = name_of(&bsd);
        assert!(!name.is_empty());
        assert!(name_starts_with(&bsd, name.as_bytes()));
        assert!(name_starts_with(&bsd, &name.as_bytes()[..1]));
        assert!(name_starts_with(&bsd, b""));
        assert!(!name_starts_with(&bsd, b"com.apple.WebKit.not-this-test"));
        let mut longer = name.as_bytes().to_vec();
        longer.push(b'x');
        assert!(!name_starts_with(&bsd, &longer));
        assert!(start_time_us(&bsd) > 1_600_000_000_000_000);
        assert!(rusage(me).unwrap().ri_phys_footprint > 0);
        assert!(task_all_info(me).is_some());
    }
}
