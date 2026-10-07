//! Per-interface network rates: `net.rx`, `net.tx` and `net.link_rate`, labelled `iface`.
//!
//! Counters come from `sysctl(NET_RT_IFLIST2)`, whose `if_msghdr2` carries `if_data64`
//! (the same source as `netstat -ib`). The 64-bit fields do not give unprivileged
//! callers 64-bit bytes: measured on macOS 27 (D-089), the kernel hands a non-root
//! caller byte counters truncated to 32 bits and rounded down to 1 KiB, so they wrap
//! every 4 GiB and move in 1,024-byte steps. Packet counters are exact. A decrease from
//! a value below 2^32 is therefore a 32-bit wrap; any other decrease is a counter reset
//! and yields no rate for that tick. Over a 10 s history bucket the 1 KiB steps are
//! noise; the per-tick rate is accurate to 1 KiB per sample.
//!
//! Besides the rates, each measured sample records the exact counter deltas, bytes and
//! packets summed over the reported interfaces, as [`IfaceNet`] on the [`SampleBuf`],
//! so network history can split integer totals across buckets.
//!
//! Interface kind comes from SystemConfiguration (`SCNetworkInterfaceGetInterfaceType`,
//! public API), with name-prefix fallbacks for interfaces SystemConfiguration does not
//! list. Only Wi-Fi, Ethernet and cellular interfaces that are up, running and have
//! moved at least one byte become series; loopback, VPN tunnels, bridges, AWDL, VM
//! interfaces and idle Thunderbolt ports are skipped so totals do not double count.
//! The engine re-probes to pick up interfaces that appear later.
//!
//! `net.rx_total` and `net.tx_total` sum the per-interface rates of the same sample
//! (D-092). The parts are the interfaces the last probe reported, so membership changes
//! when the engine re-probes. A total is a gap when any part's own value is a gap on that
//! sample (no previous counters, a reset, an implausible rate, the interface gone until
//! the re-probe drops it); rx and tx are gated separately.
//!
//! `net.link_rate` is `ifi_baudrate`, sampled at most every 60 s. For Wi-Fi this is the
//! driver's nominal figure, not the CoreWLAN transmit rate (not implemented; unverified
//! whether CoreWLAN needs location permission for it).
//!
//! The primary interface (D-092) is the reported interface carrying the default route:
//! `PrimaryInterface` under SystemConfiguration's `State:/Network/Global/IPv4` key, or the
//! IPv6 key when there is no IPv4 one. It is read on probe and on the link-rate cadence,
//! and is `None` when the route is on an interface the collector does not report (a VPN
//! tunnel such as `utun3`).

use std::collections::HashMap;
use std::sync::Arc;

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_foundation_sys::array::CFArrayRef;
use core_foundation_sys::base::CFIndex;
use core_foundation_sys::base::CFTypeRef;
use core_foundation_sys::string::{CFStringGetCString, CFStringRef, kCFStringEncodingUTF8};
use kelvo_schema::{Entitlement, Labels, MetricId, Module, SeriesKey};

use super::{PartSum, sysctl};
use crate::{
    Cadence, CollectError, Collector, CollectorId, Every, IfaceNet, Interval, Probe, SampleBuf,
    Tick,
};

/// `net.link_rate` changes only on reassociation: at most every 60 s.
const LINK_RATE_MS: u32 = 60_000;
/// Rates above this are a counter glitch, not traffic (about 400 Gbit/s).
const MAX_PLAUSIBLE_BPS: f64 = 50e9;

/// What kind of link an interface is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InterfaceKind {
    Wifi,
    Ethernet,
    Cellular,
    Vpn,
    Bridge,
    Loopback,
    Other,
}

impl InterfaceKind {
    /// Whether the collector reports this kind as series.
    fn reported(self) -> bool {
        matches!(
            self,
            InterfaceKind::Wifi | InterfaceKind::Ethernet | InterfaceKind::Cellular
        )
    }
}

/// An interface the collector reports.
#[derive(Clone, Debug, PartialEq)]
pub struct InterfaceInfo {
    pub name: String,
    pub kind: InterfaceKind,
}

/// One interface's counters from a routing-socket dump.
#[derive(Clone, Copy, Debug, PartialEq)]
struct IfCounters {
    index: u16,
    flags: i32,
    ibytes: u64,
    obytes: u64,
    ipackets: u64,
    opackets: u64,
    baudrate: u64,
}

