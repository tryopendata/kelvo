//! The systemd side: the scripts that write the unit through the agent, start and stop
//! it, and wait for the daemon. Each takes a target's [`Words`].
use super::Words;
use crate::agent_layout::{UNIT_NAME, UPDATE_TIMER};

/// The history files an update copies aside: the database and its WAL and shared
/// memory. The `.lock` file is not history.
const HISTORY_FILES: &str = "history.sqlite history.sqlite-wal history.sqlite-shm";

/// Polls `status --json` for up to `seconds` until the daemon answers, then prints the
/// last status line. Always exits 0; the caller reads `running` from the status.
pub(crate) fn wait_running(w: &Words, seconds: u32) -> String {
    let polls = seconds * 4;
    format!(
        r#"i=0; out=
while [ $i -lt {polls} ]; do
  out=$({bin} status --json{all} 2>/dev/null | head -n 1) || true
  case $out in *running?:true*) break;; esac
  sleep 0.25 2>/dev/null || sleep 1
  i=$((i + 1))
done
printf "status=%s\n" "$out"
"#,
        bin = w.bin,
        all = w.all,
    )
}

/// After an upload: write the unit, record the history budget, enable and start it.
pub(crate) fn activate(w: &Words, configure_json: &str) -> String {
    format!(
        r#"set -eu
{bin} install-unit{all} >/dev/null
{bin} configure {configure_json}{data_only} >/dev/null
{systemctl} daemon-reload
{systemctl} enable --now {UNIT_NAME}
{systemctl} enable --now {UPDATE_TIMER}
set +e
{wait}"#,
        bin = w.bin,
        all = w.all,
        data_only = w.data_only,
        systemctl = w.systemctl,
        // The daemon waits up to 3 s for Docker names before it binds its socket.
        wait = wait_running(w, 10),
    )
}

/// Before an update's upload: drop any backup and `.prev` binary an earlier attempt
/// left (first, so a stale backup can never be restored over newer history), stop the
/// daemon so the history is consistent, and copy it aside.
pub(crate) fn stop_and_back_up(w: &Words) -> String {
    format!(
        r#"set -eu
umask 077
data={data}
bin={bin}
rm -f "${{bin%/*}}/.kelvo-agent.prev"
rm -rf "$data/backup"
{systemctl} stop {UNIT_NAME}
mkdir -p "$data/backup"
for f in {HISTORY_FILES}; do
  if [ -e "$data/$f" ]; then cp -p "$data/$f" "$data/backup/$f"; fi
done
: > "$data/backup/complete"
"#,
        data = w.data,
        bin = w.bin,
        systemctl = w.systemctl,
    )
}

/// After an update's upload: rewrite the unit with the new binary and start it.
pub(crate) fn restart(w: &Words) -> String {
    format!(
        r#"set -eu
{bin} install-unit{all} >/dev/null
{systemctl} daemon-reload
{systemctl} start {UNIT_NAME}
{systemctl} enable --now {UPDATE_TIMER}
set +e
{wait}"#,
        bin = w.bin,
        all = w.all,
        systemctl = w.systemctl,
        wait = wait_running(w, 10),
    )
}

