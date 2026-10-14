//! Conservative inventory evidence, per-resource classification and accounting.
use super::*;

pub(super) fn bytes(v: Option<i64>) -> Option<f64> {
    v.filter(|n| *n >= 0).map(|n| n as f64)
}
fn number(v: &Value, key: &str) -> Option<f64> {
    bytes(v.get(key).and_then(Value::as_i64))
}
fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(|s| bounded(s, 1024))
}
fn fingerprint(v: Value) -> String {
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
}
pub(super) fn volume_fingerprint(v: &Value) -> String {
    fingerprint(json!([
        v.get("Name"),
        v.get("CreatedAt"),
        v.get("Driver"),
        v.get("Options"),
        v.get("Labels")
    ]))
}
fn project(v: &Value) -> Option<String> {
    v.get("Labels")
        .and_then(|l| text(l, "com.docker.compose.project"))
}
pub(super) fn database(image: &str) -> Option<String> {
    let repo = image.split('@').next()?.rsplit(':').next_back()?;
    let repo = repo
        .strip_prefix("docker.io/")
        .unwrap_or(repo)
        .strip_prefix("library/")
        .unwrap_or(repo.strip_prefix("docker.io/").unwrap_or(repo));
    match repo {
        "postgres"
        | "mysql"
        | "mariadb"
        | "mongo"
        | "redis"
        | "valkey/valkey"
        | "couchdb"
        | "pgvector/pgvector"
        | "cassandra"
        | "clickhouse/clickhouse-server" => Some(repo.into()),
        _ => None,
    }
}
fn mounts(v: &Value) -> Vec<DockerStorageMount> {
    v.get("Mounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|m| DockerStorageMount {
            kind: text(m, "Type").unwrap_or_else(|| "unknown".into()),
            name: text(m, "Name"),
            source: text(m, "Source"),
            destination: text(m, "Destination"),
        })
        .collect()
}
fn resource(kind: DockerStorageKind, id: String) -> DockerStorageResource {
    DockerStorageResource {
        kind,
        name: id.clone(),
        id,
        project: None,
        size_bytes: None,
        reclaimable_bytes: None,
        references: vec![],
        age_ms: None,
        age_basis: None,
        database: None,
        mounts: vec![],
        auto_remove: None,
        can_delete: false,
        can_stop: false,
        reason: None,
        fingerprint: String::new(),
    }
}
/// Docker emits RFC3339 UTC timestamps. Unknown/non-UTC forms stay unknown.
pub(super) fn timestamp(s: &str) -> Option<i64> {
    if !s.ends_with('Z') {
        return None;
    }
    let (date, time) = s.trim_end_matches('Z').split_once('T')?;
    let d: Vec<i64> = date
        .split('-')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let t: Vec<i64> = time
        .split('.')
        .next()?
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [year, month, day] = d.as_slice() else {
        return None;
    };
    let [hour, minute, second] = t.as_slice() else {
        return None;
    };
    if !(1970..=9999).contains(year)
        || !(1..=12).contains(month)
        || !(1..=31).contains(day)
        || !(0..=23).contains(hour)
        || !(0..=59).contains(minute)
        || !(0..=59).contains(second)
    {
        return None;
    }
    // Gregorian civil date -> Unix days (400-year eras).
    let y = year - i64::from(*month <= 2);
    let era = y / 400;
    let yo = y - era * 400;
    let mp = month + if *month > 2 { -3 } else { 9 };
    let days = era * 146097 + yo * 365 + yo / 4 - yo / 100 + (153 * mp + 2) / 5 + day - 1 - 719468;
    days.checked_mul(86400)?
        .checked_add(hour * 3600 + minute * 60 + second)?
        .checked_mul(1000)
}
fn age(v: &Value, key: &str, now: i64) -> Option<i64> {
    timestamp(v.get(key)?.as_str()?)
        .filter(|time| *time <= now)
        .map(|time| now - time)
}
pub(super) fn classify(
    df: &Value,
    details: &BTreeMap<String, Value>,
    engine: &str,
    generation: u32,
    now: i64,
    mut complete: bool,
    exact_cache: bool,
) -> DockerStorageSnapshot {
    let mut rows = Vec::new();
    let containers = df.get("Containers").and_then(Value::as_array);
    let counts = ["Containers", "Images", "Volumes", "BuildCache"]
        .iter()
        .map(|key| df.get(key).and_then(Value::as_array).map_or(0, Vec::len))
        .sum::<usize>();
    if counts > MAX_ROWS || containers.is_none() {
        complete = false;
    }
    classify_containers(containers, details, now, &mut rows, &mut complete);
    classify_images(df, containers, &mut rows, &mut complete);
    classify_volumes(df, &mut rows, &mut complete);
    classify_cache(df, now, exact_cache, &mut rows, &mut complete);
    if !complete {
        for r in &mut rows {
            r.can_delete = false;
            r.can_stop = false;
            r.reason = Some("Incomplete reference inventory; refresh before any action".into());
        }
    }
    let categories = categories(df, &rows, counts, complete);
    let mut warnings = vec!["Docker-managed storage excludes VM allocation, logs, bind-mounted host files and other Buildx builders. Image and cache row sizes can overlap; do not add them.".into()];
    if !complete {
        warnings.push("Inventory is incomplete; actions are disabled. Scan admits at most 10,000 resources and 256 container inspections.".into());
    }
    if !exact_cache {
        warnings.push(
            "Cache cleanup requires an engine build with verified exact-ID filtering.".into(),
        );
    }
    DockerStorageSnapshot {
        engine_id: engine.into(),
        generation,
        observed_ms: now,
        complete,
        resources: rows,
        categories,
        warnings,
    }
}

