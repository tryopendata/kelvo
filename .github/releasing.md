# Releasing Kelvo

Kelvo stays free. The public channel is an Apple Silicon DMG on GitHub Releases,
ad-hoc signed without Apple notarization. Mac App Store and Homebrew distribution
are outside this release workflow.

## Release identity and signing

- Keep `Cargo.toml` (workspace version), `package.json` and
  `src-tauri/tauri.conf.json` on the same stable version.
- `src-tauri/tauri.conf.json` contains the public updater key. The private key lives
  in the repository Actions secret `TAURI_SIGNING_PRIVATE_KEY`. An encrypted key
  also needs `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- The initial key has an owner-only local backup at
  `~/.config/kelvo/release/updater.key`. Back it up securely. Never commit it,
  print it in CI logs, or regenerate it during a release. Losing it prevents
  updates to existing installations; replacing the key requires a planned transition.
- Updater signatures bind both archive contents and the app version. These are
  independent of Apple's code signing and require no Apple membership.

## Publish

1. Update the three versions and `RELEASE_NOTES.md` plus `CHANGELOG.md`. Run `make check`,
   `bun run test:e2e`, and `python3 scripts/test-release-manifest.py`.
2. Commit and push main through the normal Git hooks, then create and push the
   matching `vMAJOR.MINOR.PATCH` tag. Hosted CI uses Linux only; the Release
   workflow validates version agreement and manifest tests, without macOS runners.
3. Build locally using the command below after `make agent-linux` succeeds.
   Verify the app with `codesign --verify --deep --strict`, confirm the bundled
   helper and both Linux agents, and run `scripts/check-deps.sh` against the app
   and helper executables. Stage with `python3 scripts/release-manifest.py TAG --stage`
   then sign `release.json` and generate checksums using the commands below. Run
   `cargo run -p kelvo --example verify_release -- target/release-assets`. This verifies
   the catalog signature, protocol/version agreement, and every advertised binary
   before exercising the Mac archive installation.
   Run `cargo build -p kelvo --example verify_update_restart` followed by
   `python3 scripts/verify-update-restart.py` for isolated native restart acceptance.
   It checks ordinary, background and remote-setup launches with real shutdown,
   persistent identity/history and no restarted windows; a real login-session launch
   remains part of the fresh-account check below.
   Create the draft with `gh release create TAG target/release-assets/* --verify-tag
   --draft --title "Kelvo TAG" --notes-file RELEASE_NOTES.md`. It must contain
   `Kelvo_aarch64.dmg`, `Kelvo.app.tar.gz`, its `.sig`, `latest.json`, `release.json`,
   `release.json.sig`, both standalone Linux musl agents, and `SHA256SUMS`.
4. Download the draft DMG. Verify the signature with `codesign --verify --deep
   --strict Kelvo.app`, install and launch it on a fresh account/second Mac, and
   check a real update from the previous release when one exists. Confirm history
   and settings survive and note any Gatekeeper prompts. Browser tests cannot
   establish native installation or restart behavior.
5. Publish the complete draft as the latest stable release. Only then does the
   fixed `/releases/latest/download/latest.json` and `release.json` endpoints advertise it to clients.

Never replace published updater archives or reuse a version. Fix a bad release
with a higher version. A failed draft can be deleted and rebuilt from its existing
tag using the local build procedure. Do not move published tags.

## Local release build

With the signing key stored at the path above:

```sh
TAURI_SIGNING_PRIVATE_KEY="$HOME/.config/kelvo/release/updater.key" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
bun run build:app --config src-tauri/tauri.release.conf.json --bundles app,dmg
python3 scripts/release-manifest.py v0.3.0 --stage
bun run tauri signer sign --private-key-path "$HOME/.config/kelvo/release/updater.key" \
  --password '' --app-version 0.3.0 target/release-assets/release.json
python3 scripts/release-manifest.py v0.3.0 --checksums
cargo run -p kelvo --example verify_release -- target/release-assets
```

Use the actual version tag. `target/release-assets` must be empty before staging;
move a previous deliverable aside rather than mixing files between releases.
Ordinary local builds do not create signed updater archives and need no private key.

## Update behavior

Automatic checking and installation are host-owned preferences. Existing disabled
checks remain disabled. Stable releases install automatically only when their signed
protocol range retains every protocol version supported by the installed release.
Explicit checks still work with automatic checks disabled.

The Mac app downloads verified releases and waits until interactive windows are hidden
and maintenance has finished before installing and restarting. Ambient boards do not
block installation; editing does. Automatic installation never elevates privileges;
Settings offers manual installation when authorization is required. Debug, benchmark
and App Store builds keep their existing exclusions. The app flushes history through
its normal restart lifecycle.

Linux system/user installations include a daily systemd update timer with startup
checking and jitter. The updater verifies and stages before stopping collection,
tests the candidate privately, then commits before exposing history to controllers.
Interrupted trials restore the previous installation; committed updates never roll
history back. Update policy and results are visible through the agent CLI and Hosts.
The collector sandbox and service limits remain in effect.

Existing Macs need one update through Install and restart, and legacy Linux agents
need one Update agent action to acquire this capability. Hosts need outbound HTTPS
access. Incompatible releases wait for a coordinated manual upgrade. Do not run an
older controller's direct install scripts concurrently with the host-owned updater;
only updater-capable controllers share its transaction lock.

## Coordinated protocol upgrades

When a release drops a protocol that an installed host supports, Kelvo leaves it
pending. Repeating **Update agent** does not override this protection. Upgrade the
controllers first, and schedule a monitoring interruption for each affected host.

For a remote Mac, quit Kelvo and install the newer signed release from its DMG.
Its existing app data remains in place. Open it and confirm the host reconnects.

For Linux, use the updated controller's bundled agent through Add host:

1. On the host, disable and stop `kelvo-agent-update.timer`, stop
   `kelvo-agent-update.service` (allow its recovery to finish), then stop
   `kelvo-agent.service`. Use `systemctl --user` for a user installation or
   `systemctl` as root for a system installation.
2. Keep an offline backup of the entire data directory, installed executable and
   service units. System data is `/var/lib/kelvo-agent`; user data is normally
   `~/.local/state/kelvo-agent` and may follow `XDG_STATE_HOME`. Record the host
   identity, history epoch and configured paths before proceeding.
3. Run `kelvo-agent uninstall` in its installation account, with `--system` for a
   system installation. **Do not pass `--purge`.** Confirm that uninstallation
   succeeds and the data, settings and `host-id` remain. Keep the backup outside
   the installation's data directory.
4. Remove the saved connection from the controller with all removal options
   unchecked, keeping the mirrored history. Use Add host and install the newer bundled agent
   under the same account and scope. The installer reuses the retained data and
   identity. Confirm the identity, history, settings and connection before
   re-enabling the desired automatic-update preferences.

This explicit reinstall is outside the automatic transaction's rollback guarantee.
If the new daemon has exposed public history, never restore an older database under
the same history epoch: controllers may already hold newer cursors. Resolve a
post-start failure with a forward fix. Keep the offline backup for investigation;
do not silently overwrite the current database with it.
