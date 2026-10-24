//! Search retained metadata and merge the bounded live overlay.
use super::*;

impl DockerReadSnapshot<'_> {
    /// Search the latest retained metadata in this read transaction.
    pub fn catalog(
        &self,
        engine: Option<&str>,
        search: &str,
        offset: usize,
        limit: usize,
    ) -> Result<DockerCatalog> {
        self.catalog_with_overlay(engine, search, offset, limit, &[], &[])
    }
    /// Ring facts override durable facts by daemon/container identity. Merge the
    /// sorted streams while retaining only the requested page, never all DB rows.
    #[allow(clippy::too_many_arguments)]
    #[expect(
        clippy::too_many_lines,
        reason = "split when substantially changed; architecture refactor section 10"
    )]
    pub fn catalog_with_overlay(
        &self,
        engine: Option<&str>,
        search: &str,
        offset: usize,
        limit: usize,
        overlay: &[DockerCatalogEntity],
        overlay_engines: &[DockerCatalogEngine],
    ) -> Result<DockerCatalog> {
        if search.len() > 128
            || offset > 100_000
            || limit == 0
            || limit > 100
            || engine.is_some_and(|id| id.len() > 256)
            || overlay_engines.len() > 256
        {
            return Err(invalid("catalog search or page bounds exceeded"));
        }
        self.check(0)?;
        let mut bytes = 0;
        let mut identities = std::collections::BTreeMap::new();
        for entity in overlay {
            bytes += pack_limited(&entity.metadata, MAX_QUERY_BYTES.saturating_sub(bytes))?.len()
                + entity.engine_id.len()
                + 128;
            self.check(bytes)?;
            if !entity.metadata.external_id.is_empty() {
                identities.insert((&entity.engine_id, &entity.metadata.external_id), entity);
            }
        }
        let mut out = DockerCatalog::default();
        let mut engines=self.conn.prepare("SELECT d.daemon_id,d.runtime FROM docker_engines d JOIN hosts h ON h.id=d.host_id WHERE h.uuid=?1 AND EXISTS(SELECT 1 FROM docker_entities e JOIN docker_metadata m ON m.entity_id=e.id WHERE e.engine_id=d.id AND e.kind='container') ORDER BY d.daemon_id LIMIT 257")?;
        let mut rows = engines.query([self.host.to_string()])?;
        let mut known = std::collections::BTreeMap::new();
        while let Some(row) = rows.next()? {
            let engine = DockerCatalogEngine {
                id: row.get(0)?,
                runtime: row.get(1)?,
            };
            bytes += engine.id.len() + engine.runtime.len() + 64;
            self.check(bytes)?;
            known.insert(engine.id.clone(), engine);
        }
        // Caller derives these descriptors from its bounded metadata ownership
        // map, independently of the selected page's entity overlay.
        for engine in overlay_engines {
            bytes += engine.id.len() + engine.runtime.len() + 64;
            self.check(bytes)?;
            known.insert(engine.id.clone(), engine.clone());
        }
        if known.len() > 256 {
            return Err(invalid("catalog engine bound exceeded"));
        }
        out.engines = known.into_values().collect();
        let needle = search.to_ascii_lowercase();
        let mut live: Vec<_> = identities
            .values()
            .copied()
            .filter(|e| {
                engine.is_none_or(|id| id == e.engine_id)
                    && format!(
                        "{} {} {} {} {}",
                        e.metadata.name,
                        e.metadata.image,
                        e.metadata.project.as_deref().unwrap_or(""),
                        e.metadata.service.as_deref().unwrap_or(""),
                        e.metadata.external_id
                    )
                    .to_ascii_lowercase()
                    .contains(&needle)
            })
            .collect();
        live.sort_by(|a, b| catalog_key(a).cmp(&catalog_key(b)));
        let mut live = live.into_iter().peekable();
        let mut append = |entity: &DockerCatalogEntity| -> Result<()> {
            if out.total >= offset && out.entities.len() < limit {
                bytes += pack_limited(&entity.metadata, MAX_QUERY_BYTES.saturating_sub(bytes))?
                    .len()
                    + entity.engine_id.len()
                    + 128;
                self.check(bytes)?;
                out.entities.push(entity.clone());
            }
            out.total += 1;
            Ok(())
        };
        // Fixed SQL with parameterized identity/search; metadata latest lookup is
        // covered by its entity-local primary index. Rust merge uses the same
        // ASCII lower/binary order as SQLite's built-in lower().
        let mut stmt=self.conn.prepare("SELECT d.daemon_id,e.external_id,m.effective_ms,m.revision,m.name,m.image,m.project,m.service,m.lifecycle,m.explicit_limit,m.effective_limit,m.capacity,m.last_seen_ms,m.tombstone,m.extras FROM docker_entities e JOIN docker_engines d ON d.id=e.engine_id JOIN hosts h ON h.id=e.host_id JOIN docker_metadata m ON m.rowid=(SELECT rowid FROM docker_metadata latest WHERE latest.entity_id=e.id ORDER BY latest.effective_ms DESC,latest.revision DESC LIMIT 1) WHERE h.uuid=?1 AND e.kind='container' AND (?2 IS NULL OR d.daemon_id=?2) AND instr(lower(m.name || ' ' || m.image || ' ' || coalesce(m.project,'') || ' ' || coalesce(m.service,'') || ' ' || e.external_id),lower(?3))>0 ORDER BY lower(m.name),d.daemon_id,e.external_id")?;
        let mut rows = stmt.query(params![self.host.to_string(), engine, search])?;
        while let Some(row) = rows.next()? {
            self.check(0)?;
            let metadata = DockerEntity {
                external_id: row.get(1)?,
                effective_ms: row.get(2)?,
                revision: row.get(3)?,
                name: row.get(4)?,
                image: row.get(5)?,
                project: row.get(6)?,
                service: row.get(7)?,
                lifecycle: row.get(8)?,
                explicit_limit: row.get(9)?,
                effective_limit: row.get(10)?,
                capacity: row.get(11)?,
                last_seen_ms: row.get(12)?,
                tombstone: row.get(13)?,
                extras: row.get(14)?,
            };
            let entity = DockerCatalogEntity {
                engine_id: row.get(0)?,
                metadata,
            };
            if identities.contains_key(&(&entity.engine_id, &entity.metadata.external_id)) {
                continue;
            }
            let key = catalog_key(&entity);
            while live.peek().is_some_and(|e| catalog_key(e) < key) {
                if let Some(e) = live.next() {
                    append(e)?;
                }
            }
            append(&entity)?;
        }
        for entity in live {
            append(entity)?;
        }
        self.check(bytes)?;
        Ok(out)
    }
}

fn catalog_key(entity: &DockerCatalogEntity) -> (String, &str, &str) {
    (
        entity.metadata.name.to_ascii_lowercase(),
        &entity.engine_id,
        &entity.metadata.external_id,
    )
}
