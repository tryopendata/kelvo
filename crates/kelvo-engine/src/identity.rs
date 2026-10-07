//! The local host's identity (architecture.md infra 2): a random UUID created on first run
//! and kept in `host-id` in the app data directory and in the store's `hosts` table. It
//! lives here rather than in the app shell so the v4 headless agent derives the same id
//! for its handshake (D-071).
//!
//! On later runs the file wins. If the file is missing or unreadable but the store still
//! names a local host, that id is reused and the file rewritten, so losing the file does
//! not orphan the history. The store has at most one local host (a partial unique index,
//! D-064), so the recovery is never a guess between two. Only when neither has one is a
//! new id made.
//!
//! The id is bound to the Mac (D-071). The file's second line is a `machine=` token
//! ([`machine_binding`]: a hash of the id and the hardware UUID). When it does not match
//! this Mac, the data directory was cloned from another one (Migration Assistant, a disk
//! clone or restore to new hardware): this Mac gets a new id rather than share the
//! other's, and the copied store's old local host stays in the file as a non-local host
//! (the new local registration demotes it). A file without the line (or a Mac whose
//! hardware UUID cannot be read) is trusted and the line added. A cloned store whose
//! `host-id` file was lost too has nothing to check against, so its local host is reused;
//! the store's `HostConflict` and the v4 handshake are the backstop for that case.
//!
//! None of this stops the launch. An id that cannot be written (an unwritable or full data
//! directory) is kept in memory for this run and logged, like a store that cannot open.
//! The next run recovers it from the store if history was written under it, or makes a
//! new one.
//!
//! The file format, the binding input and its hash are a one-way door: changing any of
//! them makes every existing install look cloned.

use std::path::Path;

use kelvo_schema::{HostId, HostRecord};
use uuid::Uuid;

pub const HOST_ID_FILE: &str = "host-id";
const MACHINE_PREFIX: &str = "machine=";

/// Where the id came from, for the startup log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdSource {
    File,
    /// The file was missing or unreadable; the store's local host was reused.
    Store,
    /// First run (or both lost): a new id.
    Created,
    /// The id on disk was made on another Mac: a new id for this one.
    Cloned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalId {
    pub id: HostId,
    pub source: IdSource,
    /// The id and its binding are on disk. False when writing them failed: the id lasts
    /// for this run only.
    pub persisted: bool,
}

/// What the file says: the id and the machine token, each `None` when absent.
struct IdFile {
    id: Option<HostId>,
    machine: Option<String>,
}

fn read_file(path: &Path) -> IdFile {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            tracing::warn!("reading {}: {e}; recovering the host id", path.display());
            String::new()
        }
    };
    let mut lines = text.lines().map(str::trim);
    let id = lines.next().filter(|l| !l.is_empty()).and_then(|l| {
        Uuid::parse_str(l)
            .map(HostId)
            .map_err(|e| tracing::warn!("{} is unreadable ({e}); replacing it", path.display()))
            .ok()
    });
    let machine = lines
        .find_map(|l| l.strip_prefix(MACHINE_PREFIX))
        .map(str::to_owned);
    IdFile { id, machine }
}

/// Reads the host id from `dir/host-id`, or recovers or creates it as described in the
/// module docs. `stored_local` is the store's local host (`Reader::local_host`), `None`
/// when there is none or history is unavailable. `binding` gives this Mac's token for an
/// id ([`machine_binding`]), `None` when it cannot be computed.
pub fn load_or_create(
    dir: &Path,
    stored_local: Option<&HostRecord>,
    binding: impl Fn(HostId) -> Option<String>,
) -> LocalId {
    let path = dir.join(HOST_ID_FILE);
    let file = read_file(&path);
    let candidate = file
        .id
        .map(|id| (id, IdSource::File))
        .or_else(|| stored_local.map(|h| (h.id, IdSource::Store)));
    let (id, source) = match candidate {
        Some((id, source)) => match (&file.machine, binding(id)) {
            (Some(theirs), Some(ours)) if *theirs != ours => {
                tracing::warn!(
                    old = %id,
                    "the host id was made on another Mac (a cloned or migrated data directory); this Mac gets its own"
                );
                (HostId(Uuid::new_v4()), IdSource::Cloned)
            }
            _ => (id, source),
        },
        None => (HostId(Uuid::new_v4()), IdSource::Created),
    };
    let machine = binding(id);
    let up_to_date = source == IdSource::File && (machine.is_none() || machine == file.machine);
    if up_to_date {
        // Owner-only (D-074); a file from before that is tightened here.
        if let Err(e) = kelvo_store::restrict(&path) {
            tracing::warn!("restricting {}: {e}", path.display());
        }
        return LocalId {
            id,
            source,
            persisted: true,
        };
    }
    let mut text = format!("{id}\n");
    if let Some(m) = machine {
        text.push_str(&format!("{MACHINE_PREFIX}{m}\n"));
    }
    let persisted = match write_atomically(dir, &path, &text) {
        Ok(()) => true,
        Err(e) => {
            tracing::error!(host = %id, "cannot save the host id, using it for this run only: {e}");
            false
        }
    };
    LocalId {
        id,
        source,
        persisted,
    }
}

