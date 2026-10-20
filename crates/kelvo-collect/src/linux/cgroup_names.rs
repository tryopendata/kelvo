//! What a cgroup v2 path is (D-116): a system service, a user-manager service, or a
//! Docker or Podman container. Shared by the cgroup collector, which walks the tree,
//! and the process collector, which reads `/proc/<pid>/cgroup` once per process.
//!
//! Sessions (`session-*.scope`), slices, `init.scope` and `user@<uid>.service` itself
//! are not units here: processes in them belong to no app, and the cgroup collector
//! does not report them.

use std::sync::Arc;

use super::SysRoot;
use super::procfs::user_name;
use crate::ContainerNames;

/// Characters of a container id shown when its name is unknown, as `docker ps` does.
pub(super) const SHORT_ID: usize = 12;

/// The `kind` label of a `cgroup.*` series.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Kind {
    Service,
    Docker,
    Podman,
    User,
    Other,
}

impl Kind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Kind::Service => "service",
            Kind::Docker => "docker",
            Kind::Podman => "podman",
            Kind::User => "user",
            Kind::Other => "other",
        }
    }
}

/// A cgroup that is a unit or a container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Unit<'a> {
    /// A system service: `nginx.service`, `getty@tty1.service`.
    Service(&'a str),
    /// A service of user `uid`'s systemd instance.
    User { uid: u32, unit: &'a str },
    /// A Docker container by full id (`docker-<id>.scope`, or `docker/<id>` with the
    /// cgroupfs driver).
    Docker(&'a str),
    /// A Podman container by full id (`libpod-<id>.scope`).
    Podman(&'a str),
}

fn is_id(s: &str) -> bool {
    s.len() >= SHORT_ID && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The id in a `docker-<id>.scope` directory name.
pub(super) fn docker_scope(name: &str) -> Option<&str> {
    let id = name.strip_prefix("docker-")?.strip_suffix(".scope")?;
    is_id(id).then_some(id)
}

/// The id in a `libpod-<id>.scope` directory name. `libpod-conmon-<id>.scope`, the
/// monitor process beside a container, is not the container.
pub(super) fn podman_scope(name: &str) -> Option<&str> {
    let id = name.strip_prefix("libpod-")?.strip_suffix(".scope")?;
    is_id(id).then_some(id)
}

/// A bare container id directory (`/sys/fs/cgroup/docker/<id>`, the cgroupfs driver).
pub(super) fn bare_id(name: &str) -> Option<&str> {
    is_id(name).then_some(name)
}

/// The uid in a `user@<uid>.service` or `user-<uid>.slice` name.
pub(super) fn user_uid(name: &str, prefix: &str, suffix: &str) -> Option<u32> {
    name.strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .parse()
        .ok()
}

/// The unified-hierarchy path in `/proc/<pid>/cgroup` (its `0::` line).
pub(super) fn proc_cgroup_path(text: &str) -> Option<&str> {
    text.lines().find_map(|l| l.strip_prefix("0::"))
}

/// What the cgroup at `path` (`/system.slice/nginx.service`) is, if a unit.
pub(super) fn classify(path: &str) -> Option<Unit<'_>> {
    let mut parts = path.trim_matches('/').split('/');
    let first = parts.next()?;
    let mut prev = first;
    let mut service = None;
    let mut user = None;
    for part in std::iter::once(first).chain(parts) {
        if let Some(id) = docker_scope(part) {
            return Some(Unit::Docker(id));
        }
        if prev == "docker"
            && let Some(id) = bare_id(part)
        {
            return Some(Unit::Docker(id));
        }
        if let Some(id) = podman_scope(part) {
            return Some(Unit::Podman(id));
        }
        if first == "user.slice" {
            if let Some(uid) = user_uid(part, "user@", ".service") {
                user = Some(uid);
            } else if let Some(uid) = user
                && service.is_none()
                && part.ends_with(".service")
            {
                service = Some(Unit::User { uid, unit: part });
            }
        } else if first == "system.slice" && service.is_none() && part.ends_with(".service") {
            service = Some(Unit::Service(part));
        }
        prev = part;
    }
    service
}

/// The first [`SHORT_ID`] characters of a container id.
pub(super) fn short_id(id: &str) -> &str {
    id.get(..SHORT_ID).unwrap_or(id)
}

/// A container's name, once known. A miss asks the refresher for a pass and records
/// when, so a name that is coming is waited for and one that never comes falls back to
/// the short id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Resolved {
    Named(Arc<str>),
    /// Waiting for the refresh that started after this generation.
    Pending(u64),
}

/// Looks `id` up in `names`. `pending` is the generation recorded by an earlier miss.
pub(super) fn resolve(names: &ContainerNames, id: &str, pending: Option<u64>) -> Resolved {
    if let Some(name) = names.get(id) {
        return Resolved::Named(name);
    }
    match pending {
        Some(asked) if names.settled_since(asked) => Resolved::Named(Arc::from(short_id(id))),
        Some(asked) => Resolved::Pending(asked),
        None => {
            let asked = names.generation();
            if names.settled_since(asked) {
                return Resolved::Named(Arc::from(short_id(id)));
            }
            names.request();
            Resolved::Pending(asked)
        }
    }
}

/// The `name` label of a user-manager service: `<user>/<unit>`, so two users' units of
/// the same name stay apart.
pub(super) fn user_unit_name(root: &SysRoot, uid: u32, unit: &str) -> String {
    format!("{}/{unit}", user_name(root, uid))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "ba1d8caf4daec08a0dfd56c5d1e80e270631bedc443634cc398d78deb1769013";

    #[test]
    fn classifies_units_and_containers() {
        let docker = format!("/system.slice/docker-{ID}.scope");
        assert_eq!(classify(&docker), Some(Unit::Docker(ID)));
        assert_eq!(
            classify(&format!("/docker/{ID}")),
            Some(Unit::Docker(ID)),
            "cgroupfs driver"
        );
        assert_eq!(
            classify(&format!("/machine.slice/libpod-{ID}.scope/container")),
            Some(Unit::Podman(ID))
        );
        assert_eq!(
            classify(&format!("/machine.slice/libpod-conmon-{ID}.scope")),
            None
        );
        assert_eq!(
            classify("/system.slice/nginx.service"),
            Some(Unit::Service("nginx.service"))
        );
        assert_eq!(
            classify("/system.slice/system-getty.slice/getty@tty1.service"),
            Some(Unit::Service("getty@tty1.service"))
        );
        assert_eq!(
            classify("/user.slice/user-1000.slice/user@1000.service/app.slice/pipewire.service"),
            Some(Unit::User {
                uid: 1000,
                unit: "pipewire.service"
            })
        );
        assert_eq!(
            classify(&format!(
                "/user.slice/user-1000.slice/user@1000.service/user.slice/libpod-{ID}.scope"
            )),
            Some(Unit::Podman(ID))
        );
        for none in [
            "/",
            "/init.scope",
            "/user.slice/user-0.slice/session-23.scope",
            "/user.slice/user-0.slice/user@0.service",
            "/user.slice/user-0.slice/user@0.service/init.scope",
            "/system.slice",
            "/system.slice/system-getty.slice",
            "/system.slice/docker-xyz.scope",
            "/system.slice/boot.mount",
            "/machine.slice/machine-qemu.scope",
        ] {
            assert_eq!(classify(none), None, "{none}");
        }
    }

    #[test]
    fn reads_the_unified_line() {
        let text = "12:pids:/x\n0::/system.slice/sshd.service\n";
        assert_eq!(proc_cgroup_path(text), Some("/system.slice/sshd.service"));
        assert_eq!(proc_cgroup_path("1:name=systemd:/x\n"), None, "v1 only");
    }

    #[test]
    fn names_resolve_wait_and_fall_back_to_short_ids() {
        // Nobody fills the map: the short id at once.
        let none = ContainerNames::default();
        assert_eq!(
            resolve(&none, ID, None),
            Resolved::Named(Arc::from("ba1d8caf4dae"))
        );
        // An attached map: the miss waits, then takes the name the refresh found.
        let names = ContainerNames::attached();
        let Resolved::Pending(asked) = resolve(&names, ID, None) else {
            panic!("should wait");
        };
        assert!(names.requested());
        assert_eq!(resolve(&names, ID, Some(asked)), Resolved::Pending(asked));
        names.finish(Some([(ID.to_owned(), Arc::from("nginx"))].into()));
        assert_eq!(
            resolve(&names, ID, Some(asked)),
            Resolved::Named(Arc::from("nginx"))
        );
        // A refresh that did not find it (or failed): the short id.
        let other = "47d70c0c2814b54a3bf9cfc143c8c4ec5632efd4732a4c9afa4f7d831daf270b";
        let Resolved::Pending(asked) = resolve(&names, other, None) else {
            panic!("should wait");
        };
        names.finish(None);
        assert_eq!(
            resolve(&names, other, Some(asked)),
            Resolved::Named(Arc::from("47d70c0c2814"))
        );
    }
}