/// After a failed update: put the previous binary and the copied history back, rewrite
/// the unit with the old binary and start it. Runs every step and prints
/// `restored=1|0` after the status line. With neither a `.prev` binary nor a complete
/// backup, the update failed before it replaced anything (stopping, or copying the
/// history aside): the history is left as it is, and only the restart decides.
pub(crate) fn roll_back(w: &Words) -> String {
    format!(
        r#"set -u
ok=1
data={data}
bin={bin}
prev=${{bin%/*}}/.kelvo-agent.prev
{systemctl} stop {UNIT_NAME} 2>/dev/null
replaced=0
if [ -e "$prev" ]; then
  replaced=1
  mv -f "$prev" "$bin" || ok=0
  if command -v restorecon >/dev/null 2>&1; then restorecon -F "$bin"; fi
fi
if [ -f "$data/backup/complete" ]; then
  for f in {HISTORY_FILES}; do
    rm -f "$data/$f"
    if [ -e "$data/backup/$f" ]; then cp -p "$data/backup/$f" "$data/$f" || ok=0; fi
  done
else
  rm -rf "$data/backup"
  if [ $replaced = 1 ]; then ok=0; fi
fi
{bin} install-unit{all} >/dev/null || ok=0
{systemctl} daemon-reload || ok=0
{systemctl} start {UNIT_NAME} || ok=0
{wait}case $out in *running?:true*) ;; *) ok=0;; esac
if [ $ok = 1 ]; then rm -rf "$data/backup"; fi
printf "restored=%s\n" $ok
"#,
        data = w.data,
        bin = w.bin,
        all = w.all,
        systemctl = w.systemctl,
        wait = wait_running(w, 10),
    )
}

/// After a successful update: the copied history is no longer needed.
pub(crate) fn drop_backup(w: &Words) -> String {
    format!("data={data}; rm -rf \"$data/backup\"", data = w.data)
}

/// Stops the daemon, gives the data directory a new host id, starts it again. The
/// daemon is started even when the reset fails, so the host is never left stopped.
pub(crate) fn reset_identity(w: &Words) -> String {
    format!(
        r#"set -u
cap=$({bin} version --json 2>/dev/null)
case "$cap" in *automatic_updates?:true*)
  {bin} reset-identity{data_only} >/dev/null
  rc=$?
  {wait}exit $rc
  ;;
esac
{systemctl} stop {UNIT_NAME}
{bin} reset-identity{data_only} >/dev/null
rc=$?
{systemctl} start {UNIT_NAME}
{wait}exit $rc
"#,
        bin = w.bin,
        data_only = w.data_only,
        systemctl = w.systemctl,
        wait = wait_running(w, 10),
    )
}

/// Removes everything: stops and disables the unit, lets the agent remove its unit (and
/// with `purge` its data), then removes the unit, binary and socket directly so a
/// missing or broken binary still uninstalls. Prints one `step=outcome` line per step
/// and always exits 0 (D-105: removal never blocks on the remote side).
pub(crate) fn uninstall(w: &Words, purge: bool) -> String {
    let purge_flag = if purge { " --purge" } else { "" };
    let purge_data = if purge {
        r#"if [ -e "$data" ]; then step data rm -rf "$data"; else echo data=absent; fi"#
    } else {
        ""
    };
    format!(
        r#"set -u
step() {{
  name=$1; shift
  if out=$("$@" 2>&1); then echo "$name=ok"
  else printf "%s=failed: %s\n" "$name" "$(printf "%s" "$out" | head -n 1)"; fi
}}
bin={bin}
data={data}
unit={unit}
sock={socket}
dir=${{bin%/*}}
cap=$("$bin" version --json 2>/dev/null) || cap=
case "$cap" in *automatic_updates?:true*)
  if out=$("$bin" uninstall{purge_flag}{data_only}); then
    printf "agent-report=%s\n" "$out"
    step reload {systemctl} daemon-reload
  else printf "agent=failed: uninstall command failed: %s\n" "$out"; fi
  exit 0
  ;;
esac
if [ -e "$unit" ]; then step stop {systemctl} disable --now {UNIT_NAME}; else echo stop=absent; fi
if [ -x "$bin" ]; then step agent "$bin" uninstall{purge_flag}{data_only}; else echo agent=absent; fi
if [ -e "$unit" ]; then step unit rm -f "$unit"; else echo unit=absent; fi
{purge_data}
step reload {systemctl} daemon-reload
{systemctl} reset-failed {UNIT_NAME} >/dev/null 2>&1
if [ -e "$bin" ]; then step binary rm -f "$bin"; else echo binary=absent; fi
rm -f "$dir/.kelvo-agent.prev" "$dir/.kelvo-agent.tmp" "$sock"
rmdir "${{sock%/*}}" 2>/dev/null
exit 0
"#,
        bin = w.bin,
        data = w.data,
        unit = w.unit,
        socket = w.socket,
        systemctl = w.systemctl,
        data_only = w.data_only,
    )
}

/// New agents submit the work to their own service. SSH only queues and observes;
/// ending this connection cannot interrupt replacement or recovery.
pub(crate) fn request_update(w: &Words) -> String {
    format!(
        "set -eu\n{} update --request{}\n{}",
        w.bin,
        w.all,
        wait_running(w, 10)
    )
}
