//! Per-interface network rates from `/sys/class/net/<iface>/statistics`: `net.rx`,
//! `net.tx` (bytes/s) and `net.link_rate` (bits/s), labelled `iface`, plus the
//! `net.rx_total`/`net.tx_total` sums and the exact [`IfaceNet`] counter deltas.
//!
//! Which interfaces: those with a `device` link (backed by a NIC: `end0`, `wlp1s0f0`,
//! a VM's virtio NIC), plus tunnels of type 65534 (`ARPHRD_NONE`: `tailscale0`,
//! WireGuard, tun). Everything else has no `device` link and another type, so the rule
//! alone leaves out loopback, `veth*`, `br-*`, `docker0`, `virbr*`, `cni*`, `flannel*`,
//! bonds and VLANs (whose traffic is already on their member NICs). Interfaces are
//! reported whether they are up or down, so a link going down does not change the
//! series set.
//!
//! The totals and [`IfaceNet`] count only the NIC-backed interfaces. Tunnel traffic is
//! encapsulated on a NIC as well, so adding the tunnel would count it twice. This is the
//! one difference from the macOS totals, which sum every reported interface (macOS
//! reports no tunnels).
//!
//! Counters are 64-bit in the kernel, but some drivers keep 32-bit ones: a decrease
//! from a value below 2^32 is a 32-bit wrap, any other decrease a reset (no rate that
//! sample). Rates above about 400 Gbit/s are dropped as counter glitches.
//!
//! `net.link_rate` is the `speed` attribute (Mb/s), at most every 60 s; absent when it
//! is unreadable or `-1` (tunnels, Wi-Fi, a link that is down).
//!
//! The interface list is rescanned every 30 s, and the series revision bumps only when
//! the filtered set changed, so a container's veth coming and going changes nothing. The
//! re-probe keeps the baselines of the interfaces that stayed.
//!
//! The primary interface (D-092) is the reported interface carrying the main table's
//! IPv4 default route (`/proc/net/route`), else the IPv6 one, read on probe and with the
//! link rate.

use std::path::PathBuf;
use std::sync::Arc;

use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey};

use super::SysRoot;
use super::counters::{PartSum, read_u64_at, span_secs};
use crate::{
    Cadence, CollectError, Collector, CollectorId, Every, IfaceNet, Interval, Probe, SampleBuf,
    Tick,
};

/// `ARPHRD_NONE`: L3 tunnels (tun, WireGuard, Tailscale).
const TUNNEL_TYPE: u64 = 65534;
const LINK_RATE_MS: u32 = 60_000;
const RESCAN_MS: u32 = 30_000;
/// Rates above this are a counter glitch, not traffic (about 400 Gbit/s).
const MAX_PLAUSIBLE_BPS: f64 = 50e9;

/// Whether `/sys/class/net/<iface>/device` exists. A seam so fixture trees, which
/// cannot hold symlinks, can say which interfaces have one.
pub(super) type DeviceLink = Box<dyn Fn(&SysRoot, &str) -> bool + Send>;

/// The live device-link check.
pub(super) fn has_device_link(root: &SysRoot, iface: &str) -> bool {
    root.path(&format!("/sys/class/net/{iface}/device"))
        .symlink_metadata()
        .is_ok()
}

/// A reported interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Iface {
    pub(super) name: String,
    /// NIC-backed, so counted in the totals; false for a tunnel.
    pub(super) physical: bool,
}

/// `Some(physical)` for a reported interface, `None` for one left out.
fn classify(has_device: bool, if_type: Option<u64>) -> Option<bool> {
    if has_device {
        Some(true)
    } else if if_type == Some(TUNNEL_TYPE) {
        Some(false)
    } else {
        None
    }
}