/// Writes through a temporary file and a rename. The error names the step that failed;
/// it only goes to the log.
fn write_atomically(dir: &Path, path: &Path, text: &str) -> Result<(), String> {
    kelvo_store::create_private_dir(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let tmp = path.with_extension("tmp");
    kelvo_store::write_private(&tmp, text.as_bytes())
        .map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("renaming to {}: {e}", path.display()))?;
    Ok(())
}

/// A token tying `id` to this Mac: SHA-256 of a fixed prefix, the id and the hardware
/// UUID (`kelvo_collect::macos::platform_uuid`), in hex. Salting with the id means the
/// file never holds the hardware UUID or anything that matches across installs. `None`
/// when the hardware UUID is unreadable, and always off macOS, where the file is trusted
/// as it is.
pub fn machine_binding(id: HostId) -> Option<String> {
    binding::machine_binding(id)
}

#[cfg(target_os = "macos")]
mod binding {
    use std::ffi::c_void;

    use kelvo_schema::HostId;

    // CommonCrypto lives in libSystem, which every binary links.
    unsafe extern "C" {
        fn CC_SHA256(data: *const c_void, len: u32, md: *mut u8) -> *mut u8;
    }

    pub(super) fn machine_binding(id: HostId) -> Option<String> {
        token(id, &kelvo_collect::macos::platform_uuid()?)
    }

    /// The binding for `id` on a Mac whose hardware UUID is `platform`.
    pub(super) fn token(id: HostId, platform: &str) -> Option<String> {
        let mut input = b"kelvo host-id binding v1\0".to_vec();
        input.extend_from_slice(id.0.as_bytes());
        input.extend_from_slice(platform.as_bytes());
        let len = u32::try_from(input.len()).ok()?;
        let mut md = [0u8; 32];
        // SAFETY: `input` has `len` readable bytes and `md` the 32 bytes SHA-256 writes.
        unsafe { CC_SHA256(input.as_ptr().cast(), len, md.as_mut_ptr()) };
        Some(md.iter().map(|b| format!("{b:02x}")).collect())
    }
}

#[cfg(not(target_os = "macos"))]
mod binding {
    use kelvo_schema::HostId;

    pub(super) fn machine_binding(_id: HostId) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use kelvo_schema::{HostInfo, OsKind};

    use super::*;

