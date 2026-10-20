//! Bounds on series a host can create through label values it does not control (D-116):
//! a cap on series per metric, and a length bound on label values such as container names.

use std::borrow::Cow;
use std::fmt::Write;

use crate::catalog::{Catalog, MetricDef};
use crate::series::SeriesKey;

/// The longest label value a key keeps inline: `CompactString` stores up to 24 bytes
/// without a heap allocation on 64-bit targets, so cloning a key in `sample` stays free.
pub const LABEL_VALUE_MAX: usize = 24;

/// Hex digits of the hash a bounded value ends with.
const HASH_HEX: usize = 8;

impl Catalog {
    /// The positions in `keys` beyond their metric's [`MetricDef::max_cardinality`].
    /// Walking `keys` in order, the first `n` distinct series of a capped metric stay and
    /// every later one is over, so the result depends only on the order given. A repeat
    /// of a kept series stays; metrics without a cap, or unknown to this build, are never
    /// over. Ascending, and empty (no allocation) when every metric is within its cap.
    pub fn over_cap(&self, keys: &[SeriesKey]) -> Vec<usize> {
        let mut kept: Vec<&SeriesKey> = Vec::new();
        let mut over = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            let Some(cap) = self
                .get(key.metric.as_str())
                .and_then(|d: &MetricDef| d.max_cardinality)
            else {
                continue;
            };
            if kept.contains(&key) {
                continue;
            }
            let n = kept.iter().filter(|k| k.metric == key.metric).count();
            if n < usize::from(cap) {
                kept.push(key);
            } else {
                over.push(i);
            }
        }
        over
    }
}

/// `value` bounded to [`LABEL_VALUE_MAX`] bytes, for label values a user names
/// (container names). A longer value keeps as much of its start as fits (cut on a char
/// boundary), then `~` and the 32-bit FNV-1a hash of the whole value in hex, so two long
/// names that share a prefix stay distinct and a name always maps to the same value
/// across runs and builds. Values that fit are returned as they are.
pub fn bound_label_value(value: &str) -> Cow<'_, str> {
    if value.len() <= LABEL_VALUE_MAX {
        return Cow::Borrowed(value);
    }
    let mut end = LABEL_VALUE_MAX - 1 - HASH_HEX;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(LABEL_VALUE_MAX);
    out.push_str(value.get(..end).unwrap_or_default());
    // Writing to a String cannot fail.
    let _ = write!(out, "~{:08x}", fnv1a32(value.as_bytes()));
    Cow::Owned(out)
}

/// 32-bit FNV-1a: stable across Rust releases, unlike `DefaultHasher`, which matters
/// because the result is stored and synced as part of a series key.
fn fnv1a32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5, |h: u32, &b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Module;
    use crate::series::{Labels, MetricId};
    use crate::{MetricKind, Unit};

    static DEFS: &[MetricDef] = &[
        MetricDef {
            id: MetricId::from_static("cgroup.mem"),
            unit: Unit::Bytes,
            kind: MetricKind::Gauge,
            label_keys: &["kind", "name"],
            module: Module::Containers,
            persisted: true,
            max_cardinality: Some(2),
            period_s: 5,
        },
        MetricDef {
            id: MetricId::from_static("cpu.load"),
            unit: Unit::Percent,
            kind: MetricKind::Mean,
            label_keys: &["core"],
            module: Module::Cpu,
            persisted: true,
            max_cardinality: None,
            period_s: 1,
        },
    ];

    fn mem(name: &str) -> SeriesKey {
        SeriesKey::new(
            MetricId::from_static("cgroup.mem"),
            Labels::from_pairs([("kind", "docker"), ("name", name)]).unwrap(),
        )
    }

    fn core(n: &str) -> SeriesKey {
        SeriesKey::new(MetricId::from_static("cpu.load"), Labels::single("core", n))
    }

    #[test]
    fn keeps_the_first_n_distinct_series_of_a_capped_metric() {
        let catalog = Catalog::new(DEFS);
        let keys = [
            mem("c"),
            core("P0"),
            mem("a"),
            mem("c"), // a repeat of a kept series
            mem("b"),
            core("P1"),
            mem("d"),
        ];
        assert_eq!(catalog.over_cap(&keys), [4, 6]);
        // The order decides, not the names.
        let keys = [mem("b"), mem("a"), mem("c")];
        assert_eq!(catalog.over_cap(&keys), [2]);
    }

    #[test]
    fn uncapped_and_unknown_metrics_are_never_over() {
        let catalog = Catalog::new(DEFS);
        let keys: Vec<SeriesKey> = (0..100).map(|i| core(&format!("P{i}"))).collect();
        assert!(catalog.over_cap(&keys).is_empty());
        let unknown = SeriesKey::bare(MetricId::from_static("cgroup.future"));
        assert!(catalog.over_cap(&[unknown.clone(), unknown]).is_empty());
    }

    #[test]
    fn the_builtin_catalog_caps_cgroup_series_at_32() {
        let catalog = Catalog::builtin();
        let keys: Vec<SeriesKey> = (0..40)
            .map(|i| {
                SeriesKey::new(
                    MetricId::from_static("cgroup.cpu"),
                    Labels::from_pairs([("kind", "docker"), ("name", format!("c{i}").as_str())])
                        .unwrap(),
                )
            })
            .collect();
        assert_eq!(catalog.over_cap(&keys), (32..40).collect::<Vec<_>>());
    }

    #[test]
    fn short_label_values_pass_through() {
        assert!(matches!(
            bound_label_value("postgres"),
            Cow::Borrowed("postgres")
        ));
        let exact = "a".repeat(LABEL_VALUE_MAX);
        assert_eq!(bound_label_value(&exact), exact);
    }

    #[test]
    fn long_label_values_are_bounded_stable_and_distinct() {
        let a = "my-compose-project-postgres-primary-1";
        let b = "my-compose-project-postgres-replica-1";
        let (ba, bb) = (bound_label_value(a), bound_label_value(b));
        assert!(ba.len() <= LABEL_VALUE_MAX && bb.len() <= LABEL_VALUE_MAX);
        assert_ne!(ba, bb, "a shared prefix must not merge two names");
        assert!(ba.starts_with("my-compose-proj~"), "{ba}");
        // Pinned: stored keys must not change between builds.
        assert_eq!(ba, format!("my-compose-proj~{:08x}", fnv1a32(a.as_bytes())));
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        assert_eq!(bound_label_value(a), ba, "deterministic");
    }

    #[test]
    fn long_label_values_cut_on_a_char_boundary() {
        let name = "ééééééééééééééééé"; // 2-byte chars, 34 bytes
        let bounded = bound_label_value(name);
        assert!(bounded.len() <= LABEL_VALUE_MAX);
        assert!(bounded.starts_with("ééééééé~"), "{bounded}");
        // Stays inline when a key holding it is built.
        let key = SeriesKey::new(
            MetricId::from_static("cgroup.mem"),
            Labels::from_pairs([("kind", "docker"), ("name", bounded.as_ref())]).unwrap(),
        );
        assert_eq!(key.labels.get("name"), Some(bounded.as_ref()));
    }
}