/// The reported interfaces, sorted by name.
pub(super) fn scan(root: &SysRoot, link: &DeviceLink) -> Vec<Iface> {
    let mut out: Vec<Iface> = std::fs::read_dir(root.path("/sys/class/net"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|name| {
            let ty = root.read_u64(&format!("/sys/class/net/{name}/type"));
            let physical = classify(link(root, &name), ty)?;
            Some(Iface { name, physical })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Counter delta with 32-bit wrap handling; `None` means a reset.
fn counter_delta(prev: u64, cur: u64) -> Option<u64> {
    if cur >= prev {
        Some(cur - prev)
    } else if prev <= u64::from(u32::MAX) {
        Some(cur + (1u64 << 32) - prev)
    } else {
        None
    }
}

/// rx bytes, tx bytes, rx packets, tx packets.
type Totals = [u64; 4];
const STATS: [&str; 4] = ["rx_bytes", "tx_bytes", "rx_packets", "tx_packets"];

fn since(prev: &Totals, cur: &Totals) -> Option<Totals> {
    Some([
        counter_delta(prev[0], cur[0])?,
        counter_delta(prev[1], cur[1])?,
        counter_delta(prev[2], cur[2])?,
        counter_delta(prev[3], cur[3])?,
    ])
}

/// The interface of the lowest-metric default route in `/proc/net/route` text.
fn default_route_v4(text: &str) -> Option<&str> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_ascii_whitespace().collect();
            let (iface, dest, flags, metric, mask) =
                (f.first()?, f.get(1)?, f.get(3)?, f.get(6)?, f.get(7)?);
            let up = u32::from_str_radix(flags, 16).ok()? & 0x1 != 0;
            (up && *dest == "00000000" && *mask == "00000000")
                .then_some((metric.parse::<u32>().ok()?, *iface))
        })
        .min()
        .map(|(_, iface)| iface)
}

/// The same for `/proc/net/ipv6_route` (`::/0`, not on `lo`, not a reject route).
fn default_route_v6(text: &str) -> Option<&str> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_ascii_whitespace().collect();
            let (dest, plen, metric, flags, iface) =
                (f.first()?, f.get(1)?, f.get(5)?, f.get(8)?, f.get(9)?);
            let flags = u32::from_str_radix(flags, 16).ok()?;
            let default = dest.bytes().all(|b| b == b'0') && *plen == "00";
            let usable = flags & 0x1 != 0 && flags & 0x200 == 0 && *iface != "lo";
            (default && usable).then_some((u32::from_str_radix(metric, 16).ok()?, *iface))
        })
        .min()
        .map(|(_, iface)| iface)
}

struct Slot {
    /// The `iface` label, shared so the primary interface is passed on without allocating.
    name: Arc<str>,
    physical: bool,
    stats: [PathBuf; 4],
    speed: PathBuf,
    rx: SeriesKey,
    tx: SeriesKey,
    link: SeriesKey,
    prev: Option<Totals>,
}

impl Slot {
    fn read(&self) -> Option<Totals> {
        let [a, b, c, d] = &self.stats;
        Some([
            read_u64_at(a)?,
            read_u64_at(b)?,
            read_u64_at(c)?,
            read_u64_at(d)?,
        ])
    }
}

/// `net.rx`/`tx`/`link_rate{iface}` and the totals.
pub struct Network {
    root: SysRoot,
    link: DeviceLink,
    slots: Vec<Slot>,
    prev_ns: u64,
    link_every: Every,
    rescan: Every,
    /// Set by `probe`: the first rescan due after it is the probe's own scan.
    fresh: bool,
    revision: u64,
    keep_baseline: bool,
    rx_total: SeriesKey,
    tx_total: SeriesKey,
    primary_due: bool,
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

impl Network {
    pub fn new() -> Self {
        Self::with(SysRoot::host(), Box::new(has_device_link))
    }

    pub(super) fn with(root: SysRoot, link: DeviceLink) -> Self {
        Self {
            root,
            link,
            slots: Vec::new(),
            prev_ns: 0,
            link_every: Every::new(LINK_RATE_MS),
            rescan: Every::new(RESCAN_MS),
            fresh: true,
            revision: 0,
            keep_baseline: false,
            rx_total: SeriesKey::bare(MetricId::from_static("net.rx_total")),
            tx_total: SeriesKey::bare(MetricId::from_static("net.tx_total")),
            primary_due: true,
        }
    }

    fn slot(&self, iface: &Iface, prev: Option<Totals>) -> Slot {
        let n = &iface.name;
        let key = |id| SeriesKey::new(MetricId::from_static(id), Labels::single("iface", n));
        Slot {
            name: Arc::from(n.as_str()),
            physical: iface.physical,
            stats: STATS.map(|s| {
                self.root
                    .path(&format!("/sys/class/net/{n}/statistics/{s}"))
            }),
            speed: self.root.path(&format!("/sys/class/net/{n}/speed")),
            rx: key("net.rx"),
            tx: key("net.tx"),
            link: key("net.link_rate"),
            prev,
        }
    }

    /// The reported interface carrying the default route.
    fn primary(&self) -> Option<Arc<str>> {
        let v4 = self.root.read("/proc/net/route").unwrap_or_default();
        let v6 = self.root.read("/proc/net/ipv6_route").unwrap_or_default();
        let route = default_route_v4(&v4).or_else(|| default_route_v6(&v6))?;
        self.slots
            .iter()
            .find(|s| &*s.name == route)
            .map(|s| Arc::clone(&s.name))
    }

