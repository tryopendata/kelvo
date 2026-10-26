use super::*;
use kelvo_store::rusqlite::Connection;

fn installation(dir: &Path) -> AgentPaths {
    let paths = AgentPaths {
        binary: dir.join("bin/kelvo-agent"),
        data_dir: dir.join("data"),
        socket: dir.join("run/metrics.sock"),
        unit: dir.join("units/kelvo-agent.service"),
    };
    fs::create_dir_all(paths.binary.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.data_dir).unwrap();
    fs::write(&paths.binary, "old binary").unwrap();
    let db = Connection::open(paths.data_dir.join("history.sqlite")).unwrap();
    db.execute_batch("CREATE TABLE cursors (seq INTEGER); INSERT INTO cursors VALUES (10); PRAGMA user_version=1;").unwrap();
    db.close().unwrap();
    paths
}

#[test]
fn failed_trial_cannot_serve_and_restores_the_whole_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let paths = installation(dir.path());
    let mut journal = begin(&paths, "2.0.0").unwrap();
    assert!(public_allowed(&paths).is_err());
    prepare(&paths, &mut journal).unwrap();
    fs::write(&paths.binary, "candidate").unwrap();
    let db = Connection::open(paths.data_dir.join("history.sqlite")).unwrap();
    db.execute_batch("ALTER TABLE cursors ADD COLUMN migrated INTEGER; UPDATE cursors SET seq=20; PRAGMA user_version=2;").unwrap();
    db.close().unwrap();
    fs::write(paths.data_dir.join("history.sqlite-wal"), "new sequences").unwrap();
    assert!(public_allowed(&paths).is_err());
    recover(&paths).unwrap();
    assert_eq!(fs::read_to_string(&paths.binary).unwrap(), "old binary");
    assert_database(&paths, 10, 1);
    assert!(!paths.data_dir.join("history.sqlite-wal").exists());
    public_allowed(&paths).unwrap();
    assert!(recover(&paths).unwrap().is_none());
}

#[test]
fn crash_after_commit_before_public_start_never_rolls_back_history() {
    let dir = tempfile::tempdir().unwrap();
    let paths = installation(dir.path());
    let mut journal = begin(&paths, "2.0.0").unwrap();
    prepare(&paths, &mut journal).unwrap();
    let db = Connection::open(paths.data_dir.join("history.sqlite")).unwrap();
    db.execute_batch("UPDATE cursors SET seq=20; PRAGMA user_version=2;")
        .unwrap();
    db.close().unwrap();
    fs::write(&paths.binary, "candidate").unwrap();
    commit(&paths, &mut journal).unwrap();
    public_allowed(&paths).unwrap();
    recover(&paths).unwrap();
    assert_database(&paths, 20, 2);
    assert_eq!(fs::read_to_string(&paths.binary).unwrap(), "candidate");
}

#[test]
fn incomplete_backup_does_not_overwrite_original_history() {
    let dir = tempfile::tempdir().unwrap();
    let paths = installation(dir.path());
    begin(&paths, "2.0.0").unwrap();
    fs::create_dir_all(directory(&paths).join("backup")).unwrap();
    fs::write(directory(&paths).join("backup/history.sqlite"), "truncated").unwrap();
    recover(&paths).unwrap();
    assert_database(&paths, 10, 1);
}

fn assert_database(paths: &AgentPaths, seq: i64, schema: i64) {
    let db = Connection::open(paths.data_dir.join("history.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT seq FROM cursors", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        seq
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        schema
    );
}

#[test]
fn recovery_refuses_to_restore_while_a_trial_writer_survives() {
    let dir = tempfile::tempdir().unwrap();
    let paths = installation(dir.path());
    let mut journal = begin(&paths, "2.0.0").unwrap();
    prepare(&paths, &mut journal).unwrap();
    let writer = history_lock(&paths).unwrap();
    assert!(
        recover(&paths)
            .unwrap_err()
            .to_string()
            .contains("collector still owns history")
    );
    assert!(read(&paths).unwrap().is_some());
    drop(writer);
    recover(&paths).unwrap();
    assert_database(&paths, 10, 1);
}

#[test]
fn rollback_restores_units_and_committed_recovery_refreshes_the_preserved_executable() {
    let dir = tempfile::tempdir().unwrap();
    let paths = installation(dir.path());
    fs::create_dir_all(paths.unit.parent().unwrap()).unwrap();
    fs::write(&paths.unit, "old sandbox").unwrap();
    let mut journal = begin(&paths, "2.0.0").unwrap();
    prepare(&paths, &mut journal).unwrap();
    fs::write(&paths.unit, "candidate sandbox").unwrap();
    fs::write(&paths.binary, "candidate binary").unwrap();
    recover(&paths).unwrap();
    assert_eq!(fs::read_to_string(&paths.unit).unwrap(), "old sandbox");
    assert_eq!(
        fs::read_to_string(recovery_binary(&paths)).unwrap(),
        "old binary"
    );
    let mut journal = begin(&paths, "2.0.1").unwrap();
    prepare(&paths, &mut journal).unwrap();
    fs::write(&paths.unit, "committed sandbox").unwrap();
    fs::write(&paths.binary, "committed binary").unwrap();
    commit(&paths, &mut journal).unwrap();
    recover(&paths).unwrap();
    assert_eq!(
        fs::read_to_string(&paths.unit).unwrap(),
        "committed sandbox"
    );
    assert_eq!(
        fs::read_to_string(recovery_binary(&paths)).unwrap(),
        "committed binary"
    );
}

#[test]
fn death_after_cleanup_before_start_preserves_restart_intent() {
    let dir = tempfile::tempdir().unwrap();
    let paths = installation(dir.path());
    let mut journal = begin(&paths, "2.0.0").unwrap();
    prepare(&paths, &mut journal).unwrap();
    commit(&paths, &mut journal).unwrap();
    recover(&paths).unwrap();
    assert!(read(&paths).unwrap().is_none());
    assert!(directory(&paths).join("restart-required").exists());
    // A second recovery cannot lose the pending systemd start.
    recover(&paths).unwrap();
    assert!(directory(&paths).join("restart-required").exists());
    public_starting(&paths).unwrap();
    assert!(!directory(&paths).join("restart-required").exists());
}
