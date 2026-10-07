//! SoC identity and DVFS tables (macmon `SocInfo`, `get_dvfs_mhz`, `parse_acc_clusters`,
//! `cpu_freq_scale`).
//!
//! The DVFS tables live as `voltage-statesN[-sram]` CFData properties on the `pmgr` (or,
//! from M6, `pmgr-child`) `AppleARMIODevice`: pairs of little-endian `u32` (frequency,
//! voltage). Which N belongs to which cluster is not documented; the keys below are
//! macmon's, verified by its users on M1 to M5. On the development M3 Max they give 6 E
//! states (1020 to 2568 MHz), 20 P states (1092 to 4056 MHz) and a GPU table topping out
//! at 1380 MHz, matching the residency state counts IOReport reports.

use std::ffi::CStr;

use super::{cf, iokit};

/// DVFS operating points in MHz, in table order (the order of IOReport's active states).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DvfsTables {
    pub ecpu_mhz: Vec<u32>,
    pub pcpu_mhz: Vec<u32>,
    /// The GPU table including its leading 0 MHz ("off") entry.
    pub gpu_mhz: Vec<u32>,
}

/// `machdep.cpu.brand_string`, for example "Apple M3 Max".
pub(crate) fn chip_name() -> Option<String> {
    sysctl_string(c"machdep.cpu.brand_string")
}

fn sysctl_string(name: &CStr) -> Option<String> {
    let mut buf = [0u8; 128];
    let mut len = buf.len();
    // SAFETY: `name` is NUL-terminated; `buf` has `len` writable bytes and the kernel
    // writes at most that many.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let bytes = buf.get(..len)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes.get(..end)?.to_vec()).ok()
}

/// Reads the DVFS tables from the IORegistry. `None` if no CPU table was found.
pub(crate) fn dvfs_tables(chip: &str) -> Option<DvfsTables> {
    let cpu_scale = cpu_freq_scale(chip);
    let mut t = DvfsTables::default();
    for entry in iokit::matching_services(c"AppleARMIODevice") {
        let Some(name) = entry.name() else { continue };
        // M1 to M5 keep the tables on "pmgr"; M6 moves them to "pmgr-child" and leaves
        // "pmgr" as a stub. The first node with a table wins.
        if name != "pmgr" && name != "pmgr-child" {
            continue;
        }
        let Some(props) = entry.properties() else {
            continue;
        };
        let acc = cf::get_bytes(&props, "acc-clusters").and_then(|b| parse_acc_clusters(&b));
        if t.ecpu_mhz.is_empty() {
            let key = acc
                .as_ref()
                .map_or("voltage-states1-sram", |(e, _)| e.as_str());
            t.ecpu_mhz = table(&props, "voltage-states1-sram", key, cpu_scale);
        }
        if t.pcpu_mhz.is_empty() {
            let key = acc
                .as_ref()
                .map_or("voltage-states5-sram", |(_, p)| p.as_str());
            t.pcpu_mhz = table(&props, "voltage-states5-sram", key, cpu_scale);
        }
        if t.gpu_mhz.is_empty() {
            t.gpu_mhz = cf::get_bytes(&props, "voltage-states9")
                .map(|b| freqs_mhz(&b, 1_000_000))
                .unwrap_or_default();
        }
    }
    (!t.ecpu_mhz.is_empty() || !t.pcpu_mhz.is_empty()).then_some(t)
}

/// The table under `known` (M1 to M4), else under `discovered` (from `acc-clusters`, M5+).
fn table(props: &cf::Dict, known: &str, discovered: &str, scale: u32) -> Vec<u32> {
    cf::get_bytes(props, known)
        .or_else(|| cf::get_bytes(props, discovered))
        .map(|b| freqs_mhz(&b, scale))
        .unwrap_or_default()
}

/// Frequencies from a `voltage-states` blob of `(freq u32 LE, voltage u32 LE)` pairs,
/// divided by `scale` (Hz or kHz to MHz).
pub(crate) fn freqs_mhz(blob: &[u8], scale: u32) -> Vec<u32> {
    let (pairs, _) = blob.as_chunks::<8>();
    pairs
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]) / scale.max(1))
        .collect()
}

/// M1 to M3 (and A-series) tables are in Hz, M4 and later in kHz.
pub(crate) fn cpu_freq_scale(chip: &str) -> u32 {
    let hz = ["M1", "M2", "M3", "A1"].iter().any(|g| chip.contains(g));
    if hz { 1_000_000 } else { 1_000 }
}

/// `acc-clusters`: 8-byte entries, byte 0 the voltage-states index, byte 1 the cluster
/// tier (0 lowest). Returns the `(lower tier, highest tier)` table keys. M5 Max has no
/// tier 0, so the two highest tiers are used.
pub(crate) fn parse_acc_clusters(data: &[u8]) -> Option<(String, String)> {
    let (entries, _) = data.as_chunks::<8>();
    let mut clusters: Vec<(u8, u8)> = entries.iter().map(|c| (c[1], c[0])).collect();
    clusters.sort_by_key(|c| c.0);
    let n = clusters.len();
    if n < 2 {
        return None;
    }
    let lo = clusters.get(n - 2)?.1;
    let hi = clusters.get(n - 1)?.1;
    Some((
        format!("voltage-states{lo}-sram"),
        format!("voltage-states{hi}-sram"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acc_clusters_from_m5_max() {
        // Captured from an M5 Max via ioreg by macmon (unverified here).
        #[rustfmt::skip]
        let data = [
            0x16, 0x00, 0, 0, 0, 0, 0, 0,
            0x17, 0x01, 0, 0, 0, 0, 0, 0,
            0x05, 0x02, 0, 0, 0, 0, 0, 0,
        ];
        let (lo, hi) = parse_acc_clusters(&data).unwrap();
        assert_eq!(lo, "voltage-states23-sram");
        assert_eq!(hi, "voltage-states5-sram");
        assert!(parse_acc_clusters(&[]).is_none());
        assert!(parse_acc_clusters(&[1, 0, 0, 0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn dvfs_blob_scales() {
        let mut blob = Vec::new();
        for (f, v) in [(1_020_000_000u32, 600u32), (2_568_000_000, 900)] {
            blob.extend_from_slice(&f.to_le_bytes());
            blob.extend_from_slice(&v.to_le_bytes());
        }
        blob.push(0xff); // trailing partial entry is ignored
        assert_eq!(
            freqs_mhz(&blob, cpu_freq_scale("Apple M3 Max")),
            [1020, 2568]
        );
        assert_eq!(cpu_freq_scale("Apple M4 Pro"), 1_000);
    }
}