impl IfCounters {
    fn totals(&self) -> Totals {
        Totals {
            ibytes: self.ibytes,
            obytes: self.obytes,
            ipackets: self.ipackets,
            opackets: self.opackets,
        }
    }
}

/// Cumulative counters of one interface, or deltas between two readings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Totals {
    ibytes: u64,
    obytes: u64,
    ipackets: u64,
    opackets: u64,
}

impl Totals {
    /// The deltas since `prev`, `None` if any counter reset.
    fn since(&self, prev: &Totals) -> Option<Totals> {
        Some(Totals {
            ibytes: counter_delta(prev.ibytes, self.ibytes)?,
            obytes: counter_delta(prev.obytes, self.obytes)?,
            ipackets: counter_delta(prev.ipackets, self.ipackets)?,
            opackets: counter_delta(prev.opackets, self.opackets)?,
        })
    }

    fn add(&mut self, d: &Totals) {
        self.ibytes = self.ibytes.saturating_add(d.ibytes);
        self.obytes = self.obytes.saturating_add(d.obytes);
        self.ipackets = self.ipackets.saturating_add(d.ipackets);
        self.opackets = self.opackets.saturating_add(d.opackets);
    }
}

/// Parses an `NET_RT_IFLIST2` buffer into `out` (cleared first). Messages that are not
/// `RTM_IFINFO2` or are truncated are skipped.
fn parse_iflist2(buf: &[u8], out: &mut Vec<IfCounters>) {
    out.clear();
    let mut off = 0usize;
    while let Some(rest) = buf.get(off..) {
        // ifm_msglen (u16), ifm_version (u8), ifm_type (u8) lead every message.
        let Some(&[l0, l1, _ver, ty]) = rest.get(..4) else {
            break;
        };
        let len = usize::from(u16::from_ne_bytes([l0, l1]));
        if len == 0 {
            break;
        }
        if i32::from(ty) == libc::RTM_IFINFO2 && rest.len() >= size_of::<libc::if_msghdr2>() {
            // SAFETY: at least size_of::<if_msghdr2>() bytes remain at `rest`, and
            // read_unaligned copes with any alignment. All fields are integers.
            let m = unsafe { std::ptr::read_unaligned(rest.as_ptr().cast::<libc::if_msghdr2>()) };
            out.push(IfCounters {
                index: m.ifm_index,
                flags: m.ifm_flags,
                ibytes: m.ifm_data.ifi_ibytes,
                obytes: m.ifm_data.ifi_obytes,
                ipackets: m.ifm_data.ifi_ipackets,
                opackets: m.ifm_data.ifi_opackets,
                baudrate: m.ifm_data.ifi_baudrate,
            });
        }
        off += len;
    }
}

/// Fills `buf` with the `NET_RT_IFLIST2` dump. Reuses `buf`'s capacity.
fn read_iflist2(buf: &mut Vec<u8>) -> Result<(), CollectError> {
    let mut mib = [libc::CTL_NET, libc::PF_ROUTE, 0, 0, libc::NET_RT_IFLIST2, 0];
    for _ in 0..3 {
        let Some(len) = sysctl::mib_len(&mut mib) else {
            break;
        };
        // Headroom for interfaces appearing between the two calls.
        let want = len + len / 8;
        buf.clear();
        buf.resize(want, 0);
        if let Some(len) = sysctl::mib_into(&mut mib, buf) {
            buf.truncate(len);
            return Ok(());
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            break;
        }
    }
    Err(CollectError::Os {
        call: "sysctl(NET_RT_IFLIST2)",
        code: i64::from(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)),
    })
}

/// Counter delta with 32-bit wrap handling. `None` means a reset. Unprivileged byte
/// counters are 32-bit (see the module docs); packet counters get the same treatment
/// in case a driver keeps them in 32 bits too.
fn counter_delta(prev: u64, cur: u64) -> Option<u64> {
    if cur >= prev {
        Some(cur - prev)
    } else if prev <= u64::from(u32::MAX) {
        Some(cur + (1u64 << 32) - prev)
    } else {
        None
    }
}

