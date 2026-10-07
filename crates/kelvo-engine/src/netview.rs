//! Network-by-app view shaping (D-089): a range's per-app bytes from the store and the
//! ring ([`kelvo_store::NetByApp`]) split into named apps, "other apps", estimated
//! protocol overhead and the "System and other" remainder. Lives next to
//! [`crate::select_processes`] so the app shell and a headless agent (v4) split a range
//! the same way; the shell maps [`NetAttribution`] to its IPC type.

use kelvo_store::{NetByApp, NetSpan};

/// Header bytes per interface packet that no app's byte count includes: Ethernet (14 B),
/// IPv4 (20 B) and TCP with the timestamp option (32 B). NetworkStatistics counts TCP
/// payload, interface counters count frames, and interface packet counts are exact, so
/// packets times this estimates the gap (D-089). The step-0 probe measured interface
/// bytes over NStat bytes at 1.0465 for an 80 MB download against 1.0456 for this size.
/// IPv6 (40 B IP header), UDP (8 B) and Wi-Fi framing make it wrong in either direction,
/// which is why the UI labels it "est.".
pub const HEADER_BYTES_PER_PACKET: u64 = 66;

/// How far the apps may exceed the interface total before the range is flagged as
/// [`NetAttribution::clamped`]. Unprivileged interface byte counters are 1 KiB-granular
/// (D-089) and buckets are split pro rata, so a quiet range can read a few KiB under
/// its apps without any double count.
pub const CLAMP_SLACK_BYTES: u64 = 8 * 1024;

/// One direction of a range: the interface total and how it splits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectionSplit {
    pub iface_bytes: u64,
    /// Bytes of named apps plus "other apps".
    pub attributed_bytes: u64,
    /// `packets * HEADER_BYTES_PER_PACKET`, at most what the interface total leaves after
    /// the apps.
    pub overhead_bytes: u64,
    /// Interface minus apps minus overhead, never below 0: root daemons and anything else
    /// NetworkStatistics does not show the user.
    pub system_bytes: u64,
    /// The apps exceed the interface total by more than [`CLAMP_SLACK_BYTES`]: late bytes
    /// added to a later bucket than the interface counted them in, or history written
    /// before per-app bytes counted only the reported interfaces (tunnels, bridges and
    /// loopback were counted by the app but not the interface).
    pub clamped: bool,
}

/// Splits one direction. Apps come first: what they moved is measured, the overhead is
/// an estimate, so when apps plus the estimate exceed the interface the estimate shrinks.
/// Outside a clamp `attributed + overhead + system == iface` exactly.
pub fn split_direction(iface_bytes: u64, iface_pkts: u64, attributed_bytes: u64) -> DirectionSplit {
    let room = iface_bytes.saturating_sub(attributed_bytes);
    let overhead_bytes = iface_pkts.saturating_mul(HEADER_BYTES_PER_PACKET).min(room);
    DirectionSplit {
        iface_bytes,
        attributed_bytes,
        overhead_bytes,
        system_bytes: room - overhead_bytes,
        clamped: attributed_bytes > iface_bytes.saturating_add(CLAMP_SLACK_BYTES),
    }
}

/// One named app's bytes over a range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppBytes {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

/// A range's bytes, split for the Network page's Apps table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetAttribution {
    pub from_ms: i64,
    pub to_ms: i64,
    pub resolution_ms: i64,
    pub measured_ms: u64,
    pub coverage: Vec<NetSpan>,
    /// Named apps, largest rx + tx first; "other apps" is not among them.
    pub apps: Vec<AppBytes>,
    /// Apps below a stored bucket's top 20, nameless bytes and names past the cap.
    pub other_apps_rx_bytes: u64,
    pub other_apps_tx_bytes: u64,
    pub rx: DirectionSplit,
    pub tx: DirectionSplit,
}

impl NetAttribution {
    /// Either direction hit the clamp.
    pub fn clamped(&self) -> bool {
        self.rx.clamped || self.tx.clamped
    }
}

