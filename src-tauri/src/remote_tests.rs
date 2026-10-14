//! Shell configuration and cancellation behavior.
use super::*;
#[test]
fn invalid_host_configuration_is_rejected_before_ssh_or_storage() {
    let valid = RemoteHostConfig {
        id: String::new(),
        name: "Other Mac".into(),
        lan: Some("build-mac".into()),
        tailscale: None,
        enabled: true,
        mirror_budget_mb: 100,
    };
    assert!(validate(&valid).is_ok());
    for alias in ["-oProxyCommand=bad", "mac --verbose", "mac\nwhoami"] {
        let mut c = valid.clone();
        c.lan = Some(alias.into());
        assert!(validate(&c).is_err());
    }
    let mut c = valid.clone();
    c.mirror_budget_mb = 0;
    assert!(validate(&c).is_err());
    c = valid;
    c.lan = None;
    assert!(validate(&c).is_err());
}

#[tokio::test]
async fn cancellation_drops_inflight_setup_before_it_can_register_a_host() {
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Cleanup(Arc<AtomicBool>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let (tx, mut rx) = tokio::sync::watch::channel(false);
    let (started, ready) = tokio::sync::oneshot::channel();
    let observed = dropped.clone();
    let task = tokio::spawn(async move {
        setup_step(&mut rx, async move {
            let _cleanup = Cleanup(observed);
            let _ = started.send(());
            std::future::pending::<()>().await;
            Ok::<(), CommandError>(())
        })
        .await
    });
    ready.await.unwrap();
    tx.send(true).unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("setup cancellation must settle promptly")
            .unwrap()
            .is_err()
    );
    assert!(dropped.load(Ordering::SeqCst));
}