    fn emit(&mut self, now: u64, link_tick: bool, out: &mut SampleBuf) {
        let secs = span_secs(self.prev_ns, now);
        let mut sum: Totals = [0; 4];
        let mut summed = false;
        let mut rx_total = PartSum::default();
        let mut tx_total = PartSum::default();
        for slot in &mut self.slots {
            let cur = slot.read();
            let mut rates = [None, None];
            if let (Some(prev), Some(cur), Some(secs)) = (slot.prev, cur, secs) {
                for (i, rate) in rates.iter_mut().enumerate() {
                    *rate = counter_delta(prev[i], cur[i])
                        .map(|d| d as f64 / secs)
                        .filter(|r| *r <= MAX_PLAUSIBLE_BPS)
                        .map(|r| r as f32);
                }
                let plausible = |d: &Totals| (d[0].max(d[1]) as f64 / secs) <= MAX_PLAUSIBLE_BPS;
                if slot.physical
                    && let Some(d) = since(&prev, &cur).filter(plausible)
                {
                    for (s, d) in sum.iter_mut().zip(d) {
                        *s = s.saturating_add(d);
                    }
                    summed = true;
                }
            }
            let [rx, tx] = rates;
            if let Some(r) = rx {
                out.push(&slot.rx, r);
            }
            if let Some(r) = tx {
                out.push(&slot.tx, r);
            }
            if slot.physical {
                rx_total.add(rx);
                tx_total.add(tx);
            }
            if link_tick && let Some(mbps) = read_u64_at(&slot.speed).filter(|&s| s > 0) {
                out.push(&slot.link, (mbps as f64 * 1e6) as f32);
            }
            slot.prev = cur;
        }
        if self.slots.iter().any(|s| s.physical) {
            if let Some(v) = rx_total.total() {
                out.push(&self.rx_total, v);
            }
            if let Some(v) = tx_total.total() {
                out.push(&self.tx_total, v);
            }
        }
        if summed {
            out.set_iface_net(IfaceNet {
                interval: Interval {
                    prev_ns: self.prev_ns,
                    now_ns: now,
                },
                rx_bytes: sum[0],
                tx_bytes: sum[1],
                rx_packets: sum[2],
                tx_packets: sum[3],
            });
        }
        self.prev_ns = now;
    }

    fn set_changed(&self) -> bool {
        let now = scan(&self.root, &self.link);
        !now.iter()
            .map(|i| (i.name.as_str(), i.physical))
            .eq(self.slots.iter().map(|s| (&*s.name, s.physical)))
    }
}

impl Collector for Network {
    fn id(&self) -> CollectorId {
        CollectorId("network")
    }

    fn cadence(&self) -> Cadence {
        // D-067: every tick while a window or the menu bar shows it, else every 10 s.
        crate::LIVE_OR_IDLE
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Network]
    }

    fn probe(&mut self) -> Probe {
        let keep = std::mem::take(&mut self.keep_baseline);
        let old = std::mem::take(&mut self.slots);
        if !keep {
            self.prev_ns = 0;
            self.link_every.reset();
        }
        self.rescan.reset();
        self.fresh = true;
        self.primary_due = true;
        let mut series = Vec::new();
        for iface in scan(&self.root, &self.link) {
            let prev = old
                .iter()
                .find(|o| keep && *o.name == *iface.name)
                .and_then(|o| o.prev);
            let slot = self.slot(&iface, prev);
            series.extend([slot.rx.clone(), slot.tx.clone(), slot.link.clone()]);
            self.slots.push(slot);
        }
        if self.slots.iter().any(|s| s.physical) {
            series.extend([self.rx_total.clone(), self.tx_total.clone()]);
        }
        Probe::Supported(series)
    }

