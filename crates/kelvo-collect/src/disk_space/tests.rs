use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use kelvo_schema::disk_space::{SpaceKind, SpacePhase, SpaceTarget, SpaceVolume};

use super::*;

fn volume() -> SpaceVolume {
    SpaceVolume {
        mount: "/".into(),
        name: "Test HD".into(),
        is_boot: true,
        used_bytes: 0.0,
        total_bytes: 0.0,
        protected: Vec::new(),
    }
}

fn plan(root: &Path, used: u64) -> ScanPlan {
    ScanPlan {
        volume: volume(),
        root: root.to_path_buf(),
        skip: HashSet::new(),
        skipped: Vec::new(),
        siblings: vec![Sibling {
            label: "macOS system".into(),
            bytes: 1_000,
        }],
        used_bytes: used,
        firmlinked: Vec::new(),
        threads: 3,
    }
}

fn run(plan: ScanPlan) -> Arc<Scan> {
    let (tx, rx) = mpsc::channel();
    let scan = Scan::start(
        plan,
        || StdLister,
        move || {
            let _ = tx.send(());
        },
    )
    .unwrap();
    rx.recv_timeout(Duration::from_secs(30)).unwrap();
    scan
}

fn write(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, vec![7u8; bytes]).unwrap();
}

fn alloc(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path).unwrap().blocks() * 512
}

/// A tree with a dominant folder, a hard link, a symlink loop and nested folders.
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("vol");
    write(&root.join("Users/me/big.bin"), 4 << 20);
    write(&root.join("Users/me/code/a.rs"), 64 << 10);
    write(&root.join("Library/cache.db"), 1 << 20);
    fs::hard_link(root.join("Library/cache.db"), root.join("Users/me/link.db")).unwrap();
    std::os::unix::fs::symlink(&root, root.join("Users/loop")).unwrap();
    (dir, root)
}

#[test]
fn walks_the_tree_and_matches_allocated_sizes() {
    let (_dir, root) = fixture();
    let expect = alloc(&root.join("Users/me/big.bin"))
        + alloc(&root.join("Users/me/code/a.rs"))
        + alloc(&root.join("Library/cache.db"))
        + alloc(&root.join("Users/loop"));
    let scan = run(plan(&root, 100 << 20));
    assert_eq!(scan.phase(), SpacePhase::Done);
    let p = scan.progress();
    assert_eq!(
        p.items, 4,
        "the hard link counts once; the symlink is one entry"
    );
    assert_eq!(p.measured, expect);
    let top = scan.subtree(None).unwrap();
    assert_eq!(top.unmeasured_bytes, None);
    assert_eq!(top.total_bytes, (100 << 20) as f64);
    let names: Vec<_> = top
        .children
        .iter()
        .map(|c| (c.name.as_str(), c.kind))
        .collect();
    assert_eq!(
        names[0],
        (
            "Other: snapshots, APFS metadata, unreadable and skipped folders",
            SpaceKind::Other
        )
    );
    assert!(names.contains(&("Users", SpaceKind::Folder)));
    assert!(names.contains(&("macOS system", SpaceKind::Volume)));
    let sum: f64 = top.children.iter().map(|c| c.bytes).sum();
    assert_eq!(
        sum, top.total_bytes,
        "files, volumes and other add up to used"
    );
}

#[test]
fn drills_into_a_folder() {
    let (_dir, root) = fixture();
    let scan = run(plan(&root, 100 << 20));
    let users = scan.subtree(None).unwrap();
    let id = users
        .children
        .iter()
        .find(|c| c.name == "Users")
        .unwrap()
        .id;
    let users = scan.subtree(id).unwrap();
    let me = users.children.iter().find(|c| c.name == "me").unwrap();
    assert!(me.openable && me.done);
    let me = scan.subtree(me.id).unwrap();
    assert_eq!(
        me.path.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["Test HD", "Users", "me"]
    );
    assert_eq!(me.children[0].name, "big.bin");
    assert_eq!(me.children[0].kind, SpaceKind::File);
    assert_eq!(me.total_bytes, me.folder.bytes);
}

