//! Read retained identities without connecting to a Docker daemon.
use super::*;

impl DockerRecorder {
    /// Capture retained metadata without contacting a daemon or enabling collection.
    pub fn retained(
        &self,
        q: &api::DockerRetainedRequest,
    ) -> Result<api::DockerRetainedPage, DockerServiceError> {
        if q.limit == 0
            || q.limit > 100
            || q.offset > 100_000
            || q.search.len() > 128
            || q.engine_id.as_ref().is_some_and(|id| id.len() > 256)
        {
            return Err(DockerServiceError::InvalidRequest(
                "Retained catalog search or page bounds exceeded",
            ));
        }
        let (attached, coordination) = self.context();
        let mut reader = attached
            .as_ref()
            .map(|a| Reader::open(&a.path))
            .transpose()?;
        let guard = coordination.lock_ok();
        let snapshot = reader
            .as_mut()
            .map(|r| r.docker_snapshot(self.host, q.engine_id.as_deref().unwrap_or("")))
            .transpose()?;
        let mut state = self.state.lock_ok();
        drain(&mut state);
        let CatalogRead {
            overlay,
            engines,
            pin,
        } = self.catalog_overlay(&mut state, q)?;
        drop(state);
        drop(guard);
        let catalog = if let Some(snapshot) = &snapshot {
            snapshot.catalog_with_overlay(
                q.engine_id.as_deref(),
                &q.search,
                q.offset as usize,
                q.limit as usize,
                &overlay,
                &engines,
            )?
        } else {
            live_catalog(q, overlay, engines)
        };
        let page = retained_page(q, catalog)?;
        if !pin.valid() {
            return Err(DockerServiceError::SnapshotExpired(
                "History was reset or catalog read expired",
            ));
        }
        if serde_json::to_vec(&page)?.len() > db::MAX_QUERY_BYTES {
            return Err(DockerServiceError::ResourceLimit(
                "Retained catalog response byte limit exceeded",
            ));
        }
        Ok(page)
    }
    pub fn detail(&self, engine: &str, id: &str) -> Option<api::DockerContainer> {
        if let Some(c) = self
            .state
            .lock_ok()
            .metadata
            .get(&(engine.into(), id.into()))
            .cloned()
        {
            return Some(c);
        }
        let now = wall_ms();
        self.history(&api::DockerHistoryRequest {
            engine_id: engine.into(),
            container_id: Some(id.into()),
            from_ms: now - db::MAX_RANGE_MS + 1,
            to_ms: now + 1,
            max_points: 1000,
        })
        .ok()?
        .metadata
        .pop()
    }
}

struct CatalogRead<'a> {
    overlay: Vec<db::DockerCatalogEntity>,
    engines: Vec<db::DockerCatalogEngine>,
    pin: ReadPin<'a>,
}

impl DockerRecorder {
    fn catalog_overlay(
        &self,
        state: &mut State,
        q: &api::DockerRetainedRequest,
    ) -> Result<CatalogRead<'_>, DockerServiceError> {
        let held = catalog_bytes(state, q)?;
        state.reader_bytes += held;
        let pin = ReadPin {
            state: &self.state,
            bytes: held,
            generation: state.generation,
            reset_epoch: state.reset_epoch,
            started: Instant::now(),
        };
        let overlay: Vec<_> = state
            .facts
            .iter()
            .filter(|((engine, id), _)| {
                !id.is_empty()
                    && q.engine_id
                        .as_ref()
                        .is_none_or(|selected| selected == engine)
            })
            .map(|((engine, _), fact)| db::DockerCatalogEntity {
                engine_id: engine.clone(),
                metadata: fact.clone(),
            })
            .collect();
        let engines: Vec<_> = state
            .facts
            .keys()
            .filter(|(_, id)| !id.is_empty())
            .map(|(engine, _)| engine.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|id| db::DockerCatalogEngine {
                runtime: state
                    .runtime
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| "Retained Docker engine".into()),
                id,
            })
            .collect();
        Ok(CatalogRead {
            overlay,
            engines,
            pin,
        })
    }
}

