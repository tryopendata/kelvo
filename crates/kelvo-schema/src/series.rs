//! Series identity: [`MetricId`], [`Labels`], [`SeriesKey`] and [`SeriesSelector`].
//!
//! A series is a metric plus a label set, for example `cpu.load{core=P7}` or
//! `disk.read{dev=disk3}`. Storage and sync work on series, never on typed structs
//! (architecture.md, infra 1). This is a one-way door: the display form below is what the
//! store writes into `series.labels` and what travels over the wire.
//!
//! # Canonical text form
//!
//! - [`Labels::canonical`]: `key=value` pairs sorted by key, joined by `,`. The empty set
//!   is the empty string. This is the `series.labels` column in the store.
//! - [`SeriesKey`]'s `Display`: `metric{labels}`, or just `metric` when there are no
//!   labels. [`SeriesKey::parse`] reads it back.
//! - Inside keys and values, the characters `\ , = { }` are escaped with a backslash, so a
//!   raw HID sensor name such as `PMU tdie4` or one containing a comma still round-trips.

use std::borrow::Cow;
use std::fmt;

use compact_str::CompactString;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use smallvec::SmallVec;

/// Dotted, stable, lowercase metric name such as `cpu.load` or `power.cpu`. Never reused
/// for a different meaning; renaming one is a decision entry.
///
/// Allowed characters are `a-z`, `0-9`, `_` and `.`, with no empty segment. Construct
/// known IDs with [`MetricId::from_static`] and untrusted ones with [`MetricId::parse`].
/// Deserialization does not validate: a malformed ID from a peer is simply an unknown
/// metric, and unknown metrics are ignored by every receiver.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct MetricId(pub Cow<'static, str>);

