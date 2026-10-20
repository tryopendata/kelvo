//! The machine identifiers a Linux host's id is bound to (D-071, D-116): systemd's
//! `/etc/machine-id`, plus the firmware's DMI `product_uuid` or the device-tree serial
//! when a system agent asks for them. The engine hashes them (`kelvo_engine::identity`);
//! this module only reads and validates.
//!
//! Plain file reads under an injectable root, so they compile and test on every OS;
//! only a Linux build calls them for real.

use std::path::Path;

/// What a binding is made from. `machine_id` is always present: without a usable one
/// there is no binding at all, and the `host-id` file is trusted as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineIds {
    /// `/etc/machine-id`, 32 lowercase hex digits.
    pub machine_id: String,
    /// DMI `product_uuid` or, without DMI, `/proc/device-tree/serial-number`. Only read
    /// when asked for, and `None` when neither is readable (both are usually root-only
    /// or absent).
    pub hardware: Option<String>,
}

/// Reads the identifiers under `root` (`/` on a real host). `None` when the machine-id
/// is missing, empty, `uninitialized`, malformed or transient (systemd mounted a
/// `/run/machine-id` over it because `/etc` was read-only, so it changes every boot).
/// `hardware` adds the firmware identifier: a hypervisor clone keeps the machine-id but
/// gets a new VM UUID.
pub fn machine_ids(root: &Path, hardware: bool) -> Option<MachineIds> {
    let machine_id = read_trimmed(&root.join("etc/machine-id"))?;
    let valid = machine_id.len() == 32
        && machine_id.bytes().all(|b| b.is_ascii_hexdigit())
        && machine_id.bytes().any(|b| b != b'0');
    if !valid {
        return None;
    }
    if read_trimmed(&root.join("run/machine-id")).is_some_and(|run| run == machine_id) {
        return None;
    }
    let hardware = hardware
        .then(|| {
            read_hardware(&root.join("sys/class/dmi/id/product_uuid"))
                .or_else(|| read_hardware(&root.join("proc/device-tree/serial-number")))
        })
        .flatten();
    Some(MachineIds {
        machine_id: machine_id.to_ascii_lowercase(),
        hardware,
    })
}

/// A file's text without surrounding whitespace or the device tree's trailing NULs;
/// `None` when unreadable or empty.
fn read_trimmed(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    (!text.is_empty()).then(|| text.to_owned())
}

/// A firmware identifier, skipping the placeholders firmware ships when nothing was
/// programmed (all zeros or all `f`).
fn read_hardware(path: &Path) -> Option<String> {
    let text = read_trimmed(path)?;
    let placeholder = text.chars().all(|c| matches!(c, '0' | 'f' | 'F' | '-'));
    (!placeholder).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "4c1d3f0e9a8b47c6b5d2e1f0a9b8c7d6";

    fn root(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::Builder::new()
            .prefix("kelvo-collect-machine-")
            .tempdir()
            .unwrap();
        for (path, text) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    #[test]
    fn reads_the_machine_id() {
        let r = root(&[("etc/machine-id", &format!("{ID}\n"))]);
        assert_eq!(
            machine_ids(r.path(), false),
            Some(MachineIds {
                machine_id: ID.into(),
                hardware: None
            })
        );
    }

    #[test]
    fn unusable_machine_ids_give_no_binding() {
        for text in [
            "",
            "\n",
            "uninitialized\n",
            "not-hex",
            "0000",
            &"0".repeat(32),
        ] {
            let r = root(&[("etc/machine-id", text)]);
            assert_eq!(machine_ids(r.path(), true), None, "{text:?}");
        }
        assert_eq!(machine_ids(root(&[]).path(), false), None, "missing");
    }

    #[test]
    fn a_transient_machine_id_gives_no_binding() {
        let r = root(&[
            ("etc/machine-id", ID),
            ("run/machine-id", &format!("{ID}\n")),
        ]);
        assert_eq!(machine_ids(r.path(), false), None);
        // A different /run/machine-id does not make /etc's transient.
        let r = root(&[("etc/machine-id", ID), ("run/machine-id", "x")]);
        assert!(machine_ids(r.path(), false).is_some());
    }

    #[test]
    fn hardware_ids_only_when_asked_and_readable() {
        let dmi = "6E0A1B2C-3D4E-5F60-7182-93A4B5C6D7E8";
        let r = root(&[
            ("etc/machine-id", ID),
            ("sys/class/dmi/id/product_uuid", &format!("{dmi}\n")),
            ("proc/device-tree/serial-number", "C07XYZ\0"),
        ]);
        assert_eq!(machine_ids(r.path(), false).unwrap().hardware, None);
        assert_eq!(
            machine_ids(r.path(), true).unwrap().hardware.as_deref(),
            Some(dmi)
        );

        // No DMI (an Apple Silicon board): the device-tree serial, without its NUL.
        let r = root(&[
            ("etc/machine-id", ID),
            ("proc/device-tree/serial-number", "C07XYZ\0"),
        ]);
        assert_eq!(
            machine_ids(r.path(), true).unwrap().hardware.as_deref(),
            Some("C07XYZ")
        );

        // Firmware placeholders and nothing readable bind on the machine-id alone.
        let r = root(&[
            ("etc/machine-id", ID),
            (
                "sys/class/dmi/id/product_uuid",
                "00000000-0000-0000-0000-000000000000",
            ),
        ]);
        assert_eq!(machine_ids(r.path(), true).unwrap().hardware, None);
        let r = root(&[("etc/machine-id", ID)]);
        assert_eq!(machine_ids(r.path(), true).unwrap().hardware, None);
    }
}
