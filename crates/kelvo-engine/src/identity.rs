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
//! On Linux the binding is made from `/etc/machine-id` instead ([`linux_binding`],
//! D-116), and a system agent adds a separate `hw=` line from the firmware's DMI UUID or
//! device-tree serial, which catches a hypervisor clone that kept the machine-id. Each
//! tagged line is compared only when both the file and this run have it, so firmware
//! that becomes readable or unreadable (a kernel or permission change) never looks like
//! a clone; a line missing from the file is added. A host without a usable machine-id
//! has no binding, like a Mac whose hardware UUID cannot be read. [`reset`] mints a new
//! id on purpose (`kelvo-agent reset-identity`).
//!
//! The file format, the binding input and its hash are a one-way door: changing any of
//! them makes every existing install look cloned.

use std::path::Path;

use kelvo_collect::host_info::machine;
use kelvo_schema::{HostId, HostRecord};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::HistoryStore;

pub const HOST_ID_FILE: &str = "host-id";
/// The component a bare binding token (a Mac's) is stored under.
const MACHINE_TAG: &str = "machine";
/// A binding component that is never written: a token an older file's `machine=` line
/// may hold for this machine ([`linux_binding`]).
const LEGACY_TAG: &str = "legacy";

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

/// What the file says: the id (`None` when absent) and its `tag=value` binding lines.
struct IdFile {
    id: Option<HostId>,
    binding: Vec<(String, String)>,
}

/// A binding as a binding function returns it, parsed. The text is either one bare
/// token (the Mac's hardware-UUID hash, stored as `machine=`) or whitespace-separated
/// `tag=value` components ([`linux_binding`]). `legacy=` components are not written:
/// they are the tokens an earlier file's single `machine=` line may hold for this host.
#[derive(Debug, Default, PartialEq, Eq)]
struct Binding {
    components: Vec<(String, String)>,
    legacy: Vec<String>,
}

impl Binding {
    fn parse(text: &str) -> Self {
        let text = text.trim();
        if !text.contains('=') {
            return Self {
                components: vec![(MACHINE_TAG.to_owned(), text.to_owned())],
                legacy: Vec::new(),
            };
        }
        let mut binding = Self::default();
        for (tag, value) in text.split_whitespace().filter_map(|c| c.split_once('=')) {
            if tag == LEGACY_TAG {
                binding.legacy.push(value.to_owned());
            } else {
                binding.components.push((tag.to_owned(), value.to_owned()));
            }
        }
        binding
    }