fn classify_containers(
    containers: Option<&Vec<Value>>,
    details: &BTreeMap<String, Value>,
    now: i64,
    rows: &mut Vec<DockerStorageResource>,
    complete: &mut bool,
) {
    for c in containers.into_iter().flatten().take(MAX_ROWS) {
        let Some(id) = text(c, "Id").filter(|s| valid_id(s)) else {
            *complete = false;
            continue;
        };
        let detail = details.get(&id);
        if detail.is_none_or(|d| {
            d.get("Id").and_then(Value::as_str) != Some(&id)
                || d.get("Mounts").and_then(Value::as_array).is_none()
        }) || c.get("ImageID").and_then(Value::as_str).is_none()
        {
            *complete = false;
        }
        let mut r = resource(DockerStorageKind::Container, id);
        r.name = c
            .get("Names")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(|s| bounded(s.trim_start_matches('/'), 512))
            .unwrap_or(r.name);
        r.project = project(c);
        r.size_bytes = number(c, "SizeRw");
        r.mounts = mounts(detail.unwrap_or(c));
        r.database = text(c, "Image").and_then(|s| database(&s));
        let state = detail.and_then(|d| d.get("State"));
        r.auto_remove = detail
            .and_then(|d| d.get("HostConfig"))
            .and_then(|h| h.get("AutoRemove"))
            .and_then(Value::as_bool);
        let stopped = state
            .and_then(|s| s.get("Status"))
            .and_then(Value::as_str)
            .is_some_and(|s| matches!(s, "exited" | "created" | "dead"));
        let running =
            state.and_then(|s| s.get("Status")).and_then(Value::as_str) == Some("running");
        r.can_delete = stopped;
        r.can_stop = running && r.auto_remove == Some(false);
        r.reclaimable_bytes = Some(0.0);
        if stopped {
            r.age_ms = state.and_then(|s| age(s, "FinishedAt", now));
            r.age_basis = Some("stopped".into());
            r.reclaimable_bytes = r.size_bytes;
        }
        r.reason = if r.auto_remove == Some(true) {
            Some(
                "Stop is disabled: AutoRemove can delete the container and anonymous volumes"
                    .into(),
            )
        } else if !stopped && !r.can_stop {
            Some("Container state or AutoRemove metadata is unavailable".into())
        } else if running {
            Some("Running container; stop it before deletion".into())
        } else {
            None
        };
        r.fingerprint = fingerprint(json!([
            r.id,
            detail.and_then(|d| d.get("Created")),
            state.and_then(|s| s.get("Status")),
            state.and_then(|s| s.get("StartedAt")),
            state.and_then(|s| s.get("FinishedAt")),
            r.auto_remove,
            r.mounts
        ]));
        rows.push(r);
    }
}

fn classify_images(
    df: &Value,
    containers: Option<&Vec<Value>>,
    rows: &mut Vec<DockerStorageResource>,
    complete: &mut bool,
) {
    for i in df
        .get("Images")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_ROWS.saturating_sub(rows.len()))
    {
        let Some(id) = text(i, "Id").filter(|s| image_id(s)) else {
            *complete = false;
            continue;
        };
        let mut r = resource(DockerStorageKind::Image, id);
        r.name = i
            .get("RepoTags")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(|s| bounded(s, 1024))
            .unwrap_or(r.name);
        r.project = project(i);
        r.size_bytes = number(i, "Size");
        r.references = containers
            .into_iter()
            .flatten()
            .filter(|c| c.get("ImageID").and_then(Value::as_str) == Some(&r.id))
            .filter_map(|c| text(c, "Id"))
            .collect();
        r.can_delete =
            r.references.is_empty() && i.get("Containers").and_then(Value::as_i64) == Some(0);
        // Unique layers are conservative; shared ownership is never added per image.
        r.reclaimable_bytes = if r.can_delete {
            r.size_bytes
                .zip(number(i, "SharedSize"))
                .and_then(|(size, shared)| (shared <= size).then_some(size - shared))
        } else {
            Some(0.0)
        };
        r.reason =
            (!r.can_delete).then(|| "Image is referenced or reference count is unknown".into());
        r.fingerprint = fingerprint(json!([
            r.id,
            i.get("RepoTags"),
            i.get("RepoDigests"),
            i.get("Created")
        ]));
        rows.push(r);
    }
}

