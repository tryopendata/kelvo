//! On-demand Docker storage and explicitly confirmed, session-only operations.
//! No task in this module is driven by the monitoring clock or a window lifetime.
use super::DockerHandle;
use kelvo_collect::{docker::now_ms, docker_storage::DockerStorageAdapter};
use kelvo_schema::{JsSafeInt, docker::*, docker_storage::*, lock::LockExt};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use tokio::time::Instant;

const SCAN_TTL: Duration = Duration::from_secs(300);
const REVIEW_TTL: Duration = Duration::from_secs(60);
const RESULT_TTL: Duration = Duration::from_secs(3600);
const MAX_ITEMS: usize = 128;
const MAX_RESULTS: usize = 20;

#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct DockerStorageView {
    pub snapshot: Option<DockerStorageSnapshot>,
    pub error: Option<String>,
    /// Recover the latest session result when a window is reopened.
    pub latest_operation_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct DockerStorageReview {
    pub token: String,
    pub engine_id: String,
    pub generation: u32,
    #[specta(type = JsSafeInt)]
    pub expires_ms: i64,
    pub resources: Vec<DockerStorageResource>,
    pub selections: Vec<DockerStorageSelection>,
    /// Unknown when selected resources may share physical layers.
    pub estimated_reclaimed_bytes: Option<f64>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct DockerStorageOperation {
    pub id: String,
    pub engine_id: String,
    pub generation: u32,
    #[specta(type = JsSafeInt)]
    pub started_ms: i64,
    #[specta(type = Option<JsSafeInt>)]
    pub finished_ms: Option<i64>,
    pub results: Vec<DockerStorageItemResult>,
    pub total: u32,
}
#[derive(Debug, thiserror::Error)]
pub enum DockerStorageError {
    #[error("{0}")]
    Unavailable(String),
    #[error("Select between one and 128 distinct Docker resources")]
    Selection,
    #[error("The cleanup review expired or was already used; prepare a new review")]
    Review,
    #[error("Confirm permanent volume deletion separately before continuing")]
    VolumeConfirmation,
    #[error("A Docker operation is already running")]
    Busy,
    #[error("This operation is no longer available in session history")]
    Operation,
}

struct Prepared {
    review: DockerStorageReview,
    engine: DockerEngine,
    deadline: Instant,
}
struct RetainedOperation {
    operation: DockerStorageOperation,
    completed: Option<Instant>,
}
#[derive(Default)]
struct Registry {
    view: Option<DockerStorageView>,
    scanned: Option<Instant>,
    attempt: u64,
    reviews: VecDeque<Prepared>,
    operations: VecDeque<RetainedOperation>,
}
impl Registry {
    fn latest_operation(&self, engine_id: Option<&str>) -> Option<String> {
        self.operations
            .iter()
            .rev()
            .find(|o| Some(o.operation.engine_id.as_str()) == engine_id)
            .map(|o| o.operation.id.clone())
    }
    fn prune(&mut self) {
        let now = Instant::now();
        self.reviews.retain(|r| r.deadline > now);
        self.operations.retain(|o| {
            o.completed
                .is_none_or(|t| now.duration_since(t) < RESULT_TTL)
        });
    }
    fn consume(
        &mut self,
        token: &str,
        confirm_volumes: bool,
    ) -> Result<Prepared, DockerStorageError> {
        self.prune();
        if self.operations.iter().any(|o| o.completed.is_none()) {
            return Err(DockerStorageError::Busy);
        }
        let index = self
            .reviews
            .iter()
            .position(|r| r.review.token == token)
            .ok_or(DockerStorageError::Review)?;
        let review = self.reviews.get(index).ok_or(DockerStorageError::Review)?;
        if !confirm_volumes
            && review.review.selections.iter().any(|s| {
                s.kind == DockerStorageKind::Volume && s.action == DockerStorageAction::Delete
            })
        {
            return Err(DockerStorageError::VolumeConfirmation);
        }
        self.reviews.remove(index).ok_or(DockerStorageError::Review)
    }
}
#[derive(Default)]
pub(super) struct StorageManager {
    registry: Mutex<Registry>,
    scan_gate: tokio::sync::Mutex<()>,
    pub(super) resumed_generation: AtomicU32,
}

impl DockerHandle {
    fn storage_context(&self) -> Result<(DockerEngine, u32), DockerStorageError> {
        let control = self.control.borrow();
        let snapshot = self.changed.borrow();
        if !control.settings.enabled
            || control.paused
            || control.shutdown
            || !(snapshot.status == DockerStatus::Connected
                || (snapshot.status == DockerStatus::IdleSuspended
                    && self.storage.resumed_generation.load(Ordering::SeqCst)
                        == control.generation))
            || snapshot.generation != control.generation
        {
            return Err(DockerStorageError::Unavailable(
                "Connect or resume Docker monitoring before scanning or changing resources".into(),
            ));
        }
        let engine = snapshot.engine.clone().ok_or_else(|| {
            DockerStorageError::Unavailable("Docker engine is unavailable".into())
        })?;
        if control.settings.selected_engine.as_deref() != Some(engine.socket.as_str()) {
            return Err(DockerStorageError::Unavailable(
                "Docker engine selection changed".into(),
            ));
        }
        Ok((engine, control.generation))
    }
    fn storage_current(&self, engine: &DockerEngine, generation: u32) -> bool {
        self.storage_context().is_ok_and(|(e, g)| {
            g == generation
                && e.id == engine.id
                && e.socket == engine.socket
                && e.daemon_id == engine.daemon_id
        })
    }
    /// Cache remains visible after a failed scan, but the error prohibits preparing actions.
    pub async fn storage_scan(&self, force: bool) -> DockerStorageView {
        let attempt = self.storage.registry.lock_ok().attempt;
        let _lane = self.storage.scan_gate.lock().await;
        let (engine, generation) = match self.storage_context() {
            Ok(context) => context,
            Err(error) => return self.storage_scan_error(error.to_string()),
        };
        {
            let mut registry = self.storage.registry.lock_ok();
            registry.prune();
            if let Some(view) = &registry.view
                && view
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.engine_id == engine.id && s.generation == generation)
                && ((!force && registry.scanned.is_some_and(|t| t.elapsed() < SCAN_TTL))
                    || registry.attempt != attempt)
            {
                let mut view = view.clone();
                view.latest_operation_id = registry.latest_operation(Some(&engine.id));
                return view;
            }
        }
        let result = tokio::time::timeout(REVIEW_TTL, async {
            let adapter = DockerStorageAdapter::connect(engine.clone()).await?;
            adapter.scan(generation).await
        })
        .await;
        let _gate = self.gate.lock_ok();
        if !self.storage_current(&engine, generation) {
            return self.storage_scan_error("Docker configuration changed during the scan".into());
        }
        match result {
            Ok(Ok(snapshot)) => {
                let mut registry = self.storage.registry.lock_ok();
                registry.prune();
                let view = DockerStorageView {
                    snapshot: Some(snapshot),
                    error: None,
                    latest_operation_id: registry.latest_operation(Some(&engine.id)),
                };
                registry.attempt = registry.attempt.wrapping_add(1);
                registry.scanned = Some(Instant::now());
                registry.view = Some(view.clone());
                view
            }
            Ok(Err(error)) => self.storage_scan_error(error.to_string()),
            Err(_) => self
                .storage_scan_error("Docker storage scan exceeded its 60-second deadline".into()),
        }
    }
    fn storage_scan_error(&self, error: String) -> DockerStorageView {
        let engine_id = self.snapshot().engine.map(|e| e.id);
        let mut registry = self.storage.registry.lock_ok();
        registry.prune();
        let snapshot = registry.view.as_ref().and_then(|v| v.snapshot.clone());
        let view = DockerStorageView {
            snapshot,
            error: Some(error),
            latest_operation_id: registry.latest_operation(engine_id.as_deref()),
        };
        registry.attempt = registry.attempt.wrapping_add(1);
        registry.view = Some(view.clone());
        // A failed refresh invalidates all outstanding authorizations, not its last measurement.
        registry.reviews.clear();
        view
    }
    pub async fn storage_prepare(
        &self,
        selections: Vec<DockerStorageSelection>,
    ) -> Result<DockerStorageReview, DockerStorageError> {
        if selections.is_empty()
            || selections.len() > MAX_ITEMS
            || selections.iter().enumerate().any(|(i, s)| {
                selections
                    .iter()
                    .take(i)
                    .any(|old| old.kind == s.kind && old.id == s.id)
            })
        {
            return Err(DockerStorageError::Selection);
        }
        let view = self.storage_scan(true).await;
        if let Some(error) = view.error {
            return Err(DockerStorageError::Unavailable(error));
        }
        let snapshot = view.snapshot.filter(|s| s.complete).ok_or_else(|| {
            DockerStorageError::Unavailable(
                "A complete storage scan is required before reviewing changes".into(),
            )
        })?;
        let _gate = self.gate.lock_ok();
        let (engine, generation) = self.storage_context()?;
        if snapshot.engine_id != engine.id || snapshot.generation != generation {
            return Err(DockerStorageError::Review);
        }
        let mut resources = Vec::with_capacity(selections.len());
        for selection in &selections {
            let resource = snapshot
                .resources
                .iter()
                .find(|r| r.kind == selection.kind && r.id == selection.id)
                .ok_or(DockerStorageError::Selection)?;
            if !match selection.action {
                DockerStorageAction::Start => false,
                DockerStorageAction::Delete => resource.can_delete,
                DockerStorageAction::Stop => resource.can_stop,
            } {
                return Err(DockerStorageError::Unavailable(
                    resource.reason.clone().unwrap_or_else(|| {
                        "Selected resource is not eligible for this action".into()
                    }),
                ));
            }
            resources.push(resource.clone());
        }
        let mut warnings = vec!["Shared layers make resource sizes non-additive. Reclaimed Docker storage may not immediately free space on your Mac.".into()];
        if selections
            .iter()
            .any(|s| s.kind == DockerStorageKind::Volume)
        {
            warnings.push("Volume deletion permanently destroys its data. Another Docker client can replace an unused volume between recheck and deletion; Docker cannot atomically protect against this replacement race.".into());
        }
        if selections
            .iter()
            .any(|s| s.action == DockerStorageAction::Stop)
        {
            warnings.push("Stop allows 30 seconds for shutdown, then Docker may terminate the process. AutoRemove containers cannot be stopped here.".into());
        }
        let review = DockerStorageReview {
            token: uuid::Uuid::new_v4().to_string(),
            engine_id: engine.id.clone(),
            generation,
            expires_ms: now_ms().saturating_add(REVIEW_TTL.as_millis() as i64),
            estimated_reclaimed_bytes: selected_estimate(&selections, &resources),
            resources,
            selections,
            warnings,
        };
        let mut registry = self.storage.registry.lock_ok();
        if registry.view.as_ref().is_none_or(|v| v.error.is_some()) {
            return Err(DockerStorageError::Review);
        }
        // A reviewed Stop may leave no running containers. Keep this explicit workflow
        // available when monitoring idles; power/configuration generation changes revoke it.
        self.storage
            .resumed_generation
            .store(generation, Ordering::SeqCst);
        registry.prune();
        while registry.reviews.len() >= MAX_RESULTS {
            registry.reviews.pop_front();
        }
        registry.reviews.push_back(Prepared {
            review: review.clone(),
            engine,
            deadline: Instant::now() + REVIEW_TTL,
        });
        Ok(review)
    }
    pub fn storage_execute(
        &self,
        token: &str,
        confirm_volumes: bool,
    ) -> Result<DockerStorageOperation, DockerStorageError> {
        let _gate = self.gate.lock_ok();
        let mut registry = self.storage.registry.lock_ok();
        let prepared = registry.consume(token, confirm_volumes)?;
        if !self.storage_current(&prepared.engine, prepared.review.generation) {
            return Err(DockerStorageError::Review);
        }
        let operation = DockerStorageOperation {
            id: uuid::Uuid::new_v4().to_string(),
            engine_id: prepared.review.engine_id.clone(),
            generation: prepared.review.generation,
            started_ms: now_ms(),
            finished_ms: None,
            results: Vec::new(),
            total: prepared.review.selections.len() as u32,
        };
        while registry.operations.len() >= MAX_RESULTS {
            registry.operations.pop_front();
        }
        registry.operations.push_back(RetainedOperation {
            operation: operation.clone(),
            completed: None,
        });
        let handle = self.clone();
        let id = operation.id.clone();
        tokio::spawn(async move {
            handle.storage_run(id, prepared).await;
        });
        Ok(operation)
    }
    /// Start/Stop share cleanup's operation registry and never perform a storage scan.
    pub fn container_action(
        &self,
        engine_id: &str,
        container_id: &str,
        action: DockerContainerAction,
    ) -> Result<DockerStorageOperation, DockerStorageError> {
        let _gate = self.gate.lock_ok();
        let mut registry = self.storage.registry.lock_ok();
        registry.prune();
        if registry.operations.iter().any(|o| o.completed.is_none()) {
            return Err(DockerStorageError::Busy);
        }
        let selection = DockerStorageSelection {
            kind: DockerStorageKind::Container,
            id: container_id.into(),
            action: action.into(),
        };
        let context = self
            .storage_context()
            .ok()
            .filter(|(engine, _)| engine.id == engine_id);
        let mut operation = DockerStorageOperation {
            id: uuid::Uuid::new_v4().to_string(),
            engine_id: engine_id.into(),
            generation: self.control.borrow().generation,
            started_ms: now_ms(),
            finished_ms: None,
            results: Vec::new(),
            total: 1,
        };
        if context.is_none() {
            operation.results.push(item_result(
                &selection,
                DockerStorageItemStatus::Skipped,
                "Docker engine changed, paused, or disconnected before dispatch",
            ));
            operation.finished_ms = Some(now_ms());
        }
        while registry.operations.len() >= MAX_RESULTS {
            registry.operations.pop_front();
        }
        registry.operations.push_back(RetainedOperation {
            completed: operation.finished_ms.map(|_| Instant::now()),
            operation: operation.clone(),
        });
        if let Some((engine, generation)) = context {
            let handle = self.clone();
            let id = operation.id.clone();
            tokio::spawn(async move {
                let result = bounded_action(&selection, async {
                    if !handle.storage_current(&engine, generation) {
                        return item_result(&selection, DockerStorageItemStatus::Skipped,
                            "Docker engine changed, paused, or disconnected before dispatch");
                    }
                    match DockerStorageAdapter::connect(engine.clone()).await {
                        Ok(adapter) => adapter.container_action(&selection.id, action,
                            || handle.storage_current(&engine, generation)).await,
                        Err(_) => item_result(&selection, DockerStorageItemStatus::Skipped,
                            "Docker engine identity or connection is unavailable; no action was sent"),
                    }
                }).await;
                let mut registry = handle.storage.registry.lock_ok();
                registry.reviews.clear();
                // The old storage snapshot remains useful but must refresh before reuse.
                registry.scanned = None;
                if let Some(retained) = registry
                    .operations
                    .iter_mut()
                    .find(|o| o.operation.id == id)
                {
                    retained.operation.results.push(result);
                    retained.operation.finished_ms = Some(now_ms());
                    retained.completed = Some(Instant::now());
                }
            });
        }
        Ok(operation)
    }
    pub fn storage_operation(
        &self,
        id: &str,
    ) -> Result<DockerStorageOperation, DockerStorageError> {
        let mut registry = self.storage.registry.lock_ok();
        registry.prune();
        registry
            .operations
            .iter()
            .find(|o| o.operation.id == id)
            .map(|o| o.operation.clone())
            .ok_or(DockerStorageError::Operation)
    }
    async fn storage_run(&self, id: String, prepared: Prepared) {
        let adapter = tokio::time::timeout(
            REVIEW_TTL,
            DockerStorageAdapter::connect(prepared.engine.clone()),
        )
        .await;
        for (selection, reviewed) in prepared
            .review
            .selections
            .iter()
            .zip(&prepared.review.resources)
        {
            let result = if !self.storage_current(&prepared.engine, prepared.review.generation) {
                item_result(
                    selection,
                    DockerStorageItemStatus::Skipped,
                    "Docker engine changed, paused, or disconnected before dispatch",
                )
            } else if let Ok(Ok(adapter)) = &adapter {
                let _lane = self.storage.scan_gate.lock().await;
                bounded_action(
                    selection,
                    adapter.execute(selection, reviewed, || {
                        self.storage_current(&prepared.engine, prepared.review.generation)
                    }),
                )
                .await
            } else {
                item_result(
                    selection,
                    DockerStorageItemStatus::Failed,
                    "Could not establish the Docker action connection; no action was sent",
                )
            };
            let mut registry = self.storage.registry.lock_ok();
            if let Some(operation) = registry
                .operations
                .iter_mut()
                .find(|o| o.operation.id == id)
            {
                operation.operation.results.push(result);
            }
        }
        // Fresh accounting is useful even when a sent request's outcome is unknown.
        // Keep the operation running until refresh finishes so the UI sees final scan data.
        if self.storage_current(&prepared.engine, prepared.review.generation) {
            self.storage_scan(true).await;
        }
        let mut registry = self.storage.registry.lock_ok();
        registry.reviews.clear();
        if let Some(operation) = registry
            .operations
            .iter_mut()
            .find(|o| o.operation.id == id)
        {
            operation.operation.finished_ms = Some(now_ms());
            operation.completed = Some(Instant::now());
        }
    }
}
fn selected_estimate(
    selections: &[DockerStorageSelection],
    resources: &[DockerStorageResource],
) -> Option<f64> {
    if selections.len() != resources.len()
        || selections.is_empty()
        || selections.iter().any(|s| {
            s.action != DockerStorageAction::Delete
                || !matches!(
                    s.kind,
                    DockerStorageKind::Container | DockerStorageKind::Volume
                )
        })
    {
        return None;
    }
    resources.iter().map(|r| r.reclaimable_bytes).sum()
}
fn item_result(
    selection: &DockerStorageSelection,
    status: DockerStorageItemStatus,
    message: &str,
) -> DockerStorageItemResult {
    DockerStorageItemResult {
        selection: selection.clone(),
        status,
        message: message.into(),
        reclaimed_bytes: None,
    }
}