fn if_name(index: u16) -> Option<String> {
    let mut name = [0 as libc::c_char; libc::IF_NAMESIZE];
    // SAFETY: `name` is IF_NAMESIZE bytes, the size if_indextoname requires.
    let p = unsafe { libc::if_indextoname(u32::from(index), name.as_mut_ptr()) };
    if p.is_null() {
        return None;
    }
    // SAFETY: on success the buffer holds a NUL-terminated name.
    let s = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) };
    s.to_str().ok().map(str::to_owned)
}

/// Whether the interface with this index is a kind the collector reports, so its bytes
/// are in the interface totals. Per-app network bytes count only flows on these
/// interfaces (D-089), or apps would exceed the totals they split. Reads the interface
/// name and SystemConfiguration's interface list: callers cache the answer per index.
pub(crate) fn is_reported_interface(index: u32) -> bool {
    let Some(name) = u16::try_from(index).ok().and_then(if_name) else {
        return false;
    };
    let types = sc_interface_types();
    classify(&name, types.get(&name).map(String::as_str)).reported()
}

#[link(name = "SystemConfiguration", kind = "framework")]
unsafe extern "C" {
    fn SCNetworkInterfaceCopyAll() -> CFArrayRef;
    fn SCNetworkInterfaceGetBSDName(interface: CFTypeRef) -> CFStringRef;
    fn SCNetworkInterfaceGetInterfaceType(interface: CFTypeRef) -> CFStringRef;
    fn SCDynamicStoreCopyValue(store: CFTypeRef, key: CFStringRef) -> CFTypeRef;
}

/// `PrimaryInterface` under a SystemConfiguration global state key
/// (`State:/Network/Global/IPv4` or `IPv6`), `None` when the key or the entry is absent.
/// The name is copied into `buf`, so the read makes no Rust heap allocation.
fn global_primary_interface<'b>(key: &'static str, buf: &'b mut IfNameBuf) -> Option<&'b str> {
    let key = CFString::from_static_string(key);
    // SAFETY: a null store uses a temporary session (the parameter is nullable in the
    // SDK header); Copy rule, so a non-null result is owned here.
    let v = unsafe { SCDynamicStoreCopyValue(std::ptr::null(), key.as_concrete_TypeRef()) };
    if v.is_null() {
        return None;
    }
    // SAFETY: non-null CF object we own (Copy rule).
    let v = unsafe { CFType::wrap_under_create_rule(v) };
    let dict = v.downcast::<CFDictionary>()?;
    // SAFETY: the global state values are CFString-keyed dictionaries; the get rule
    // retains the dictionary for the wrapper's lifetime.
    let dict: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_get_rule(dict.as_concrete_TypeRef()) };
    let name = dict
        .find(CFString::from_static_string("PrimaryInterface"))?
        .downcast::<CFString>()?;
    // SAFETY: `buf` has `buf.len()` writable bytes and CFStringGetCString writes at most
    // that many, NUL included; it fails rather than truncate.
    let ok = unsafe {
        CFStringGetCString(
            name.as_concrete_TypeRef(),
            buf.as_mut_ptr().cast(),
            buf.len() as CFIndex,
            kCFStringEncodingUTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    std::ffi::CStr::from_bytes_until_nul(buf)
        .ok()?
        .to_str()
        .ok()
}

/// Room for an interface name (`IF_NAMESIZE` bytes with its NUL) and then some.
type IfNameBuf = [u8; 32];

/// The primary interface among `reported` (D-092): the IPv4 default route's interface,
/// or the IPv6 one when there is no IPv4 route. `None` when that interface is not
/// reported (a VPN tunnel carries the route) or there is no route.
fn pick_primary(
    ipv4: Option<&str>,
    ipv6: Option<&str>,
    reported: &[InterfaceInfo],
) -> Option<usize> {
    let route = ipv4.or(ipv6)?;
    reported.iter().position(|i| i.name == route)
}

/// BSD name to SystemConfiguration interface type ("IEEE80211", "Ethernet", ...).
fn sc_interface_types() -> HashMap<String, String> {
    let mut out = HashMap::new();
    // SAFETY: Copy-rule function; we take ownership of the returned array (or null).
    let arr = unsafe { SCNetworkInterfaceCopyAll() };
    if arr.is_null() {
        return out;
    }
    // SAFETY: `arr` is a non-null CFArray we own (Create/Copy rule).
    let arr: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(arr) };
    for item in arr.iter() {
        let r = item.as_CFTypeRef();
        // SAFETY: `r` is an SCNetworkInterfaceRef from the array; the Get functions
        // return borrowed strings (or null) that live as long as the array.
        let (name, ty) = unsafe {
            (
                SCNetworkInterfaceGetBSDName(r),
                SCNetworkInterfaceGetInterfaceType(r),
            )
        };
        if name.is_null() || ty.is_null() {
            continue;
        }
        // SAFETY: non-null borrowed CFStrings (Get rule).
        let (name, ty) = unsafe {
            (
                CFString::wrap_under_get_rule(name),
                CFString::wrap_under_get_rule(ty),
            )
        };
        out.insert(name.to_string(), ty.to_string());
    }
    out
}