fn classify_volumes(df: &Value, rows: &mut Vec<DockerStorageResource>, complete: &mut bool) {
    for v in df
        .get("Volumes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_ROWS.saturating_sub(rows.len()))
    {
        let Some(id) = text(v, "Name").filter(|s| volume_id(s)) else {
            *complete = false;
            continue;
        };
        let mut r = resource(DockerStorageKind::Volume, id);
        r.project = project(v);
        r.size_bytes = v.get("UsageData").and_then(|v| number(v, "Size"));
        r.references = rows
            .iter()
            .filter(|c| {
                c.kind == DockerStorageKind::Container
                    && c.mounts
                        .iter()
                        .any(|m| m.kind == "volume" && m.name.as_ref() == Some(&r.id))
            })
            .map(|c| c.id.clone())
            .collect();
        r.database = rows
            .iter()
            .find(|c| r.references.contains(&c.id) && c.database.is_some())
            .and_then(|c| c.database.clone());
        r.can_delete = r.references.is_empty()
            && v.get("UsageData")
                .and_then(|v| v.get("RefCount"))
                .and_then(Value::as_i64)
                == Some(0)
            && v.get("CreatedAt").and_then(Value::as_str).is_some();
        let default_local = v.get("Driver").and_then(Value::as_str) == Some("local")
            && v.get("Options")
                .and_then(Value::as_object)
                .is_some_and(|options| options.is_empty());
        r.reclaimable_bytes = if !default_local {
            None
        } else if r.can_delete {
            r.size_bytes
        } else {
            Some(0.0)
        };
        r.reason = Some(if !r.can_delete {
            "Volume has container references or incomplete identity/reference metadata"
        } else if !default_local {
            "Driver or mount options determine deletion effects; reclaimed bytes are unknown. Previous use is unknown. Docker cannot prevent a concurrent replacement by another client."
        } else {
            "Previous use unknown; deleting permanently loses its data. Docker cannot prevent a concurrent replacement by another client."
        }.into());
        r.fingerprint = volume_fingerprint(v);
        rows.push(r);
    }
}

fn classify_cache(
    df: &Value,
    now: i64,
    exact_cache: bool,
    rows: &mut Vec<DockerStorageResource>,
    complete: &mut bool,
) {
    for c in df
        .get("BuildCache")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_ROWS.saturating_sub(rows.len()))
    {
        let Some(id) = text(c, "ID").filter(|s| cache_pattern(s).is_some()) else {
            *complete = false;
            continue;
        };
        let mut r = resource(DockerStorageKind::BuildCache, id);
        r.size_bytes = number(c, "Size");
        r.age_ms = age(c, "LastUsedAt", now);
        r.age_basis = Some("last_used".into());
        let unused = c.get("InUse").and_then(Value::as_bool) == Some(false);
        r.can_delete = exact_cache && unused;
        r.reclaimable_bytes = match (
            c.get("InUse").and_then(Value::as_bool),
            c.get("Shared").and_then(Value::as_bool),
        ) {
            (Some(false), Some(false)) => r.size_bytes,
            (Some(true), _) | (_, Some(true)) => Some(0.0),
            _ => None,
        };
        r.reason = if !exact_cache {
            Some("Exact cache cleanup is unsupported for this engine build".into())
        } else if !unused {
            Some("Cache is in use or usage is unknown".into())
        } else {
            None
        };
        r.fingerprint = fingerprint(json!([
            r.id,
            c.get("CreatedAt"),
            c.get("LastUsedAt"),
            c.get("InUse"),
            c.get("Parents"),
            c.get("Parent")
        ]));
        rows.push(r);
    }
}

fn categories(
    df: &Value,
    rows: &[DockerStorageResource],
    counts: usize,
    complete: bool,
) -> Vec<DockerStorageCategory> {
    let mut categories = Vec::new();
    for (kind, key) in [
        (DockerStorageKind::Container, "Containers"),
        (DockerStorageKind::Image, "Images"),
        (DockerStorageKind::Volume, "Volumes"),
        (DockerStorageKind::BuildCache, "BuildCache"),
    ] {
        let known = df.get(key).and_then(Value::as_array).is_some() && counts <= MAX_ROWS;
        let selected: Vec<_> = rows.iter().filter(|r| r.kind == kind).collect();
        let size_bytes = if !known {
            None
        } else if kind == DockerStorageKind::Image {
            number(df, "LayersSize")
        } else {
            selected.iter().map(|r| r.size_bytes).sum::<Option<f64>>()
        };
        let reclaimable_bytes = if known && complete {
            selected
                .iter()
                .map(|r| r.reclaimable_bytes)
                .sum::<Option<f64>>()
        } else {
            None
        };
        categories.push(DockerStorageCategory {
            kind,
            size_bytes,
            reclaimable_bytes,
        });
    }
    categories
}
