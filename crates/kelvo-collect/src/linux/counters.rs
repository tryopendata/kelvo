//! Small helpers the counter-delta collectors (disk, network) share.

/// A total over a set of reported parts on one sample (D-092): their sum, or a gap when
/// any part's own value is a gap.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PartSum {
    sum: f64,
    gap: bool,
}

impl PartSum {
    /// Adds one reported part's value for this sample; `None` is that part's gap.
    pub(super) fn add(&mut self, part: Option<f32>) {
        match part {
            Some(v) => self.sum += f64::from(v),
            None => self.gap = true,
        }
    }

    /// The total, `None` when any part was a gap.
    pub(super) fn total(self) -> Option<f32> {
        (!self.gap).then_some(self.sum as f32)
    }
}

/// Seconds between two continuous-clock readings, `None` without a previous one.
pub(super) fn span_secs(prev_ns: u64, now_ns: u64) -> Option<f64> {
    (prev_ns > 0 && now_ns > prev_ns).then(|| (now_ns - prev_ns) as f64 / 1e9)
}

/// Parses an unsigned decimal from a short sysfs/procfs value, surrounding whitespace
/// allowed. Never allocates.
pub(super) fn parse_u64(bytes: &[u8]) -> Option<u64> {
    std::str::from_utf8(bytes).ok()?.trim().parse().ok()
}

/// Reads a small file (a sysfs attribute) into `buf` without allocating and parses it as
/// an unsigned integer. `None` when unreadable (an interface gone, `speed` on a link that
/// is down) or not a number (`-1`).
pub(super) fn read_u64_at(path: &std::path::Path) -> Option<u64> {
    use std::io::Read;
    let mut buf = [0u8; 32];
    let mut f = std::fs::File::open(path).ok()?;
    let n = f.read(&mut buf).ok()?;
    parse_u64(buf.get(..n)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_sum_gaps_on_any_missing_part() {
        let mut s = PartSum::default();
        s.add(Some(1.0));
        s.add(Some(2.5));
        assert_eq!(s.total(), Some(3.5));
        s.add(None);
        assert_eq!(s.total(), None);
        assert_eq!(PartSum::default().total(), Some(0.0));
    }

    #[test]
    fn parse_u64_rejects_negative_and_garbage() {
        assert_eq!(parse_u64(b"1000\n"), Some(1000));
        assert_eq!(parse_u64(b"-1\n"), None);
        assert_eq!(parse_u64(b""), None);
        assert_eq!(span_secs(0, 5), None);
        assert_eq!(span_secs(1_000_000_000, 3_000_000_000), Some(2.0));
    }
}