/// Interface kind from the SystemConfiguration type, falling back to the name.
fn classify(name: &str, sc_type: Option<&str>) -> InterfaceKind {
    match sc_type {
        Some("IEEE80211") => return InterfaceKind::Wifi,
        Some("Ethernet") => return InterfaceKind::Ethernet,
        Some("WWAN") => return InterfaceKind::Cellular,
        Some("Bridge") | Some("Bond") | Some("VLAN") => return InterfaceKind::Bridge,
        Some("PPP") | Some("IPSec") | Some("VPN") | Some("L2TP") | Some("PPTP") => {
            return InterfaceKind::Vpn;
        }
        _ => {}
    }
    let prefix = name.trim_end_matches(|c: char| c.is_ascii_digit());
    match prefix {
        "lo" => InterfaceKind::Loopback,
        "pdp_ip" => InterfaceKind::Cellular,
        "utun" | "ipsec" | "ppp" => InterfaceKind::Vpn,
        "bridge" => InterfaceKind::Bridge,
        _ => InterfaceKind::Other,
    }
}

struct Slot {
    index: u16,
    /// The `iface` label, shared so the primary interface is passed on without allocating.
    name: Arc<str>,
    rx: SeriesKey,
    tx: SeriesKey,
    link: SeriesKey,
    prev: Option<Totals>,
}

pub struct Network {
    slots: Vec<Slot>,
    infos: Vec<InterfaceInfo>,
    buf: Vec<u8>,
    counters: Vec<IfCounters>,
    prev_ns: u64,
    link_every: Every,
    rx_total: SeriesKey,
    tx_total: SeriesKey,
    /// Re-read the primary interface on the next sample (set by `probe`).
    primary_due: bool,
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

impl Network {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            infos: Vec::new(),
            buf: Vec::new(),
            counters: Vec::new(),
            prev_ns: 0,
            link_every: Every::new(LINK_RATE_MS),
            rx_total: SeriesKey::bare(MetricId::from_static("net.rx_total")),
            tx_total: SeriesKey::bare(MetricId::from_static("net.tx_total")),
            primary_due: true,
        }
    }

    /// The interfaces found by the last probe, in series order.
    pub fn interfaces(&self) -> &[InterfaceInfo] {
        &self.infos
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
        self.slots.clear();
        self.infos.clear();
        self.prev_ns = 0;
        self.link_every.reset();
        self.primary_due = true;
        if read_iflist2(&mut self.buf).is_err() {
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            };
        }
        parse_iflist2(&self.buf, &mut self.counters);
        let types = sc_interface_types();
        let up_running = libc::IFF_UP | libc::IFF_RUNNING;
        let mut series = Vec::new();
        for c in &self.counters {
            if c.flags & up_running != up_running || c.ibytes + c.obytes == 0 {
                continue;
            }
            let Some(name) = if_name(c.index) else {
                continue;
            };
            let kind = classify(&name, types.get(&name).map(String::as_str));
            if !kind.reported() {
                continue;
            }
            let key =
                |id| SeriesKey::new(MetricId::from_static(id), Labels::single("iface", &name));
            let slot = Slot {
                index: c.index,
                name: Arc::from(name.as_str()),
                rx: key("net.rx"),
                tx: key("net.tx"),
                link: key("net.link_rate"),
                prev: None,
            };
            series.extend([slot.rx.clone(), slot.tx.clone(), slot.link.clone()]);
            self.slots.push(slot);
            self.infos.push(InterfaceInfo { name, kind });
        }
        if !self.slots.is_empty() {
            series.extend([self.rx_total.clone(), self.tx_total.clone()]);
        }
        Probe::Supported(series)
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        read_iflist2(&mut self.buf)?;
        parse_iflist2(&self.buf, &mut self.counters);
        let link_tick = self.link_every.due(tick);
        if link_tick || self.primary_due {
            self.primary_due = false;
            let (mut b4, mut b6) = ([0; 32], [0; 32]);
            let ipv4 = global_primary_interface("State:/Network/Global/IPv4", &mut b4);
            let ipv6 = ipv4
                .is_none()
                .then(|| global_primary_interface("State:/Network/Global/IPv6", &mut b6))
                .flatten();
            // `slots` and `infos` are pushed together in `probe`.
            let primary = pick_primary(ipv4, ipv6, &self.infos)
                .and_then(|i| self.slots.get(i))
                .map(|slot| Arc::clone(&slot.name));
            out.set_primary_iface(primary);
        }
        self.emit(tick.continuous_ns, link_tick, out);
        Ok(())
    }
}