    fn series_revision(&self) -> u64 {
        self.revision
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        let link_tick = self.link_every.due(tick);
        if link_tick || std::mem::take(&mut self.primary_due) {
            out.set_primary_iface(self.primary());
        }
        self.emit(tick.continuous_ns, link_tick, out);
        if self.rescan.due(tick) && !std::mem::take(&mut self.fresh) && self.set_changed() {
            self.revision += 1;
            self.keep_baseline = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::blockdev::tests::fixture;
    use super::*;

    fn fixture_links() -> DeviceLink {
        let text = fixture("t0").read("/net_device_links.txt").unwrap();
        let with: Vec<String> = text
            .lines()
            .filter_map(|l| l.strip_suffix(" device"))
            .map(str::to_owned)
            .collect();
        Box::new(move |_, iface| with.iter().any(|w| w == iface))
    }

    fn tick(n: u64, secs: u64) -> Tick {
        Tick {
            n,
            wall_ms: 0,
            continuous_ns: secs * 1_000_000_000,
            interval_ms: 1_000,
        }
    }

    fn get(buf: &SampleBuf, k: &str) -> Option<f32> {
        buf.values()
            .iter()
            .find(|s| s.key.to_string() == k)
            .map(|s| s.value)
    }

    #[test]
    fn fixture_keeps_nics_and_tunnels_only() {
        let ifaces = scan(&fixture("t0"), &fixture_links());
        assert_eq!(
            ifaces,
            [
                Iface {
                    name: "end0".into(),
                    physical: true
                },
                Iface {
                    name: "tailscale0".into(),
                    physical: false
                },
                Iface {
                    name: "wlp1s0f0".into(),
                    physical: true
                },
            ]
        );
        // Mutation check: without the device-link seam nothing NIC-backed is left, and
        // veth/bridges/docker0 are never let in by their type.
        let none: DeviceLink = Box::new(|_, _| false);
        let only_tun = scan(&fixture("t0"), &none);
        assert_eq!(only_tun.len(), 1);
        assert_eq!(classify(false, Some(1)), None, "veth, br-*, docker0");
        assert_eq!(classify(false, Some(772)), None, "lo");
        assert_eq!(classify(false, None), None);
        assert_eq!(classify(true, Some(1)), Some(true));
        assert_eq!(classify(false, Some(65534)), Some(false));
    }

    /// t0 -> t1 is one second. end0 rx 687886851694 -> ..856816 (+5122 B), tx
    /// 500779778945 -> ..781329 (+2384 B), +25 / +18 packets. tailscale0 and wlp1s0f0
    /// did not move; veth14c2281 did (+699 B rx) but is not reported.
    #[test]
    fn fixture_rates_and_totals_cover_nics_only() {
        let mut c = Network::with(fixture("t0"), fixture_links());
        let Probe::Supported(series) = c.probe() else {
            panic!("probe");
        };
        assert_eq!(series.len(), 3 * 3 + 2);
        let mut buf = SampleBuf::new();
        c.sample(&tick(0, 10), &mut buf).unwrap();
        assert_eq!(get(&buf, "net.link_rate{iface=end0}"), Some(1e9));
        assert_eq!(
            get(&buf, "net.link_rate{iface=tailscale0}"),
            None,
            "speed -1"
        );
        assert_eq!(
            get(&buf, "net.link_rate{iface=wlp1s0f0}"),
            None,
            "no speed file"
        );
        assert_eq!(get(&buf, "net.rx{iface=end0}"), None, "baseline");
        assert!(buf.iface_net().is_none());
        // Point the same slots at t1.
        let t1 = Network::with(fixture("t1"), fixture_links());
        for s in &mut c.slots {
            let fresh = t1.slot(
                &Iface {
                    name: s.name.to_string(),
                    physical: s.physical,
                },
                None,
            );
            s.stats = fresh.stats;
        }
        buf.clear();
        c.sample(&tick(1, 11), &mut buf).unwrap();
        assert_eq!(get(&buf, "net.rx{iface=end0}"), Some(5122.));
        assert_eq!(get(&buf, "net.tx{iface=end0}"), Some(2384.));
        assert_eq!(get(&buf, "net.rx{iface=tailscale0}"), Some(0.));
        assert_eq!(get(&buf, "net.rx_total"), Some(5122.));
        assert_eq!(get(&buf, "net.tx_total"), Some(2384.));
        assert_eq!(get(&buf, "net.link_rate{iface=end0}"), None, "60 s cadence");
        let n = buf.iface_net().unwrap();
        assert_eq!(
            (n.rx_bytes, n.tx_bytes, n.rx_packets, n.tx_packets),
            (5122, 2384, 25, 18)
        );
        assert_eq!(
            n.interval,
            Interval {
                prev_ns: 10_000_000_000,
                now_ns: 11_000_000_000
            }
        );
    }

    #[test]
    fn tunnels_stay_out_of_the_totals() {
        let dir = tempfile::tempdir().unwrap();
        let root = SysRoot::at(dir.path());
        let set = |iface: &str, ty: &str, rx: u64| {
            let base = format!("/sys/class/net/{iface}");
            std::fs::create_dir_all(root.path(&format!("{base}/statistics"))).unwrap();
            std::fs::write(root.path(&format!("{base}/type")), ty).unwrap();
            for (s, v) in STATS.iter().zip([rx, 0, 0, 0]) {
                std::fs::write(root.path(&format!("{base}/statistics/{s}")), v.to_string())
                    .unwrap();
            }
        };
        set("eth0", "1\n", 1000);
        set("wg0", "65534\n", 500);
        set("veth1", "1\n", 0);
        std::fs::write(root.path("/sys/class/net/eth0/speed"), "0\n").unwrap();
        let link: DeviceLink = Box::new(|_, i| i == "eth0");
        let mut c = Network::with(root.clone(), link);
        c.probe();
        let mut buf = SampleBuf::new();
        c.sample(&tick(0, 1), &mut buf).unwrap();
        assert_eq!(
            get(&buf, "net.link_rate{iface=eth0}"),
            None,
            "speed 0 is unknown"
        );
        set("eth0", "1\n", 3000);
        set("wg0", "65534\n", 1500);
        set("veth1", "1\n", 99_999);
        buf.clear();
        c.sample(&tick(1, 3), &mut buf).unwrap();
        assert_eq!(get(&buf, "net.rx{iface=eth0}"), Some(1000.));
        assert_eq!(get(&buf, "net.rx{iface=wg0}"), Some(500.));
        assert_eq!(
            get(&buf, "net.rx_total"),
            Some(1000.),
            "the tunnel is not added"
        );
        assert_eq!(buf.iface_net().unwrap().rx_bytes, 2000);
        assert!(
            !buf.values()
                .iter()
                .any(|s| s.key.to_string().contains("veth"))
        );
        // A vanished NIC is a gap in its series and the totals, not an error.
        std::fs::remove_dir_all(root.path("/sys/class/net/eth0")).unwrap();
        buf.clear();
        c.sample(&tick(2, 4), &mut buf).unwrap();
        assert_eq!(get(&buf, "net.rx_total"), None);
        assert_eq!(get(&buf, "net.rx{iface=wg0}"), Some(0.));
    }

    #[test]
    fn veth_churn_does_not_bump_the_revision_but_a_nic_does() {
        let dir = tempfile::tempdir().unwrap();
        let root = SysRoot::at(dir.path());
        let mk = |iface: &str, ty: &str| {
            std::fs::create_dir_all(root.path(&format!("/sys/class/net/{iface}/statistics")))
                .unwrap();
            std::fs::write(root.path(&format!("/sys/class/net/{iface}/type")), ty).unwrap();
        };
        mk("eth0", "1");
        let link: DeviceLink = Box::new(has_device_link);
        let mut c = Network::with(root.clone(), link);
        assert!(
            matches!(c.probe(), Probe::Supported(s) if s.is_empty()),
            "no device link yet"
        );
        std::os::unix::fs::symlink(
            "../../devices/pci0000:00/0000:00:03.0",
            root.path("/sys/class/net/eth0/device"),
        )
        .unwrap();
        let mut buf = SampleBuf::new();
        c.sample(&tick(0, 0), &mut buf).unwrap();
        mk("veth9", "1");
        mk("br-1", "1");
        c.sample(&tick(1, 30), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 1, "eth0 gained its device link");
        assert!(matches!(c.probe(), Probe::Supported(s) if s.len() == 5));
        c.sample(&tick(2, 31), &mut buf).unwrap();
        std::fs::remove_dir_all(root.path("/sys/class/net/veth9")).unwrap();
        mk("veth10", "1");
        c.sample(&tick(3, 61), &mut buf).unwrap();
        assert_eq!(c.series_revision(), 1, "veth churn is filtered out");
    }

    #[test]
    fn counter_wraps_at_32_bits_and_resets_otherwise() {
        assert_eq!(counter_delta(100, 150), Some(50));
        assert_eq!(counter_delta(u64::from(u32::MAX) - 9, 5), Some(15));
        assert_eq!(counter_delta(1 << 40, 5), None);
        assert_eq!(since(&[1 << 40, 0, 0, 0], &[5, 0, 0, 0]), None);
    }

    #[test]
    fn default_routes() {
        let v4 = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
                  wlan0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0\n\
                  end0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n\
                  end0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0\n\
                  tun0\t00000000\t00000000\t0002\t0\t0\t1\t00000000\t0\t0\t0\n";
        assert_eq!(
            default_route_v4(v4),
            Some("end0"),
            "lowest metric among up routes"
        );
        assert_eq!(default_route_v4("Iface\tDestination\n"), None);
        let z = "00000000000000000000000000000000";
        let v6 = format!(
            "{z} 00 {z} 00 fe800000000000000000000000000001 00000400 00000001 00000000 00000003 end0\n\
             {z} 00 {z} 00 {z} ffffffff 00000001 00000000 00200200 lo\n"
        );
        assert_eq!(default_route_v6(&v6), Some("end0"));
    }
}