    fn temp_dir(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("kelvo-engine-{name}-"))
            .tempdir()
            .unwrap()
    }

    fn record(id: HostId, is_local: bool) -> HostRecord {
        HostRecord {
            id,
            is_local,
            display_name: "Mac".into(),
            info: HostInfo {
                os: OsKind::MacOs,
                os_version: "27.0".into(),
                model: None,
                chip: None,
                chip_known: true,
                cpu_topology: Vec::new(),
                mem_total_bytes: 0,
                boot_time_ms: 0,
                gpu_dvfs_mhz: Vec::new(),
                boot_mounts: Vec::new(),
            },
        }
    }

    /// This Mac's binding in the tests: Mac "a".
    fn mac_a(id: HostId) -> Option<String> {
        Some(format!("a-{id}"))
    }

    fn mac_b(id: HostId) -> Option<String> {
        Some(format!("b-{id}"))
    }

    fn unknown_mac(_: HostId) -> Option<String> {
        None
    }

    fn load(dir: &Path, stored: Option<&HostRecord>) -> (HostId, IdSource) {
        let l = load_or_create(dir, stored, mac_a);
        assert!(l.persisted);
        (l.id, l.source)
    }

    #[test]
    fn first_run_creates_and_later_runs_read_the_same_id() {
        let tmp = temp_dir("id-first");
        let dir = tmp.path();
        let (a, src) = load(dir, None);
        assert_eq!(src, IdSource::Created);
        let text = std::fs::read_to_string(dir.join(HOST_ID_FILE)).unwrap();
        assert_eq!(text, format!("{a}\nmachine=a-{a}\n"));

        let (b, src) = load(dir, None);
        assert_eq!((b, src), (a, IdSource::File));
        // The file wins over the store.
        let other = record(HostId(Uuid::new_v4()), true);
        assert_eq!(load(dir, Some(&other)), (a, IdSource::File));
    }

    #[test]
    fn missing_file_reuses_the_stores_local_host() {
        let tmp = temp_dir("id-recover");
        let dir = tmp.path();
        let local = HostId(Uuid::new_v4());
        assert_eq!(
            load(dir, Some(&record(local, true))),
            (local, IdSource::Store)
        );
        assert_eq!(load(dir, None), (local, IdSource::File));
    }

    #[test]
    fn corrupt_file_is_replaced() {
        let tmp = temp_dir("id-corrupt");
        let dir = tmp.path();
        std::fs::write(dir.join(HOST_ID_FILE), "not a uuid").unwrap();
        let local = HostId(Uuid::new_v4());
        assert_eq!(
            load(dir, Some(&record(local, true))),
            (local, IdSource::Store)
        );
        std::fs::write(dir.join(HOST_ID_FILE), "").unwrap();
        let (id, src) = load(dir, None);
        assert_eq!(src, IdSource::Created);
        assert_ne!(id, local);
    }

    #[test]
    fn creates_the_directory() {
        let tmp = temp_dir("id-mkdir");
        let dir = tmp.path().join("nested");
        let (id, _) = load(&dir, None);
        assert_eq!(load(&dir, None), (id, IdSource::File));
    }

    /// D-071: a data directory copied to another Mac (Migration Assistant, a disk clone)
    /// gives that Mac a new id instead of a second copy of this one's, and the copied
    /// store's local host is not reused either.
    #[test]
    fn an_id_from_another_mac_is_replaced() {
        let tmp = temp_dir("id-cloned");
        let dir = tmp.path();
        let original = load_or_create(dir, None, mac_a).id;
        let copied_store = record(original, true);

        let on_b = load_or_create(dir, Some(&copied_store), mac_b);
        assert_eq!(on_b.source, IdSource::Cloned);
        assert!(on_b.persisted);
        assert_ne!(on_b.id, original);
        // From then on Mac b reads its own id.
        let again = load_or_create(dir, Some(&copied_store), mac_b);
        assert_eq!((again.id, again.source), (on_b.id, IdSource::File));
    }

    #[test]
    fn a_file_without_a_binding_is_trusted_and_bound() {
        let tmp = temp_dir("id-unbound");
        let dir = tmp.path();
        let id = HostId(Uuid::new_v4());
        std::fs::write(dir.join(HOST_ID_FILE), format!("{id}\n")).unwrap();
        assert_eq!(load(dir, None), (id, IdSource::File));
        let text = std::fs::read_to_string(dir.join(HOST_ID_FILE)).unwrap();
        assert_eq!(text, format!("{id}\nmachine=a-{id}\n"));

        // Where the hardware UUID cannot be read, the file is trusted as it is.
        let l = load_or_create(dir, None, unknown_mac);
        assert_eq!((l.id, l.source, l.persisted), (id, IdSource::File, true));
    }

    /// An unwritable data directory (root-owned after a `sudo` run, a full disk) never
    /// stops the launch: the id lives in memory for this run.
    #[test]
    fn an_id_that_cannot_be_saved_is_kept_in_memory() {
        let tmp = temp_dir("id-unwritable");
        let parent = tmp.path();
        // A path under a regular file: reading and creating both fail, whoever runs this.
        std::fs::write(parent.join("file"), "").unwrap();
        let dir = parent.join("file").join("data");

        let l = load_or_create(&dir, None, mac_a);
        assert_eq!(l.source, IdSource::Created);
        assert!(!l.persisted);

        let stored = HostId(Uuid::new_v4());
        let l = load_or_create(&dir, Some(&record(stored, true)), mac_a);
        assert_eq!(
            (l.id, l.source, l.persisted),
            (stored, IdSource::Store, false)
        );
    }

    /// The binding is a one-way door: every existing `host-id` file holds a token made
    /// this way, so a change here would make every install look cloned. The expected
    /// value is SHA-256 of the prefix, the id's 16 bytes and the hardware UUID, computed
    /// independently (Python `hashlib`).
    #[cfg(target_os = "macos")]
    #[test]
    fn the_binding_hash_is_stable() {
        let id = HostId(Uuid::parse_str("6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b").unwrap());
        assert_eq!(
            binding::token(id, "00000000-0000-0000-0000-000000000001").as_deref(),
            Some("316cbedc91ce4a52c6fd77bab18dad5c9193f176546ab22950b512bc44839ffa")
        );
    }

    /// On a real Mac the hardware UUID reads, so the binding exists and is stable.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "reads IOPlatformUUID from this Mac's registry; run by hand"]
    fn this_mac_has_a_binding() {
        let id = HostId(Uuid::new_v4());
        let a = machine_binding(id);
        assert!(a.is_some());
        assert_eq!(a, machine_binding(id));
    }
}