impl Network {
    /// Rates, totals and link rates from `self.counters`, read at `now` (continuous ns).
    fn emit(&mut self, now: u64, link_tick: bool, out: &mut SampleBuf) {
        let secs = if self.prev_ns > 0 && now > self.prev_ns {
            Some((now - self.prev_ns) as f64 / 1e9)
        } else {
            None
        };
        // Exact deltas over the interfaces with a valid delta this tick. An interface
        // that reset or just appeared adds nothing: its bytes in this interval are
        // unknown, the same as the rate series' missing value. These are counter facts
        // the per-app remainder splits (D-089): an interface with no delta has no bytes
        // to attribute, so unlike `net.rx_total` the sum keeps the other interfaces'
        // bytes instead of turning into a gap (D-092).
        let mut sum = Totals::default();
        let mut summed = false;
        let mut rx_total = PartSum::default();
        let mut tx_total = PartSum::default();
        for slot in &mut self.slots {
            let Some(c) = self.counters.iter().find(|c| c.index == slot.index) else {
                // The interface went away; the engine's re-probe drops the series. Until
                // then it is a reported part with no value.
                slot.prev = None;
                rx_total.add(None);
                tx_total.add(None);
                continue;
            };
            let cur = c.totals();
            let mut rates = [None, None];
            if let (Some(prev), Some(secs)) = (slot.prev, secs) {
                let deltas = [
                    counter_delta(prev.ibytes, cur.ibytes),
                    counter_delta(prev.obytes, cur.obytes),
                ];
                for (rate, d) in rates.iter_mut().zip(deltas) {
                    *rate = d
                        .map(|d| d as f64 / secs)
                        .filter(|r| *r <= MAX_PLAUSIBLE_BPS)
                        .map(|r| r as f32);
                }
                let plausible =
                    |d: &Totals| (d.ibytes.max(d.obytes) as f64 / secs) <= MAX_PLAUSIBLE_BPS;
                if let Some(d) = cur.since(&prev).filter(plausible) {
                    sum.add(&d);
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
            rx_total.add(rx);
            tx_total.add(tx);
            if link_tick && c.baudrate > 0 {
                out.push(&slot.link, c.baudrate as f32);
            }
            slot.prev = Some(cur);
        }
        if !self.slots.is_empty() {
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
                rx_bytes: sum.ibytes,
                tx_bytes: sum.obytes,
                rx_packets: sum.ipackets,
                tx_packets: sum.opackets,
            });
        }
        self.prev_ns = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_wraps_at_32_bits_and_resets_otherwise() {
        assert_eq!(counter_delta(100, 150), Some(50));
        assert_eq!(counter_delta(u64::from(u32::MAX) - 9, 5), Some(15));
        assert_eq!(counter_delta(1 << 40, 5), None);
    }

    #[test]
    fn classify_prefers_system_configuration() {
        assert_eq!(classify("en0", Some("IEEE80211")), InterfaceKind::Wifi);
        assert_eq!(classify("en7", Some("Ethernet")), InterfaceKind::Ethernet);
        assert_eq!(classify("pdp_ip0", None), InterfaceKind::Cellular);
        assert_eq!(classify("utun3", None), InterfaceKind::Vpn);
        assert_eq!(classify("lo0", None), InterfaceKind::Loopback);
        assert_eq!(classify("awdl0", None), InterfaceKind::Other);
        assert!(!InterfaceKind::Vpn.reported());
    }

    fn msg(index: u16, ty: i32, ibytes: u64) -> Vec<u8> {
        // SAFETY (test): all-zero is a valid if_msghdr2.
        let mut m: libc::if_msghdr2 = unsafe { std::mem::zeroed() };
        m.ifm_msglen = size_of::<libc::if_msghdr2>() as u16;
        m.ifm_type = ty as u8;
        m.ifm_index = index;
        m.ifm_flags = libc::IFF_UP | libc::IFF_RUNNING;
        m.ifm_data.ifi_ibytes = ibytes;
        m.ifm_data.ifi_obytes = ibytes / 2;
        m.ifm_data.ifi_ipackets = ibytes / 1_000;
        m.ifm_data.ifi_opackets = ibytes / 3_000;
        // SAFETY (test): viewing a POD struct as bytes.
        unsafe {
            std::slice::from_raw_parts(
                (&m as *const libc::if_msghdr2).cast::<u8>(),
                size_of::<libc::if_msghdr2>(),
            )
        }
        .to_vec()
    }

    #[test]
    fn parses_ifinfo2_and_skips_other_messages() {
        let mut buf = msg(3, libc::RTM_IFINFO2, 1000);
        buf.extend(msg(9, libc::RTM_NEWADDR, 7));
        buf.extend(msg(4, libc::RTM_IFINFO2, 2000));
        buf.extend([0u8; 3]); // truncated trailer
        let mut out = Vec::new();
        parse_iflist2(&buf, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].index, out[0].ibytes, out[0].obytes), (3, 1000, 500));
        assert_eq!((out[0].ipackets, out[0].opackets), (1, 0));
        assert_eq!(out[1].index, 4);
    }

    #[test]
    fn totals_wrap_per_counter_and_reset_as_a_whole() {
        let t = |ib, ob, ip, op| Totals {
            ibytes: ib,
            obytes: ob,
            ipackets: ip,
            opackets: op,
        };
        let prev = t(u64::from(u32::MAX) - 1023, 10, 100, 5);
        assert_eq!(
            t(1024, 2058, 160, 9).since(&prev),
            Some(t(2048, 2048, 60, 4)),
            "a 32-bit byte wrap"
        );
        assert_eq!(t(0, 0, 0, 0).since(&t(1 << 40, 0, 0, 0)), None);
        let mut sum = t(1, 2, 3, 4);
        sum.add(&t(10, 20, 30, 40));
        assert_eq!(sum, t(11, 22, 33, 44));
    }

    fn info(name: &str, kind: InterfaceKind) -> InterfaceInfo {
        InterfaceInfo {
            name: name.into(),
            kind,
        }
    }

    #[test]
    fn primary_is_the_reported_default_route_interface() {
        let reported = [
            info("en0", InterfaceKind::Wifi),
            info("en7", InterfaceKind::Ethernet),
        ];
        assert_eq!(pick_primary(Some("en7"), Some("en0"), &reported), Some(1));
        assert_eq!(
            pick_primary(None, Some("en0"), &reported),
            Some(0),
            "IPv6 when there is no IPv4 route"
        );
        assert_eq!(
            pick_primary(Some("utun3"), Some("en0"), &reported),
            None,
            "a VPN carries the route: no fallback to IPv6"
        );
        assert_eq!(pick_primary(None, None, &reported), None);
        assert_eq!(pick_primary(Some("en0"), None, &[]), None);
    }

    /// A collector reporting `names` (interface indices 1, 2, ...) without a probe.
    fn net_with(names: &[&str]) -> Network {
        let mut n = Network::new();
        for (i, name) in names.iter().enumerate() {
            let key = |id| SeriesKey::new(MetricId::from_static(id), Labels::single("iface", name));
            n.slots.push(Slot {
                index: i as u16 + 1,
                name: Arc::from(*name),
                rx: key("net.rx"),
                tx: key("net.tx"),
                link: key("net.link_rate"),
                prev: None,
            });
            n.infos.push(info(name, InterfaceKind::Ethernet));
        }
        n
    }

    fn counters(index: u16, ibytes: u64, obytes: u64) -> IfCounters {
        IfCounters {
            index,
            flags: libc::IFF_UP | libc::IFF_RUNNING,
            ibytes,
            obytes,
            ipackets: 0,
            opackets: 0,
            baudrate: 0,
        }
    }

    const SEC: u64 = 1_000_000_000;

    fn totals(buf: &SampleBuf) -> (Option<f32>, Option<f32>) {
        let bare = |id| SeriesKey::bare(MetricId::from_static(id));
        (
            buf.get(&bare("net.rx_total")),
            buf.get(&bare("net.tx_total")),
        )
    }

    #[test]
    fn totals_sum_the_reported_interfaces() {
        let mut n = net_with(&["en0", "en7"]);
        let mut buf = SampleBuf::new();
        n.counters = vec![counters(1, 1_000, 100), counters(2, 5_000, 500)];
        n.emit(SEC, false, &mut buf);
        assert_eq!(totals(&buf), (None, None), "no previous counters: a gap");
        buf.clear();
        n.counters = vec![counters(1, 3_000, 300), counters(2, 9_000, 600)];
        n.emit(3 * SEC, false, &mut buf);
        // en0: 1000/s down, 100/s up; en7: 2000/s down, 50/s up.
        assert_eq!(totals(&buf), (Some(3_000.0), Some(150.0)));
        // An interface the routing dump does not list (not yet re-probed) is no counter
        // here, so it is not part of the sum.
        buf.clear();
        n.counters.push(counters(9, 1 << 30, 1 << 30));
        n.counters[0] = counters(1, 4_000, 400);
        n.counters[1] = counters(2, 10_000, 700);
        n.emit(4 * SEC, false, &mut buf);
        assert_eq!(totals(&buf), (Some(2_000.0), Some(200.0)));
    }

    #[test]
    fn a_gap_in_any_part_is_a_gap_in_that_total_only() {
        let mut n = net_with(&["en0", "en7"]);
        let mut buf = SampleBuf::new();
        n.counters = vec![counters(1, 1_000, 100), counters(2, 5_000, 500)];
        n.emit(SEC, false, &mut buf);
        // en7's rx counter reset (a decrease above 32 bits); its tx moved normally.
        buf.clear();
        n.counters = vec![counters(1, 2_000, 200), counters(2, 5_000, 600)];
        n.slots[1].prev = Some(Totals {
            ibytes: 1 << 40,
            obytes: 500,
            ipackets: 0,
            opackets: 0,
        });
        n.emit(2 * SEC, false, &mut buf);
        assert_eq!(totals(&buf), (None, Some(200.0)), "rx and tx gated apart");
        // en7 went away: both totals are gaps until the re-probe drops it.
        buf.clear();
        n.counters = vec![counters(1, 3_000, 300)];
        n.emit(3 * SEC, false, &mut buf);
        assert_eq!(totals(&buf), (None, None));
        // The remainder's counter totals keep en0's bytes all the same.
        assert_eq!(buf.iface_net().map(|t| t.rx_bytes), Some(1_000));
    }

    #[test]
    fn no_reported_interface_means_no_totals() {
        let mut n = net_with(&[]);
        let mut buf = SampleBuf::new();
        n.emit(SEC, false, &mut buf);
        n.emit(2 * SEC, false, &mut buf);
        assert_eq!(totals(&buf), (None, None));
        assert!(buf.values().is_empty());
    }

    #[test]
    #[ignore = "reads live interface counters; run by hand on a Mac"]
    fn live_smoke() {
        let mut n = Network::new();
        let Probe::Supported(series) = n.probe() else {
            panic!("network unsupported")
        };
        println!("interfaces: {:?} ({} series)", n.interfaces(), series.len());
        let mut buf = SampleBuf::new();
        for i in 0..2 {
            buf.clear();
            let tick = Tick {
                n: i,
                wall_ms: 0,
                continuous_ns: super::super::sysctl::continuous_ns(),
                interval_ms: 1_000,
            };
            n.sample(&tick, &mut buf).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        for s in buf.values() {
            println!("{} = {:.0}", s.key, s.value);
        }
        println!("totals: {:?}", buf.iface_net());
        println!("primary: {:?}", buf.primary_iface());
    }
}