impl MetricId {
    /// A metric ID from a string literal. Used by the catalog; not validated at compile
    /// time, but the catalog test validates every entry.
    pub const fn from_static(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    /// Parses and validates a metric ID.
    pub fn parse(s: &str) -> Result<Self, SeriesParseError> {
        if is_valid_metric_id(s) {
            Ok(Self(Cow::Owned(s.to_owned())))
        } else {
            Err(SeriesParseError::InvalidMetricId(s.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the ID follows the naming rule (see the type docs).
    pub fn is_valid(&self) -> bool {
        is_valid_metric_id(&self.0)
    }
}

impl fmt::Display for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_valid_metric_id(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

type LabelVec = SmallVec<[(CompactString, CompactString); 2]>;

/// A label set: small, sorted by key, keys unique. The invariant is enforced by every
/// constructor and by deserialization, so two equal label sets always compare, hash and
/// display the same.
///
/// Serialized (CBOR and JSON) as an array of `[key, value]` pairs in key order. Labels
/// that arrive out of order are sorted on decode; duplicate keys are a decode error.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, specta::Type)]
pub struct Labels(#[specta(type = Vec<(String, String)>)] LabelVec);

impl Labels {
    /// The empty label set.
    pub fn new() -> Self {
        Self::default()
    }

    /// A label set with one pair, the common case (`core=7`).
    pub fn single(key: &str, value: &str) -> Self {
        let mut v = LabelVec::new();
        v.push((key.into(), value.into()));
        Self(v)
    }

    /// Builds a label set from pairs in any order. Duplicate keys are an error.
    pub fn from_pairs<K, V, I>(pairs: I) -> Result<Self, SeriesParseError>
    where
        K: AsRef<str>,
        V: AsRef<str>,
        I: IntoIterator<Item = (K, V)>,
    {
        let v: LabelVec = pairs
            .into_iter()
            .map(|(k, v)| {
                (
                    CompactString::from(k.as_ref()),
                    CompactString::from(v.as_ref()),
                )
            })
            .collect();
        Self::canonicalize(v)
    }

    fn canonicalize(mut v: LabelVec) -> Result<Self, SeriesParseError> {
        v.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(w) = v.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(SeriesParseError::DuplicateLabel(w[0].0.to_string()));
        }
        Ok(Self(v))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .binary_search_by(|(k, _)| k.as_str().cmp(key))
            .ok()
            .and_then(|i| self.0.get(i))
            .map(|(_, v)| v.as_str())
    }

    /// Pairs in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Keys in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    /// True when every pair in `self` also appears in `other`. The empty set is a subset
    /// of everything, which is how an empty selector means "any".
    pub fn is_subset_of(&self, other: &Labels) -> bool {
        self.iter().all(|(k, v)| other.get(k) == Some(v))
    }

    /// The canonical text form (`core=7`, `cluster=P0,state=idle`, or `""`). This is what
    /// the store writes to `series.labels`; [`Labels::parse_canonical`] reads it back.
    pub fn canonical(&self) -> String {
        let mut s = String::new();
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            push_escaped(&mut s, k);
            s.push('=');
            push_escaped(&mut s, v);
        }
        s
    }

    /// Parses the canonical form. Accepts pairs in any order and sorts them.
    pub fn parse_canonical(s: &str) -> Result<Self, SeriesParseError> {
        if s.is_empty() {
            return Ok(Self::new());
        }
        let mut pairs = LabelVec::new();
        for part in split_unescaped(s, ',') {
            let mut kv = split_unescaped(part, '=');
            let (Some(k), Some(v), None) = (kv.next(), kv.next(), kv.next()) else {
                return Err(SeriesParseError::MalformedLabels(s.to_owned()));
            };
            let k = unescape(k).ok_or_else(|| SeriesParseError::MalformedLabels(s.to_owned()))?;
            let v = unescape(v).ok_or_else(|| SeriesParseError::MalformedLabels(s.to_owned()))?;
            if k.is_empty() {
                return Err(SeriesParseError::MalformedLabels(s.to_owned()));
            }
            pairs.push((k.into(), v.into()));
        }
        Self::canonicalize(pairs)
    }
}

impl Serialize for Labels {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    }
}

impl<'de> Deserialize<'de> for Labels {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pairs = Vec::<(String, String)>::deserialize(deserializer)?;
        let pairs = pairs
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        Self::canonicalize(pairs).map_err(serde::de::Error::custom)
    }
}

const ESCAPED: [char; 5] = ['\\', ',', '=', '{', '}'];

fn push_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        if ESCAPED.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
}

/// Splits on `sep` where it is not preceded by an escaping backslash.
fn split_unescaped(s: &str, sep: char) -> impl Iterator<Item = &str> {
    let mut rest = Some(s);
    std::iter::from_fn(move || {
        let cur = rest?;
        let mut escaped = false;
        for (i, c) in cur.char_indices() {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == sep {
                rest = cur.get(i + c.len_utf8()..);
                return cur.get(..i);
            }
        }
        rest = None;
        Some(cur)
    })
}

/// Removes escapes. `None` for a dangling backslash or an unescaped special character.
fn unescape(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let next = chars.next()?;
            if !ESCAPED.contains(&next) {
                return None;
            }
            out.push(next);
        } else if ESCAPED.contains(&c) {
            return None;
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// One series: a metric plus its labels. Display form `cpu.load{core=7}`.
///
/// On the wire and over IPC it is a struct `{ metric, labels }`; keys always travel as
/// strings, never as a store's interned IDs.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
pub struct SeriesKey {
    pub metric: MetricId,
    pub labels: Labels,
}

impl SeriesKey {
    pub fn new(metric: MetricId, labels: Labels) -> Self {
        Self { metric, labels }
    }

    /// A key with no labels.
    pub fn bare(metric: MetricId) -> Self {
        Self {
            metric,
            labels: Labels::new(),
        }
    }

    /// Parses the display form, `metric` or `metric{k=v,...}`.
    pub fn parse(s: &str) -> Result<Self, SeriesParseError> {
        let (metric, labels) = match s.find('{') {
            None => (s, Labels::new()),
            Some(open) => {
                let inner = s
                    .get(open + 1..)
                    .and_then(|r| r.strip_suffix('}'))
                    .ok_or_else(|| SeriesParseError::MalformedKey(s.to_owned()))?;
                let metric = s.get(..open).unwrap_or_default();
                let labels = Labels::parse_canonical(inner)?;
                if labels.is_empty() {
                    // `cpu.total{}` is not canonical; the bare form is.
                    return Err(SeriesParseError::MalformedKey(s.to_owned()));
                }
                (metric, labels)
            }
        };
        Ok(Self {
            metric: MetricId::parse(metric)?,
            labels,
        })
    }
}

impl fmt::Display for SeriesKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.metric.as_str())?;
        if !self.labels.is_empty() {
            write!(f, "{{{}}}", self.labels.canonical())?;
        }
        Ok(())
    }
}