#[test]
fn files_exceeding_used_space_leave_no_negative_remainder() {
    let (_dir, root) = fixture();
    let scan = run(plan(&root, 10));
    let report = scan.report(1, 0.0).unwrap();
    assert_eq!(report.other_bytes, None);
    assert!(
        scan.subtree(None)
            .unwrap()
            .children
            .iter()
            .all(|c| c.kind != SpaceKind::Other)
    );
}

#[test]
fn skipped_and_unreadable_folders_are_reported() {
    let (_dir, root) = fixture();
    write(&root.join("Users/me/Desktop/photo.raw"), 2 << 20);
    let locked = root.join("Locked");
    write(&locked.join("secret"), 1 << 20);
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let mut p = plan(&root, 100 << 20);
    p.skip.insert(PathBuf::from("Users/me/Desktop"));
    p.skipped.push("Desktop".into());
    let scan = run(p);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    let report = scan.report(1, 0.0).unwrap();
    assert_eq!(report.skipped, ["Desktop"]);
    let root_user = unsafe_is_root();
    if !root_user {
        assert_eq!(report.unreadable, 1);
        assert_eq!(report.unreadable_paths, ["Locked"]);
    }
    let files: Vec<_> = scan.largest_files().into_iter().map(|f| f.name).collect();
    assert!(
        !files.contains(&"photo.raw".to_string()),
        "skipped folders are not entered"
    );
}

fn unsafe_is_root() -> bool {
    std::env::var("USER").is_ok_and(|u| u == "root")
}

#[test]
fn largest_files_and_reveal_paths() {
    let (_dir, root) = fixture();
    let mut p = plan(&root, 100 << 20);
    p.firmlinked.push("Users".into());
    let scan = run(p);
    let files = scan.largest_files();
    assert_eq!(files[0].name, "big.bin");
    assert_eq!(files[0].parent, "Users/me");
    assert_eq!(
        scan.reveal_path(SpaceTarget::LargestFile { index: 0 }),
        Some(PathBuf::from("/Users/me/big.bin")),
        "firmlinked top-level folders reveal from /"
    );
    let lib = scan
        .subtree(None)
        .unwrap()
        .children
        .into_iter()
        .find(|c| c.name == "Library")
        .unwrap();
    assert_eq!(
        scan.reveal_path(SpaceTarget::Node {
            id: lib.id.unwrap()
        }),
        Some(root.join("Library"))
    );
}

#[test]
fn cancel_stops_the_walk() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..200 {
        write(&dir.path().join(format!("d{i}/e/f/g.txt")), 10);
    }
    let (tx, rx) = mpsc::channel();
    let scan = Scan::start(
        plan(dir.path(), 1 << 30),
        || StdLister,
        move || {
            let _ = tx.send(());
        },
    )
    .unwrap();
    scan.cancel();
    rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(scan.phase(), SpacePhase::Cancelled);
    assert!(scan.report(1, 0.0).is_none());
}

#[test]
fn a_running_root_is_drawn_against_used_space() {
    let (_dir, root) = fixture();
    let scan = run(plan(&root, 100 << 20));
    // A finished scan has no unmeasured region; a fresh tree does.
    let fresh = Scan::start(plan(&root, 100 << 20), || Blocked, || {}).unwrap();
    let top = fresh.subtree(None).unwrap();
    assert_eq!(top.unmeasured_bytes, Some((100 << 20) as f64 - 1_000.0));
    fresh.cancel();
    assert!(scan.subtree(None).unwrap().unmeasured_bytes.is_none());
}

/// A lister that waits until cancelled, so a scan stays running.
struct Blocked;
impl Lister for Blocked {
    fn list(&mut self, _: &Path, _: u64, _: &mut Listing) -> Result<(), ListError> {
        std::thread::sleep(Duration::from_millis(50));
        Err(ListError::Unreadable)
    }
}