async fn bounded_action(
    selection: &DockerStorageSelection,
    action: impl std::future::Future<Output = DockerStorageItemResult>,
) -> DockerStorageItemResult {
    match tokio::time::timeout(REVIEW_TTL, action).await {
        Ok(result) => result,
        Err(_) => item_result(
            selection,
            DockerStorageItemStatus::OutcomeUnknown,
            "The request deadline elapsed; Docker may have completed the action. It will not be retried.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelvo_schema::HostId;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    fn engine() -> DockerEngine {
        DockerEngine {
            id: "test".into(),
            daemon_id: Some("daemon".into()),
            socket: "/test.sock".into(),
            runtime: "test".into(),
            context: None,
            reachable: true,
            preferred: true,
            error: None,
            api_version: Some("1.49".into()),
            capacity_bytes: None,
            capacity_observed_ms: None,
        }
    }
    fn handle() -> DockerHandle {
        let engine = engine();
        let handle = DockerHandle::new(
            HostId(uuid::Uuid::new_v4()),
            DockerSettings {
                enabled: true,
                selected_engine: Some(engine.socket.clone()),
                ..Default::default()
            },
            PathBuf::new(),
        );
        handle.changed.send_modify(|s| {
            s.engine = Some(engine);
            s.status = DockerStatus::Connected;
        });
        handle
    }
    fn selection(kind: DockerStorageKind) -> DockerStorageSelection {
        DockerStorageSelection {
            kind,
            id: "resource".into(),
            action: DockerStorageAction::Delete,
        }
    }
    fn prepared(kind: DockerStorageKind) -> Prepared {
        Prepared {
            engine: engine(),
            deadline: Instant::now() + REVIEW_TTL,
            review: DockerStorageReview {
                token: "review".into(),
                engine_id: "test".into(),
                generation: 1,
                expires_ms: now_ms() + 60_000,
                resources: Vec::new(),
                selections: vec![selection(kind)],
                estimated_reclaimed_bytes: None,
                warnings: Vec::new(),
            },
        }
    }
    #[tokio::test(start_paused = true)]
    async fn reviews_expire_on_monotonic_clock_and_are_one_use() {
        let mut registry = Registry::default();
        registry
            .reviews
            .push_back(prepared(DockerStorageKind::Image));
        assert!(registry.consume("review", false).is_ok());
        assert!(matches!(
            registry.consume("review", false),
            Err(DockerStorageError::Review)
        ));
        registry
            .reviews
            .push_back(prepared(DockerStorageKind::Image));
        tokio::time::advance(REVIEW_TTL).await;
        assert!(matches!(
            registry.consume("review", false),
            Err(DockerStorageError::Review)
        ));
    }
    #[tokio::test]
    async fn volume_confirmation_is_separate_and_missing_ack_does_not_consume() {
        let mut registry = Registry::default();
        registry
            .reviews
            .push_back(prepared(DockerStorageKind::Volume));
        assert!(matches!(
            registry.consume("review", false),
            Err(DockerStorageError::VolumeConfirmation)
        ));
        assert!(registry.consume("review", true).is_ok());
    }
    #[tokio::test]
    async fn engine_and_power_changes_invalidate_prepared_authorization() {
        for change in 0..3 {
            let handle = handle();
            handle
                .storage
                .registry
                .lock_ok()
                .reviews
                .push_back(prepared(DockerStorageKind::Image));
            match change {
                0 => {
                    handle.configure(DockerSettings::default()).unwrap();
                }
                1 => handle.set_power(true, 1000),
                _ => handle.resume(),
            }
            assert!(matches!(
                handle.storage_execute("review", false),
                Err(DockerStorageError::Review)
            ));
            assert!(handle.storage.registry.lock_ok().operations.is_empty());
        }
    }
    #[tokio::test]
    async fn resume_grants_only_current_generation_access_to_idle_engine() {
        let handle = handle();
        handle
            .changed
            .send_modify(|s| s.status = DockerStatus::IdleSuspended);
        assert!(handle.storage_context().is_err());
        handle.resume();
        let generation = handle.control.borrow().generation;
        handle.changed.send_modify(|s| s.generation = generation);
        assert!(handle.storage_context().is_ok());
        handle.set_power(true, 1000);
        assert!(handle.storage_context().is_err());
        handle.set_power(false, 1000);
        let generation = handle.control.borrow().generation;
        handle.changed.send_modify(|s| s.generation = generation);
        assert!(handle.storage_context().is_err());
    }
    #[tokio::test]
    async fn failed_scan_preserves_timestamp_but_revokes_reviews() {
        let handle = handle();
        let snapshot = DockerStorageSnapshot {
            engine_id: "test".into(),
            generation: 1,
            observed_ms: 123,
            complete: true,
            resources: Vec::new(),
            categories: Vec::new(),
            warnings: Vec::new(),
        };
        {
            let mut registry = handle.storage.registry.lock_ok();
            registry.view = Some(DockerStorageView {
                snapshot: Some(snapshot),
                error: None,
                latest_operation_id: None,
            });
            registry
                .reviews
                .push_back(prepared(DockerStorageKind::Image));
        }
        let view = handle.storage_scan_error("Disconnected".into());
        assert_eq!(view.snapshot.unwrap().observed_ms, 123);
        assert!(view.error.is_some());
        assert!(matches!(
            handle.storage_execute("review", false),
            Err(DockerStorageError::Review)
        ));
    }
    #[tokio::test(start_paused = true)]
    async fn timeout_is_unknown_and_never_retried() {
        let requests = AtomicUsize::new(0);
        let result = bounded_action(&selection(DockerStorageKind::Container), async {
            requests.fetch_add(1, Ordering::SeqCst);
            std::future::pending().await
        })
        .await;
        assert_eq!(result.status, DockerStorageItemStatus::OutcomeUnknown);
        assert_eq!(result.reclaimed_bytes, None);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }
    #[tokio::test(start_paused = true)]
    async fn active_operation_blocks_second_confirmation_and_results_expire() {
        let mut registry = Registry::default();
        registry
            .reviews
            .push_back(prepared(DockerStorageKind::Image));
        registry.operations.push_back(RetainedOperation {
            operation: DockerStorageOperation {
                id: "op".into(),
                engine_id: "test".into(),
                generation: 1,
                started_ms: 1,
                finished_ms: None,
                results: Vec::new(),
                total: 1,
            },
            completed: None,
        });
        assert_eq!(registry.latest_operation(Some("test")), Some("op".into()));
        assert_eq!(registry.latest_operation(Some("other-engine")), None);
        assert!(matches!(
            registry.consume("review", false),
            Err(DockerStorageError::Busy)
        ));
        registry.operations.front_mut().unwrap().completed = Some(Instant::now());
        tokio::time::advance(RESULT_TTL).await;
        registry.prune();
        assert!(registry.operations.is_empty());
        assert_eq!(registry.latest_operation(Some("test")), None);
    }
    #[test]
    fn selected_estimates_only_sum_independent_known_deletions() {
        let row = |kind, bytes| DockerStorageResource {
            kind,
            id: "resource".into(),
            name: "resource".into(),
            project: None,
            size_bytes: bytes,
            reclaimable_bytes: bytes,
            references: Vec::new(),
            age_ms: None,
            age_basis: None,
            database: None,
            mounts: Vec::new(),
            auto_remove: None,
            can_delete: true,
            can_stop: false,
            reason: None,
            fingerprint: "test".into(),
        };
        let mut selections = vec![
            selection(DockerStorageKind::Container),
            selection(DockerStorageKind::Volume),
        ];
        let mut rows = vec![
            row(DockerStorageKind::Container, Some(12.0)),
            row(DockerStorageKind::Volume, Some(30.0)),
        ];
        assert_eq!(selected_estimate(&selections, &rows), Some(42.0));
        selections[0].action = DockerStorageAction::Stop;
        assert_eq!(selected_estimate(&selections, &rows), None);
        selections[0].action = DockerStorageAction::Delete;
        rows[0].reclaimable_bytes = None;
        assert_eq!(selected_estimate(&selections, &rows), None);
        rows[0].reclaimable_bytes = Some(12.0);
        selections[0].kind = DockerStorageKind::Image;
        assert_eq!(selected_estimate(&selections, &rows), None);
        selections[0].kind = DockerStorageKind::BuildCache;
        assert_eq!(selected_estimate(&selections, &rows), None);
    }
    #[tokio::test]
    async fn container_actions_reuse_operation_registry_and_revalidate_before_connection() {
        let handle = handle();
        let stale = handle
            .container_action("old-engine", "abc123", DockerContainerAction::Start)
            .unwrap();
        assert!(stale.finished_ms.is_some());
        assert_eq!(stale.results[0].status, DockerStorageItemStatus::Skipped);
        assert_eq!(
            handle.storage_operation(&stale.id).unwrap().results,
            stale.results
        );
        let operation = handle
            .container_action("test", "abc123", DockerContainerAction::Start)
            .unwrap();
        assert!(operation.finished_ms.is_none());
        assert!(matches!(
            handle.container_action("test", "def456", DockerContainerAction::Stop),
            Err(DockerStorageError::Busy)
        ));
        handle
            .storage
            .registry
            .lock_ok()
            .reviews
            .push_back(prepared(DockerStorageKind::Image));
        assert!(matches!(
            handle.storage_execute("review", false),
            Err(DockerStorageError::Busy)
        ));
        handle.set_power(true, 1000);
        tokio::task::yield_now().await;
        let result = handle.storage_operation(&operation.id).unwrap();
        assert!(result.finished_ms.is_some());
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].status, DockerStorageItemStatus::Skipped);
        assert!(result.results[0].message.contains("before dispatch"));
    }
    #[tokio::test]
    async fn duplicate_and_empty_selections_rejected_before_network() {
        let handle = handle();
        assert!(matches!(
            handle.storage_prepare(Vec::new()).await,
            Err(DockerStorageError::Selection)
        ));
        let s = selection(DockerStorageKind::Image);
        assert!(matches!(
            handle.storage_prepare(vec![s.clone(), s]).await,
            Err(DockerStorageError::Selection)
        ));
    }
    #[tokio::test]
    async fn concurrent_scans_share_cache_and_failed_refresh_cannot_prepare() {
        use std::sync::{Arc, atomic::AtomicBool};
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::UnixListener,
        };
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("scan.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let fail = Arc::new(AtomicBool::new(false));
        let counts = requests.clone();
        let failures = fail.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let requests = counts.clone();
                let fail = failures.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut byte = [0];
                    while !request.ends_with(b"\r\n\r\n") {
                        if stream.read_exact(&mut byte).await.is_err() {
                            return;
                        }
                        request.push(byte[0]);
                    }
                    let request = String::from_utf8_lossy(&request);
                    let path = request.split_whitespace().nth(1).unwrap();
                    let mut status = "200 OK";
                    let body = if path.ends_with("/version") {
                        serde_json::json!({"ApiVersion":"1.49", "Version":"28.1.0"})
                    } else if path.ends_with("/info") {
                        serde_json::json!({"ID":"daemon"})
                    } else if path.ends_with("/system/df") {
                        requests.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        if fail.load(Ordering::SeqCst) {
                            status = "500 Internal Server Error";
                            serde_json::json!({"message":"fixture unavailable"})
                        } else {
                            serde_json::json!({"LayersSize":10,"Containers":[],"Volumes":[],"BuildCache":[],"Images":[{"Id":format!("sha256:{}", "a".repeat(64)),"Size":10,"SharedSize":0,"Containers":0,"ParentId":"","RepoDigests":[],"Created":100,"Labels":{},"RepoTags":["fixture:latest"]}]})
                        }
                    } else {
                        status = "404 Not Found";
                        serde_json::json!({"message":"unavailable"})
                    };
                    let body = body.to_string();
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        let handle = handle();
        handle
            .control
            .send_modify(|c| c.settings.selected_engine = Some(socket.to_string_lossy().into()));
        handle.changed.send_modify(|s| {
            let engine = s.engine.as_mut().unwrap();
            engine.socket = socket.to_string_lossy().into();
            engine.id = "daemon".into();
        });
        let (first, shared) = tokio::join!(handle.storage_scan(true), handle.storage_scan(true));
        assert!(first.error.is_none(), "{:?}", first.error);
        assert!(shared.error.is_none());
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let cached = handle.storage_scan(false).await;
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let selection = DockerStorageSelection {
            kind: DockerStorageKind::Image,
            id: format!("sha256:{}", "a".repeat(64)),
            action: DockerStorageAction::Delete,
        };
        let review = handle
            .storage_prepare(vec![selection.clone()])
            .await
            .unwrap();
        handle
            .changed
            .send_modify(|s| s.status = DockerStatus::IdleSuspended);
        assert!(handle.storage_current(&handle.snapshot().engine.unwrap(), review.generation));
        fail.store(true, Ordering::SeqCst);
        assert!(handle.storage_prepare(vec![selection]).await.is_err());
        let failed = handle.storage.registry.lock_ok().view.clone().unwrap();
        assert!(failed.error.is_some());
        assert!(failed.snapshot.unwrap().observed_ms >= cached.snapshot.unwrap().observed_ms);
        assert!(matches!(
            handle.storage_execute(&review.token, false),
            Err(DockerStorageError::Review)
        ));
        server.abort();
    }
    #[tokio::test]
    #[ignore = "Mutates only explicitly supplied kelvo-storage-test-* disposable fixture resources"]
    async fn installed_storage_operation() {
        let socket = std::env::var("KELVO_STORAGE_TEST_SOCKET").unwrap();
        let fixture_name = std::env::var("KELVO_STORAGE_TEST_CONTAINER").unwrap();
        assert!(fixture_name.starts_with("kelvo-storage-test-"));
        let handle = DockerHandle::new(
            HostId(uuid::Uuid::new_v4()),
            DockerSettings {
                enabled: true,
                selected_engine: Some(socket.clone()),
                ..Default::default()
            },
            PathBuf::from(std::env::var("HOME").unwrap()),
        );
        let adapter = super::super::DockerAdapter::connect(DockerEngine { socket, ..engine() })
            .await
            .unwrap();
        handle.changed.send_modify(|s| {
            s.engine = Some(adapter.engine.clone());
            s.status = DockerStatus::Connected;
        });
        let view = handle.storage_scan(true).await;
        assert!(view.error.is_none(), "{:?}", view.error);
        let scan = view.snapshot.unwrap();
        let row = scan
            .resources
            .iter()
            .find(|r| r.kind == DockerStorageKind::Container && r.name == fixture_name)
            .expect("Disposable fixture must exist by exact name");
        let review = handle
            .storage_prepare(vec![DockerStorageSelection {
                kind: row.kind,
                id: row.id.clone(),
                action: DockerStorageAction::Delete,
            }])
            .await
            .unwrap();
        let operation = handle.storage_execute(&review.token, false).unwrap();
        assert!(handle.storage_execute(&review.token, false).is_err());
        let result = tokio::time::timeout(Duration::from_secs(180), async {
            loop {
                let result = handle.storage_operation(&operation.id).unwrap();
                if result.finished_ms.is_some() {
                    break result;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(
            result.results[0].status,
            DockerStorageItemStatus::Succeeded,
            "{:?}",
            result.results
        );
        eprintln!("Disposable engine operation results: {:?}", result);
    }
}
