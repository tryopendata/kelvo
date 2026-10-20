#!/bin/sh
# Entry point for `make rust-linux-test` inside rust:*-bookworm. Runs the command as the
# host's uid and gid, not root, like the CI runner: root reads mode-000 folders and any
# socket, which breaks the permission tests. The uid gets a passwd entry because
# ssh-keygen (the trust tests) refuses to run without one.
set -eu
: "${HOST_UID:?}" "${HOST_GID:?}"
getent group "$HOST_GID" >/dev/null || groupadd -g "$HOST_GID" kelvo
getent passwd "$HOST_UID" >/dev/null || useradd -M -d /tmp -u "$HOST_UID" -g "$HOST_GID" kelvo
exec setpriv --reuid="$HOST_UID" --regid="$HOST_GID" --clear-groups "$@"
