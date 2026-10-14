//! Closed Docker endpoint policy and bounded Unix-socket transport.
use super::*;

pub(super) fn unversioned(path: &str) -> &str {
    if path.starts_with("/v1.") {
        path.get(1..)
            .and_then(|p| p.find('/').map(|i| &p[i..]))
            .unwrap_or("")
    } else {
        path
    }
}
pub(super) fn volume_id(id: &str) -> bool {
    id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && id.len() <= 255
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
pub(super) fn image_id(id: &str) -> bool {
    id.strip_prefix("sha256:")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn decode_query(s: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut it = s.bytes();
    while let Some(b) = it.next() {
        bytes.push(if b == b'%' {
            let a = (it.next()? as char).to_digit(16)?;
            let b = (it.next()? as char).to_digit(16)?;
            (a * 16 + b) as u8
        } else if b == b'+' {
            b' '
        } else {
            b
        });
    }
    String::from_utf8(bytes).ok()
}
fn query_map(query: Option<&str>) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for part in query.unwrap_or("").split('&').filter(|s| !s.is_empty()) {
        let (k, v) = part.split_once('=')?;
        if out.insert(decode_query(k)?, decode_query(v)?).is_some() {
            return None;
        }
    }
    Some(out)
}
pub(super) fn cache_pattern(id: &str) -> Option<String> {
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return None;
    }
    let mut out = String::from("^");
    for c in id.chars() {
        if r"\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('$');
    Some(out)
}
/// Only one literal, anchored cache ID; reject regex constructs and extra filters.
pub(super) fn literal_cache_filter(value: &str) -> bool {
    let Ok(v) = serde_json::from_str::<BTreeMap<String, Vec<String>>>(value) else {
        return false;
    };
    if v.len() != 1 {
        return false;
    }
    let Some(values) = v.get("id") else {
        return false;
    };
    if values.len() != 1 {
        return false;
    }
    let Some(pattern) = values.first() else {
        return false;
    };
    let Some(inner) = pattern.strip_prefix('^').and_then(|s| s.strip_suffix('$')) else {
        return false;
    };
    let mut id = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let Some(next) = chars.next() else {
                return false;
            };
            id.push(next);
        } else {
            id.push(c);
        }
    }
    cache_pattern(&id).as_ref() == Some(pattern)
}
pub(super) fn allowed(
    method: &hyper::Method,
    path: &str,
    query: Option<&str>,
    actions: bool,
    exact_cache: bool,
) -> bool {
    let path = unversioned(path);
    let Some(q) = query_map(query) else {
        return false;
    };
    let p: Vec<_> = path.split('/').collect();
    if method == hyper::Method::GET {
        return q.is_empty()
            && (matches!(path, "/version" | "/info" | "/system/df")
                || (p.len() == 4
                    && p.get(1) == Some(&"containers")
                    && p.get(2).is_some_and(|s| valid_id(s))
                    && p.get(3) == Some(&"json"))
                || (p.len() == 3
                    && p.get(1) == Some(&"volumes")
                    && p.get(2).is_some_and(|s| volume_id(s))));
    }
    if !actions {
        return false;
    }
    let equals = |values: &[(&str, &str)]| {
        q.len() == values.len()
            && values
                .iter()
                .all(|(k, v)| q.get(*k).is_some_and(|actual| actual == v))
    };
    if method == hyper::Method::DELETE && p.len() == 3 {
        return match (p.get(1), p.get(2)) {
            (Some(&"containers"), Some(id)) => {
                valid_id(id) && equals(&[("v", "false"), ("force", "false"), ("link", "false")])
            }
            (Some(&"images"), Some(id)) => {
                image_id(id) && equals(&[("force", "false"), ("noprune", "true")])
            }
            (Some(&"volumes"), Some(id)) => volume_id(id) && equals(&[("force", "false")]),
            _ => false,
        };
    }
    if method == hyper::Method::POST
        && p.len() == 4
        && p.get(1) == Some(&"containers")
        && p.get(2).is_some_and(|s| valid_id(s))
    {
        return match p.get(3) {
            Some(&"stop") => equals(&[("t", "30")]),
            Some(&"start") => q.is_empty(),
            _ => false,
        };
    }
    method == hyper::Method::POST
        && path == "/build/prune"
        && exact_cache
        && q.len() == 2
        && q.get("all").is_some_and(|s| s == "true")
        && q.get("filters").is_some_and(|s| literal_cache_filter(s))
}
pub(super) fn client(
    socket: String,
    actions: bool,
    exact_cache: bool,
) -> Result<Docker, DockerError> {
    if !Path::new(&socket).is_absolute() || socket.contains("://") {
        return Err(DockerError::Endpoint);
    }
    Ok(Docker::connect_with_custom_transport(
        move |mut req: BollardRequest| {
            let socket = socket.clone();
            async move {
                if !allowed(
                    req.method(),
                    req.uri().path(),
                    req.uri().query(),
                    actions,
                    exact_cache,
                ) {
                    return Err(bollard::errors::Error::IOError {
                        err: std::io::Error::other("Docker storage endpoint denied"),
                    });
                }
                let stream = UnixStream::connect(socket)
                    .await
                    .map_err(|err| bollard::errors::Error::IOError { err })?;
                let io = hyper_util::rt::TokioIo::new(BoundedIo {
                    stream,
                    left: 16 * 1024 * 1024,
                });
                let (mut sender, connection) = hyper::client::conn::http1::handshake(io).await?;
                tokio::spawn(async move {
                    let _ = connection.await;
                });
                let path = req.uri().path_and_query().cloned().ok_or_else(|| {
                    bollard::errors::Error::IOError {
                        err: std::io::Error::other("Invalid Docker URI"),
                    }
                })?;
                *req.uri_mut() = hyper::Uri::builder().path_and_query(path).build()?;
                sender.send_request(req).await.map_err(Into::into)
            }
        },
        Some("http://localhost"),
        60,
        API_DEFAULT_VERSION,
    )?)
}
