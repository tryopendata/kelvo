# Kelvo updater patch

Source: the crates.io `tauri-plugin-updater` 2.14.0 package. Keep the upstream
licenses and source intact apart from the change below.

`Update::allow_elevation` defaults to `true`. Kelvo sets it to `false` for unattended
installation. If macOS bundle replacement needs administrator authorization, the
installer preserves recovery material where necessary and returns an I/O
permission-denied error before starting AppleScript. Explicit installation retains
the existing authorization behavior. Package and signed-version verification are
unchanged.

The added native test revokes installation-directory permissions after preparation
and checks that installation returns without invoking the authorization callback.
It also checks that the installed bundle is unchanged. Removing the elevation guard
makes the test fail at the callback.

Run the vendored tests separately because this dependency is excluded from the
Kelvo workspace:

```sh
cargo test --manifest-path vendor/tauri-plugin-updater/Cargo.toml --lib
```

When upgrading upstream, compare this patch with the new installer and rerun both
these tests and Kelvo's updater tests. Do not patch the Cargo registry cache.