/// Splits a store read into apps, "other apps", overhead and the remainder.
pub fn attribute(n: NetByApp) -> NetAttribution {
    let (mut other_rx, mut other_tx) = (0u64, 0u64);
    let (mut apps_rx, mut apps_tx) = (0u64, 0u64);
    let mut apps = Vec::with_capacity(n.apps.len());
    for a in n.apps {
        apps_rx = apps_rx.saturating_add(a.rx_bytes);
        apps_tx = apps_tx.saturating_add(a.tx_bytes);
        match a.name {
            Some(name) => apps.push(AppBytes {
                name,
                rx_bytes: a.rx_bytes,
                tx_bytes: a.tx_bytes,
            }),
            None => {
                other_rx = other_rx.saturating_add(a.rx_bytes);
                other_tx = other_tx.saturating_add(a.tx_bytes);
            }
        }
    }
    NetAttribution {
        from_ms: n.from_ms,
        to_ms: n.to_ms,
        resolution_ms: n.resolution_ms,
        measured_ms: n.measured_ms,
        coverage: n.coverage,
        apps,
        other_apps_rx_bytes: other_rx,
        other_apps_tx_bytes: other_tx,
        rx: split_direction(n.iface_rx_bytes, n.iface_rx_pkts, apps_rx),
        tx: split_direction(n.iface_tx_bytes, n.iface_tx_pkts, apps_tx),
    }
}

#[cfg(test)]
mod tests {
    use kelvo_schema::Tier;
    use kelvo_store::NetApp;

    use super::*;

    #[test]
    fn overhead_is_packets_times_the_header_and_the_rest_is_system() {
        // 1,000 packets: 66,000 B of headers. 1 MB interface, 800 kB of apps.
        let s = split_direction(1_000_000, 1_000, 800_000);
        assert_eq!(s.overhead_bytes, 66_000);
        assert_eq!(s.system_bytes, 134_000);
        assert!(!s.clamped);
        assert_eq!(
            s.attributed_bytes + s.overhead_bytes + s.system_bytes,
            s.iface_bytes
        );
    }

    #[test]
    fn the_overhead_estimate_shrinks_to_fit_before_the_apps_do() {
        // Apps leave 30,000 B; the estimate says 66,000. No double count, no system.
        let s = split_direction(1_000_000, 1_000, 970_000);
        assert_eq!((s.overhead_bytes, s.system_bytes), (30_000, 0));
        assert!(!s.clamped);
        assert_eq!(
            s.attributed_bytes + s.overhead_bytes + s.system_bytes,
            s.iface_bytes
        );
        // No apps at all: the estimate is capped at the interface total.
        let s = split_direction(10_000, 1_000, 0);
        assert_eq!((s.overhead_bytes, s.system_bytes), (10_000, 0));
    }

    #[test]
    fn apps_above_the_interface_clamp_only_past_the_counter_slack() {
        // Within the 1 KiB-granular counters' slack: not a double count.
        let s = split_direction(1_000_000, 10, 1_000_000 + CLAMP_SLACK_BYTES);
        assert_eq!((s.overhead_bytes, s.system_bytes), (0, 0));
        assert!(!s.clamped);
        // A tunnel counted twice.
        let s = split_direction(1_000_000, 10, 1_900_000);
        assert_eq!((s.overhead_bytes, s.system_bytes), (0, 0));
        assert!(s.clamped);
        // Nothing overflows on absurd inputs.
        let s = split_direction(u64::MAX, u64::MAX, 0);
        assert_eq!(s.overhead_bytes, u64::MAX);
    }

    fn app(name: Option<&str>, rx: u64, tx: u64) -> NetApp {
        NetApp {
            name: name.map(str::to_owned),
            rx_bytes: rx,
            tx_bytes: tx,
        }
    }

    #[test]
    fn attribute_separates_other_apps_and_splits_both_directions() {
        let span = NetSpan {
            from_ms: 0,
            to_ms: 60_000,
            tier: Some(Tier::S10),
        };
        let a = attribute(NetByApp {
            from_ms: 0,
            to_ms: 60_000,
            coverage: vec![span],
            resolution_ms: 10_000,
            measured_ms: 60_000,
            iface_rx_bytes: 10_000_000,
            iface_tx_bytes: 500_000,
            iface_rx_pkts: 7_000,
            iface_tx_pkts: 3_000,
            apps: vec![
                app(Some("Docker Desktop"), 8_000_000, 100_000),
                app(None, 300_000, 20_000),
                app(Some("Safari"), 900_000, 30_000),
            ],
        });
        assert_eq!(
            a.apps.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
            ["Docker Desktop", "Safari"],
            "the store's order, other apps left out"
        );
        assert_eq!(
            (a.other_apps_rx_bytes, a.other_apps_tx_bytes),
            (300_000, 20_000)
        );
        assert_eq!(a.rx.attributed_bytes, 9_200_000);
        assert_eq!(a.rx.overhead_bytes, 462_000);
        assert_eq!(a.rx.system_bytes, 338_000);
        assert_eq!(a.tx.attributed_bytes, 150_000);
        assert_eq!(a.tx.overhead_bytes, 198_000);
        assert_eq!(a.tx.system_bytes, 152_000);
        assert!(!a.clamped());
        assert_eq!(a.coverage, vec![span]);
    }
}