    fn get(&self, tag: &str) -> Option<&str> {
        self.components
            .iter()
            .find(|(t, _)| t == tag)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the file's lines could have been written on this machine: every tag both
    /// sides have agrees. A legacy `machine=` token matches when it is one of this
    /// machine's legacy tokens. A tag only one side has says nothing.
    fn matches(&self, file: &[(String, String)]) -> bool {
        file.iter().all(|(tag, theirs)| match self.get(tag) {
            None => true,
            Some(ours) => {
                ours == theirs || (tag == MACHINE_TAG && self.legacy.iter().any(|l| l == theirs))
            }
        })
    }

    /// The lines to write: this run's components, then any the file holds that this run
    /// could not read (a firmware id that is unreadable today), so they still guard
    /// against a clone later.
    fn merged(&self, file: &[(String, String)]) -> Vec<(String, String)> {
        let mut lines = self.components.clone();
        for (tag, value) in file {
            if self.get(tag).is_none() {
                lines.push((tag.clone(), value.clone()));
            }
        }
        lines
    }
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
    let mut binding: Vec<(String, String)> = Vec::new();
    for (tag, value) in lines.filter_map(|l| l.split_once('=')) {
        if tag != LEGACY_TAG && !binding.iter().any(|(t, _)| t == tag) {
            binding.push((tag.to_owned(), value.to_owned()));
        }
    }
    IdFile { id, binding }
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
        Some((id, source)) => match binding(id).map(|b| Binding::parse(&b)) {
            Some(ours) if !ours.matches(&file.binding) => {
                tracing::warn!(
                    old = %id,
                    "the host id was made on another machine (a cloned or migrated data directory); this one gets its own"
                );
                (HostId(Uuid::new_v4()), IdSource::Cloned)
            }
            _ => (id, source),
        },
        None => (HostId(Uuid::new_v4()), IdSource::Created),
    };
    // A new id keeps nothing of the old file's binding.
    let kept: &[(String, String)] = if source == IdSource::Cloned {
        &[]
    } else {
        &file.binding
    };
    let lines = binding(id).map_or_else(|| kept.to_vec(), |b| Binding::parse(&b).merged(kept));
    let up_to_date = source == IdSource::File && lines == file.binding;
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
    let persisted = match write_atomically(dir, &path, &file_text(id, &lines)) {
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

/// The `host-id` file's text: the id, then one `tag=value` line per binding component.
fn file_text(id: HostId, lines: &[(String, String)]) -> String {
    let mut text = format!("{id}\n");
    for (tag, value) in lines {
        text.push_str(&format!("{tag}={value}\n"));
    }
    text
}

/// Gives the data directory `dir` a new host id with its `binding`, replacing whatever
/// `host-id` held (`kelvo-agent reset-identity`, D-116). The next [`open_local`] reads
/// it from the file; registering it demotes the store's old local host to a non-local
/// one (D-064), so the history recorded under the old id stays, as with a clone.
pub fn reset(dir: &Path, binding: impl Fn(HostId) -> Option<String>) -> std::io::Result<HostId> {
    let id = HostId(Uuid::new_v4());
    let path = dir.join(HOST_ID_FILE);
    let lines = binding(id).map_or_else(Vec::new, |b| Binding::parse(&b).components);
    write_atomically(dir, &path, &file_text(id, &lines)).map_err(std::io::Error::other)?;
    Ok(id)
}

/// The data directory's part of startup, for the app and the headless agent alike:
/// create `dir` owner-only (D-074, it holds usage history), open the history store in it,
/// and read or create the host id ([`load_or_create`], with the store's local host as the
/// fallback). Never fails: a directory that cannot be created or written (root-owned
/// after a `sudo` run, a full disk) leaves history unavailable with the reason and the
/// host id in memory, so the caller runs live-only.
pub fn open_local(
    dir: &Path,
    binding: impl Fn(HostId) -> Option<String>,
) -> (HistoryStore, LocalId) {
    if let Err(e) = kelvo_store::create_private_dir(dir) {
        tracing::error!(dir = %dir.display(), "cannot create the data directory, running live-only: {e}");
    }
    let history = HistoryStore::open(dir);
    let stored_local = if history.is_available() {
        history.read(|r| r.local_host()).unwrap_or_else(|e| {
            tracing::warn!("reading the stored local host: {e}");
            None
        })
    } else {
        None
    };
    let id = load_or_create(dir, stored_local.as_ref(), binding);
    (history, id)
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

/// A token tying `id` to this machine: SHA-256 of a fixed prefix, the id and the
/// hardware UUID (`kelvo_collect::macos::platform_uuid`), in hex. Salting with the id
/// means the file never holds the hardware UUID or anything that matches across
/// installs. `None` when the hardware UUID is unreadable. On Linux it is
/// [`linux_binding`] of `/` without firmware ids; other systems have none, and their
/// file is trusted as it is.
pub fn machine_binding(id: HostId) -> Option<String> {
    binding::machine_binding(id)
}

/// Which machine identifiers a Linux binding covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingScope {
    /// `/etc/machine-id` only: what any user can read (the user agent, the default).
    MachineId,
    /// The machine-id plus the DMI `product_uuid` or device-tree serial when readable,
    /// usually only as root (the system agent's `--system`).
    WithHardware,
}

const BINDING_PREFIX: &[u8] = b"kelvo host-id binding v1\0";
const HARDWARE_PREFIX: &[u8] = b"kelvo host-id hardware v1\0";

/// SHA-256 of `parts` in order, in hex.
fn sha256_hex(parts: &[&[u8]]) -> String {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// The Linux binding for `id` from the identifiers under `root` (`/` on a real host;
/// [`machine::machine_ids`] reads and validates them), as separate components the
/// `host-id` file stores on their own lines:
///
/// - `machine=v1:<hex>`: SHA-256 of the v1 prefix, the id's 16 bytes and the machine-id.
/// - `hw=v1:<hex>`, when `scope` includes a firmware id and it is readable: SHA-256 of
///   the hardware prefix, the id's 16 bytes and the firmware id.
/// - `legacy=<hex>`, compared but never written: the single tokens this branch's first
///   format wrote, the machine-id hash above and, with a firmware id, the hash of the
///   machine-id, a NUL and the firmware id. Never released, but QA hosts have them.
///
/// `None` without a usable machine-id. Portable code, so it is tested everywhere; only a
/// Linux build uses it for its own binding.
pub fn linux_binding(id: HostId, root: &Path, scope: BindingScope) -> Option<String> {
    let ids = machine::machine_ids(root, scope == BindingScope::WithHardware)?;
    let (id, machine_id) = (id.0.as_bytes().as_slice(), ids.machine_id.as_bytes());
    let machine = sha256_hex(&[BINDING_PREFIX, id, machine_id]);
    let mut text = format!("{MACHINE_TAG}=v1:{machine} {LEGACY_TAG}={machine}");
    if let Some(hw) = &ids.hardware {
        let hardware = sha256_hex(&[HARDWARE_PREFIX, id, hw.as_bytes()]);
        let combined = sha256_hex(&[BINDING_PREFIX, id, machine_id, b"\0", hw.as_bytes()]);
        text.push_str(&format!(" hw=v1:{hardware} {LEGACY_TAG}={combined}"));
    }
    Some(text)
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
        let mut input = super::BINDING_PREFIX.to_vec();
        input.extend_from_slice(id.0.as_bytes());
        input.extend_from_slice(platform.as_bytes());
        let len = u32::try_from(input.len()).ok()?;
        let mut md = [0u8; 32];
        // SAFETY: `input` has `len` readable bytes and `md` the 32 bytes SHA-256 writes.
        unsafe { CC_SHA256(input.as_ptr().cast(), len, md.as_mut_ptr()) };
        Some(md.iter().map(|b| format!("{b:02x}")).collect())
    }
}

#[cfg(target_os = "linux")]
mod binding {
    use kelvo_schema::HostId;

    pub(super) fn machine_binding(id: HostId) -> Option<String> {
        super::linux_binding(
            id,
            std::path::Path::new("/"),
            super::BindingScope::MachineId,
        )
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
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
                kernel: None,
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

    /// `open_local` makes the directory, opens history in it and, with the file lost,
    /// recovers the id from the store it opened.
    #[test]
    fn open_local_recovers_the_id_from_its_store() {
        let tmp = temp_dir("id-open-local");
        let dir = tmp.path().join("data");
        let (history, first) = open_local(&dir, mac_a);
        assert!(history.is_available());
        assert_eq!(first.source, IdSource::Created);
        history.register_host(&record(first.id, true)).unwrap();
        history.close();
        drop(history);
        std::fs::remove_file(dir.join(HOST_ID_FILE)).unwrap();

        let (history, again) = open_local(&dir, mac_a);
        assert!(history.is_available());
        assert_eq!((again.id, again.source), (first.id, IdSource::Store));
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

    /// D-116: `reset` mints a new id that the next start reads from the file, even
    /// though the store still names the old one as its local host.
    #[test]
    fn reset_mints_an_id_the_next_start_keeps() {
        let tmp = temp_dir("id-reset");
        let dir = tmp.path();
        let old = load_or_create(dir, None, mac_a).id;
        let new = reset(dir, mac_a).unwrap();
        assert_ne!(new, old);
        let text = std::fs::read_to_string(dir.join(HOST_ID_FILE)).unwrap();
        assert_eq!(text, format!("{new}\nmachine=a-{new}\n"));
        assert_eq!(load(dir, Some(&record(old, true))), (new, IdSource::File));
        // An unwritable directory is an error, not a silent in-memory id.
        std::fs::write(dir.join("file"), "").unwrap();
        assert!(reset(&dir.join("file").join("data"), mac_a).is_err());
    }

    fn linux_root(files: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = temp_dir("linux-root");
        for (path, text) in files {
            let path = tmp.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        tmp
    }

    const MACHINE_ID: &str = "4c1d3f0e9a8b47c6b5d2e1f0a9b8c7d6";

    /// The Linux binding is a one-way door too. Expected values, computed independently
    /// (Python `hashlib`): the machine component is SHA-256 of the binding prefix, the
    /// id's 16 bytes and the machine-id; the hardware one of the hardware prefix, the id
    /// and the DMI UUID; the legacy combined token of the binding prefix, the id, the
    /// machine-id, a NUL and the DMI UUID.
    #[test]
    fn the_linux_binding_hash_is_stable() {
        let id = HostId(Uuid::parse_str("6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b").unwrap());
        let root = linux_root(&[
            ("etc/machine-id", &format!("{MACHINE_ID}\n")),
            ("sys/class/dmi/id/product_uuid", DMI),
        ]);
        let user = linux_binding(id, root.path(), BindingScope::MachineId);
        assert_eq!(
            user.as_deref(),
            Some(format!("machine=v1:{MACHINE_TOKEN} legacy={MACHINE_TOKEN}").as_str())
        );
        let system = linux_binding(id, root.path(), BindingScope::WithHardware);
        assert_eq!(
            system.as_deref(),
            Some(
                format!(
                    "machine=v1:{MACHINE_TOKEN} legacy={MACHINE_TOKEN} \
                     hw=v1:{HW_TOKEN} legacy={LEGACY_SYSTEM_TOKEN}"
                )
                .as_str()
            )
        );

        // The file holds the tagged components, never the legacy tokens.
        let dir = temp_dir("id-linux-format");
        let on_root = |id| linux_binding(id, root.path(), BindingScope::WithHardware);
        std::fs::write(dir.path().join(HOST_ID_FILE), format!("{id}\n")).unwrap();
        assert_eq!(load_or_create(dir.path(), None, on_root).id, id);
        assert_eq!(
            std::fs::read_to_string(dir.path().join(HOST_ID_FILE)).unwrap(),
            format!("{id}\nmachine=v1:{MACHINE_TOKEN}\nhw=v1:{HW_TOKEN}\n")
        );

        // Without a readable firmware id, the system binding is the machine-id one.
        let bare = linux_root(&[("etc/machine-id", MACHINE_ID)]);
        assert_eq!(
            linux_binding(id, bare.path(), BindingScope::WithHardware),
            user
        );
        // Another id on the same machine gets another token.
        assert_ne!(
            linux_binding(HostId(Uuid::new_v4()), bare.path(), BindingScope::MachineId),
            user
        );
    }

    const DMI: &str = "6E0A1B2C-3D4E-5F60-7182-93A4B5C6D7E8";
    const MACHINE_TOKEN: &str = "c4f4c4e74777ebb30f9f2adf4dbfe31acb49c69dba3d9619bfe3062b3f3c10cf";
    const HW_TOKEN: &str = "0b6ee7ab3493313d669c1634ff69f90bf0fc34d69daa6beae45ff717d94250be";
    /// What this branch's first format wrote for a system agent with DMI readable.
    const LEGACY_SYSTEM_TOKEN: &str =
        "841c40a6d4fb0f0ad4e352d684fcc94f5ba7b683dfa7da3aecc416a510b28bca";

    /// A system agent's binding over a root with `machine_id` and, when given, `dmi`.
    fn system_binding(
        machine_id: &str,
        dmi: Option<&str>,
    ) -> (tempfile::TempDir, impl Fn(HostId) -> Option<String>) {
        let mut files = vec![("etc/machine-id", machine_id)];
        if let Some(dmi) = dmi {
            files.push(("sys/class/dmi/id/product_uuid", dmi));
        }
        let root = linux_root(&files);
        let path = root.path().to_owned();
        (root, move |id| {
            linux_binding(id, &path, BindingScope::WithHardware)
        })
    }

    /// D-116: firmware that becomes unreadable or readable (a kernel, firmware or
    /// permission change) keeps the id, because the `hw=` line is compared only when
    /// both sides have it. The line stays in the file while unreadable.
    #[test]
    fn a_firmware_id_appearing_or_vanishing_keeps_the_linux_id() {
        let (_with, with_dmi) = system_binding(MACHINE_ID, Some(DMI));
        let (_without, without_dmi) = system_binding(MACHINE_ID, None);

        let dir = temp_dir("id-linux-dmi-vanishes");
        let first = load_or_create(dir.path(), None, &with_dmi).id;
        let later = load_or_create(dir.path(), None, &without_dmi);
        assert_eq!((later.id, later.source), (first, IdSource::File));
        let text = std::fs::read_to_string(dir.path().join(HOST_ID_FILE)).unwrap();
        assert!(text.contains("\nhw=v1:"), "{text}");
        let back = load_or_create(dir.path(), None, &with_dmi);
        assert_eq!((back.id, back.source), (first, IdSource::File));

        let dir = temp_dir("id-linux-dmi-appears");
        let first = load_or_create(dir.path(), None, &without_dmi).id;
        let later = load_or_create(dir.path(), None, &with_dmi);
        assert_eq!((later.id, later.source), (first, IdSource::File));
        let text = std::fs::read_to_string(dir.path().join(HOST_ID_FILE)).unwrap();
        assert!(text.contains("\nhw=v1:"), "the hw line is added: {text}");
    }

    /// A component both sides have that differs is a clone: another machine-id, or the
    /// same machine-id with another DMI UUID (a hypervisor clone).
    #[test]
    fn a_differing_linux_component_is_a_clone() {
        let (_a, a) = system_binding(MACHINE_ID, Some(DMI));
        let (_b, other_machine) = system_binding("0123456789abcdef0123456789abcdef", Some(DMI));
        let (_c, other_dmi) =
            system_binding(MACHINE_ID, Some("99999999-2222-3333-4444-555555555555"));
        let (_d, other_machine_no_dmi) = system_binding("0123456789abcdef0123456789abcdef", None);

        for (name, b) in [
            (
                "machine-id",
                &other_machine as &dyn Fn(HostId) -> Option<String>,
            ),
            ("dmi", &other_dmi),
            ("machine-id without dmi", &other_machine_no_dmi),
        ] {
            let dir = temp_dir("id-linux-clone");
            let first = load_or_create(dir.path(), None, &a).id;
            let clone = load_or_create(dir.path(), None, b);
            assert_eq!(clone.source, IdSource::Cloned, "{name}");
            assert_ne!(clone.id, first, "{name}");
            // The new id's file holds only this machine's lines.
            let again = load_or_create(dir.path(), None, b);
            assert_eq!(
                (again.id, again.source),
                (clone.id, IdSource::File),
                "{name}"
            );
        }
    }

    /// Files from this branch's first format (one `machine=` token, the machine-id hash
    /// or, for a system agent with DMI, the combined hash) load with their id and are
    /// rewritten in the tagged format. One from another machine is still a clone.
    #[test]
    fn a_legacy_linux_file_loads() {
        let id = HostId(Uuid::parse_str("6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b").unwrap());
        let (_a, with_dmi) = system_binding(MACHINE_ID, Some(DMI));
        let (_b, without_dmi) = system_binding(MACHINE_ID, None);
        for (token, binding) in [
            (
                MACHINE_TOKEN,
                &with_dmi as &dyn Fn(HostId) -> Option<String>,
            ),
            (MACHINE_TOKEN, &without_dmi),
            (LEGACY_SYSTEM_TOKEN, &with_dmi),
        ] {
            let dir = temp_dir("id-linux-legacy");
            let path = dir.path().join(HOST_ID_FILE);
            std::fs::write(&path, format!("{id}\nmachine={token}\n")).unwrap();
            let l = load_or_create(dir.path(), None, binding);
            assert_eq!((l.id, l.source), (id, IdSource::File), "{token}");
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(
                text.starts_with(&format!("{id}\nmachine=v1:{MACHINE_TOKEN}\n")),
                "{text}"
            );
        }

        let dir = temp_dir("id-linux-legacy-clone");
        let path = dir.path().join(HOST_ID_FILE);
        std::fs::write(&path, format!("{id}\nmachine={}\n", "ab".repeat(32))).unwrap();
        let l = load_or_create(dir.path(), None, &with_dmi);
        assert_eq!(l.source, IdSource::Cloned);
    }

    /// D-116: a VM cloned with its machine-id gets a new id when the system agent's
    /// binding sees the new DMI UUID. A user agent sees only the machine-id, so the copy
    /// keeps its id; without a usable machine-id there is no binding and the file wins.
    #[test]
    fn a_linux_vm_clone_is_caught_only_with_hardware_ids() {
        let dmi = "sys/class/dmi/id/product_uuid";
        let a = linux_root(&[
            ("etc/machine-id", MACHINE_ID),
            (dmi, "11111111-2222-3333-4444-555555555555"),
        ]);
        let b = linux_root(&[
            ("etc/machine-id", MACHINE_ID),
            (dmi, "99999999-2222-3333-4444-555555555555"),
        ]);
        let on = |root: &tempfile::TempDir, scope| {
            let root = root.path().to_owned();
            move |id| linux_binding(id, &root, scope)
        };

        let system = temp_dir("id-linux-system");
        let first = load_or_create(system.path(), None, on(&a, BindingScope::WithHardware)).id;
        let clone = load_or_create(system.path(), None, on(&b, BindingScope::WithHardware));
        assert_eq!(clone.source, IdSource::Cloned);
        assert_ne!(clone.id, first);

        let user = temp_dir("id-linux-user");
        let first = load_or_create(user.path(), None, on(&a, BindingScope::MachineId)).id;
        let copy = load_or_create(user.path(), None, on(&b, BindingScope::MachineId));
        assert_eq!((copy.id, copy.source), (first, IdSource::File));

        let none = linux_root(&[("etc/machine-id", "uninitialized\n")]);
        let trusted = load_or_create(user.path(), None, on(&none, BindingScope::WithHardware));
        assert_eq!((trusted.id, trusted.source), (first, IdSource::File));
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