fn catalog_bytes(
    state: &State,
    q: &api::DockerRetainedRequest,
) -> Result<usize, DockerServiceError> {
    let mut engine_count = 0usize;
    let mut descriptor_bytes = 0usize;
    let mut previous_engine = None;
    for (engine, id) in state.facts.keys() {
        if id.is_empty() || previous_engine == Some(engine) {
            continue;
        }
        previous_engine = Some(engine);
        engine_count += 1;
        if engine_count > 256 {
            return Err(DockerServiceError::ResourceLimit(
                "Retained catalog engine bound exceeded",
            ));
        }
        descriptor_bytes += engine.len() + state.runtime.get(engine).map_or(22, String::len) + 64;
    }
    let held = state
        .facts
        .iter()
        .filter(|((engine, id), _)| {
            !id.is_empty()
                && q.engine_id
                    .as_ref()
                    .is_none_or(|selected| selected == engine)
        })
        .map(|(_, fact)| fact.extras.len() + 512)
        .sum::<usize>()
        .saturating_add(descriptor_bytes);
    if held > db::MAX_QUERY_BYTES
        || state
            .bytes
            .saturating_add(state.aux_bytes)
            .saturating_add(state.reader_bytes)
            .saturating_add(held)
            > RING_BYTES
    {
        return Err(DockerServiceError::ResourceLimit(
            "Retained catalog snapshot byte limit reached",
        ));
    }
    Ok(held)
}

fn live_catalog(
    q: &api::DockerRetainedRequest,
    overlay: Vec<db::DockerCatalogEntity>,
    engines: Vec<db::DockerCatalogEngine>,
) -> db::DockerCatalog {
    let search = q.search.to_ascii_lowercase();
    let mut entities: Vec<_> = overlay
        .into_iter()
        .filter(|row| {
            let m = &row.metadata;
            format!(
                "{} {} {} {} {}",
                m.name,
                m.image,
                m.project.as_deref().unwrap_or(""),
                m.service.as_deref().unwrap_or(""),
                m.external_id
            )
            .to_ascii_lowercase()
            .contains(&search)
        })
        .collect();
    entities.sort_by(|a, b| {
        (
            a.metadata.name.to_ascii_lowercase(),
            &a.engine_id,
            &a.metadata.external_id,
        )
            .cmp(&(
                b.metadata.name.to_ascii_lowercase(),
                &b.engine_id,
                &b.metadata.external_id,
            ))
    });
    let total = entities.len();
    let entities = entities
        .into_iter()
        .skip(q.offset as usize)
        .take(q.limit as usize)
        .collect();
    db::DockerCatalog {
        engines,
        entities,
        total,
    }
}

fn retained_page(
    q: &api::DockerRetainedRequest,
    catalog: db::DockerCatalog,
) -> Result<api::DockerRetainedPage, DockerServiceError> {
    let mut containers = Vec::with_capacity(catalog.entities.len());
    for row in catalog.entities {
        let mut container: api::DockerContainer = serde_json::from_slice(&row.metadata.extras)
            .map_err(|source| DockerServiceError::Metadata {
                message: "Retained container metadata is unavailable",
                source,
            })?;
        container.engine_id = row.engine_id;
        container.metrics = api::DockerMetrics::default();
        container.monitored = false;
        containers.push(container);
    }
    let consumed = q.offset.saturating_add(containers.len() as u32);
    let total = catalog.total.min(u32::MAX as usize) as u32;
    let page = api::DockerRetainedPage {
        engines: catalog
            .engines
            .into_iter()
            .map(|e| api::DockerRetainedEngine {
                id: e.id,
                runtime: e.runtime,
            })
            .collect(),
        containers,
        total,
        next_offset: (consumed < total).then_some(consumed),
    };
    Ok(page)
}
