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
   and run `cargo run -p kelvo --example verify_release -- target/release-assets`.
   Create the draft with `gh release create TAG target/release-assets/* --verify-tag
   --draft --title "Kelvo TAG" --notes-file RELEASE_NOTES.md`. It must contain
   `Kelvo_aarch64.dmg`, `Kelvo.app.tar.gz`, its `.sig`, `latest.json` and `SHA256SUMS`.
4. Download the draft DMG. Verify the signature with `codesign --verify --deep
   --strict Kelvo.app`, install and launch it on a fresh account/second Mac, and
   check a real update from the previous release when one exists. Confirm history
   and settings survive and note any Gatekeeper prompts. Browser tests cannot
   establish native installation or restart behavior.
5. Publish the complete draft as the latest stable release. Only then does the
   fixed `/releases/latest/download/latest.json` endpoint advertise it to clients.

Never replace published updater archives or reuse a version. Fix a bad release
with a higher version. A failed draft can be deleted and rebuilt from its existing
tag using the local build procedure. Do not move published tags.

## Local release build

With the signing key stored at the path above:

```sh
TAURI_SIGNING_PRIVATE_KEY="$HOME/.config/kelvo/release/updater.key" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD='' \
bun run build:app --config src-tauri/tauri.release.conf.json --bundles app,dmg
python3 scripts/release-manifest.py v0.2.0 --stage
```

Use the actual version tag. `target/release-assets` must be empty before staging;
move a previous deliverable aside rather than mixing files between releases.
Ordinary local builds do not create signed updater archives and need no private key.

## Update behavior

Rust owns a single in-memory update candidate and serializes checks/installations
across windows. Explicit checks work with automatic checks disabled. Background
checks run 30 seconds after launch and every 24 hours, only with completed
onboarding and the update preference enabled. Debug, benchmark and App Store
builds do not run automatic checks; debug/App Store builds cannot self-update.

Installation requires approval of the displayed version, verifies the downloaded
archive's signature and version, then installs off the async executor. Tauri's
restart lifecycle flushes history and shuts down remote services. Errors are
visible in Settings; the user can check again or replace the app using a DMG.
No background download, silent installation, telemetry or paid service is added.