/// Picks series by metric and a label subset. Used by alert rules and history queries.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct SeriesSelector {
    pub metric: MetricId,
    /// Empty matches any labels. Otherwise every pair must appear in the series' labels.
    pub labels: Labels,
}

impl SeriesSelector {
    pub fn matches(&self, key: &SeriesKey) -> bool {
        self.metric == key.metric && self.labels.is_subset_of(&key.labels)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SeriesParseError {
    #[error("invalid metric id {0:?}: expected dotted lowercase [a-z0-9_]")]
    InvalidMetricId(String),
    #[error("duplicate label key {0:?}")]
    DuplicateLabel(String),
    #[error("malformed labels {0:?}")]
    MalformedLabels(String),
    #[error("malformed series key {0:?}")]
    MalformedKey(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> SeriesKey {
        SeriesKey::parse(s).unwrap()
    }

    #[test]
    fn display_parse_round_trip() {
        for s in [
            "cpu.total",
            "cpu.load{core=P7}",
            "cpu.cluster.residency{cluster=P0,state=idle}",
            "thermal.zone{sensor=PMU tdie4}",
            r"thermal.zone{sensor=odd\,name\=x\{y\}\\z}",
            "disk.used{vol=/}",
        ] {
            let k = key(s);
            assert_eq!(k.to_string(), s, "round trip of {s}");
            assert_eq!(SeriesKey::parse(&k.to_string()).unwrap(), k);
        }
    }

    #[test]
    fn escaped_value_survives() {
        let k = key(r"thermal.zone{sensor=a\,b}");
        assert_eq!(k.labels.get("sensor"), Some("a,b"));
    }

    #[test]
    fn labels_sort_into_canonical_form() {
        let k = key("cpu.cluster.residency{state=idle,cluster=P0}");
        assert_eq!(
            k.to_string(),
            "cpu.cluster.residency{cluster=P0,state=idle}"
        );
        let a = Labels::from_pairs([("b", "2"), ("a", "1")]).unwrap();
        let b = Labels::from_pairs([("a", "1"), ("b", "2")]).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.canonical(), "a=1,b=2");
    }

    #[test]
    fn rejects_bad_input() {
        for s in [
            "",
            "CPU.total",
            "cpu..total",
            ".cpu",
            "cpu.load{",
            "cpu.load{}",
            "cpu.load{core}",
            "cpu.load{core=1,core=2}",
            "cpu.load{=1}",
            "cpu.load{core=a=b}",
            r"cpu.load{core=a\}",
            "cpu-load",
        ] {
            assert!(SeriesKey::parse(s).is_err(), "{s:?} should not parse");
        }
    }

    #[test]
    fn duplicate_labels_rejected_on_decode() {
        let json = r#"{"metric":"cpu.load","labels":[["core","1"],["core","2"]]}"#;
        assert!(serde_json::from_str::<SeriesKey>(json).is_err());
    }

    #[test]
    fn unsorted_labels_sorted_on_decode() {
        let json = r#"{"metric":"x.y","labels":[["b","2"],["a","1"]]}"#;
        let k: SeriesKey = serde_json::from_str(json).unwrap();
        assert_eq!(k.to_string(), "x.y{a=1,b=2}");
    }

    #[test]
    fn json_shape() {
        let k = key("cpu.load{core=7}");
        assert_eq!(
            serde_json::to_string(&k).unwrap(),
            r#"{"metric":"cpu.load","labels":[["core","7"]]}"#
        );
    }

    #[test]
    fn cbor_round_trip() {
        let k = key("cpu.cluster.residency{cluster=P0,state=1260}");
        let mut buf = Vec::new();
        ciborium::into_writer(&k, &mut buf).unwrap();
        let back: SeriesKey = ciborium::from_reader(buf.as_slice()).unwrap();
        assert_eq!(back, k);
    }

    #[test]
    fn selector_matches_subset() {
        let any_core = SeriesSelector {
            metric: MetricId::from_static("cpu.load"),
            labels: Labels::new(),
        };
        let core7 = SeriesSelector {
            metric: MetricId::from_static("cpu.load"),
            labels: Labels::single("core", "7"),
        };
        assert!(any_core.matches(&key("cpu.load{core=7}")));
        assert!(core7.matches(&key("cpu.load{core=7}")));
        assert!(!core7.matches(&key("cpu.load{core=8}")));
        assert!(!core7.matches(&key("cpu.total")));
        assert!(!any_core.matches(&key("cpu.total")));
    }
}
