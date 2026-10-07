use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::dictionary::CFDictionaryRef;

use super::ffi::*;
use super::ledger::*;
use super::*;

fn c(pid: i32, rx: u64, tx: u64) -> Counts {
    Counts {
        pid,
        upid: 0,
        rx,
        tx,
        counted: true,
    }
}

fn settle(l: &mut Ledger) -> Option<Vec<(i32, u64, u64)>> {
    settle_named(l).map(|v| v.into_iter().map(|(p, _, r, t)| (p, r, t)).collect())
}

/// A settled entry with its identity as a string: `(pid, identity, rx, tx)`.
type Named = (i32, Option<String>, u64, u64);

fn settle_named(l: &mut Ledger) -> Option<Vec<Named>> {
    let mut out = HashMap::new();
    let measured = l.settle(&mut out);
    let mut v: Vec<_> = out
        .into_iter()
        .map(|((p, _), s)| (p, s.ident.map(|a| a.to_string()), s.rx, s.tx))
        .collect();
    v.sort_unstable();
    measured.then_some(v)
}

/// `(pid, rx, tx, late rx, late tx)`: one settled entry.
type Late = (i32, u64, u64, u64, u64);

fn settle_late(l: &mut Ledger) -> Option<Vec<Late>> {
    let mut out = HashMap::new();
    let measured = l.settle(&mut out);
    let mut v: Vec<_> = out
        .into_iter()
        .map(|((p, _), s)| (p, s.rx, s.tx, s.late_rx, s.late_tx))
        .collect();
    v.sort_unstable();
    measured.then_some(v)
}

#[test]
fn the_first_settle_is_a_baseline_then_deltas_sum_per_pid() {
    let mut l = Ledger::default();
    l.added(1);
    l.added(2);
    l.added(3);
    l.counts(1, c(100, 5_000, 700));
    l.counts(2, c(100, 1_000, 0));
    l.counts(3, c(200, 9_999, 9_999));
    assert_eq!(
        settle(&mut l),
        None,
        "baseline: lifetime bytes are not a rate"
    );

    l.counts(1, c(100, 6_000, 800));
    l.counts(2, c(100, 1_500, 0));
    l.counts(3, c(200, 9_999, 9_999));
    assert_eq!(
        settle(&mut l),
        Some(vec![(100, 1_500, 100), (200, 0, 0)]),
        "two flows of one pid add up; an idle flow moves nothing"
    );
    assert_eq!(settle(&mut l), Some(vec![(100, 0, 0), (200, 0, 0)]));
}

#[test]
fn a_closed_flow_keeps_its_last_bytes() {
    let mut l = Ledger::default();
    l.added(1);
    l.counts(1, c(7, 1_000, 10));
    settle(&mut l);
    // It moves 4,000 more bytes, gets its final counts and is removed between
    // settles. The pointer is reused at once by another process's new flow.
    l.counts(1, c(7, 5_000, 10));
    l.removed(1);
    l.added(1);
    l.counts(1, c(8, 300, 30));
    assert_eq!(settle(&mut l), Some(vec![(7, 4_000, 0), (8, 300, 30)]));
    assert_eq!(
        settle(&mut l),
        Some(vec![(8, 0, 0)]),
        "the closed bytes went once"
    );
}

#[test]
fn a_flow_opened_and_closed_between_settles_counts_whole() {
    let mut l = Ledger::default();
    settle(&mut l);
    l.added(9);
    l.counts(9, c(3, 20_000, 500));
    l.removed(9);
    assert_eq!(settle(&mut l), Some(vec![(3, 20_000, 500)]));
    assert_eq!(l.len(), 0);
}

#[test]
fn flows_from_before_the_baseline_never_count_their_history() {
    let mut l = Ledger::default();
    // Added before the baseline but no counts yet: its first counts arrive later.
    l.added(1);
    settle(&mut l);
    l.counts(1, c(5, 80_000_000, 1_000));
    assert_eq!(settle(&mut l), Some(vec![(5, 0, 0)]));
    l.counts(1, c(5, 80_001_000, 1_000));
    assert_eq!(settle(&mut l), Some(vec![(5, 1_000, 0)]));
    // Removed before the baseline: nothing to fold.
    let mut l = Ledger::default();
    l.added(2);
    l.counts(2, c(6, 10, 10));
    l.removed(2);
    assert_eq!(settle(&mut l), None);
    assert_eq!(settle(&mut l), Some(vec![]));
}

/// Flows open before the manager report pid 0 until a description names them
/// (measured on macOS 27); their bytes never go to pid 0, and once the owner is known
/// the flow counts from there: the wait is not charged to one sample as a spike.
#[test]
fn a_flow_counts_from_when_its_owner_is_learned() {
    let mut l = Ledger::default();
    l.added(1);
    l.counts(1, c(0, 1_000, 0));
    assert_eq!(l.unresolved(), 1);
    assert_eq!(settle(&mut l), None);
    l.counts(1, c(0, 3_000, 0));
    assert_eq!(settle(&mut l), Some(vec![]), "nothing for pid 0");
    l.described(1, 42, 0);
    assert_eq!(l.unresolved(), 0);
    l.counts(1, c(0, 4_000, 0));
    assert_eq!(
        settle(&mut l),
        Some(vec![(42, 1_000, 0)]),
        "only what moved after the owner was known"
    );
    l.described(1, 0, 0);
    l.counts(1, c(0, 4_500, 0));
    assert_eq!(
        settle(&mut l),
        Some(vec![(42, 500, 0)]),
        "a later 0 is not an owner"
    );
}

/// A flow opened after the baseline that reports pid 0 at first: once a
/// description names its owner, what it moved meanwhile goes to the owner's byte
/// totals as late bytes, never to the rate (D-089 amending D-082).
#[test]
fn a_fresh_flow_named_late_keeps_its_bytes_out_of_the_rate() {
    let mut l = Ledger::default();
    settle(&mut l);
    l.added(1);
    l.counts(1, c(0, 30_000_000, 500));
    assert_eq!(settle_late(&mut l), Some(vec![]), "nothing for pid 0");
    l.counts(1, c(0, 31_000_000, 520));
    l.described(1, 42, 0);
    l.counts(1, c(0, 31_400_000, 600));
    assert_eq!(
        settle_late(&mut l),
        Some(vec![(42, 400_000, 80, 31_000_000, 520)]),
        "the rate is what moved after the owner was known; the rest is late"
    );
    l.counts(1, c(0, 31_500_000, 600));
    assert_eq!(
        settle_late(&mut l),
        Some(vec![(42, 100_000, 0, 0, 0)]),
        "late bytes go once: 31,000,000 + 400,000 + 100,000, all the flow moved"
    );

    // Named by its own counts, then closed before a settle: still conserved.
    l.added(2);
    l.counts(2, c(0, 7_000, 0));
    l.counts(2, c(43, 9_000, 0));
    l.removed(2);
    assert_eq!(
        settle_late(&mut l),
        Some(vec![(42, 0, 0, 0, 0), (43, 2_000, 0, 7_000, 0)])
    );
}

#[test]
fn an_uncounted_flow_named_late_charges_nothing() {
    let mut l = Ledger::default();
    settle(&mut l);
    l.added(1);
    l.counts(
        1,
        Counts {
            counted: false,
            ..c(0, 5_000, 5_000)
        },
    );
    settle(&mut l);
    l.described(1, 42, 0);
    l.counts(
        1,
        Counts {
            counted: false,
            ..c(0, 6_000, 6_000)
        },
    );
    assert_eq!(settle_late(&mut l), Some(vec![]));
}

/// The collector's rows: the rate is the interval's bytes over its length; late
/// bytes ride along for history and never reach it.
#[test]
fn late_bytes_are_carried_apart_from_the_rate() {
    let mut l = Ledger::default();
    settle(&mut l);
    l.added(1);
    l.counts(1, c(0, 30_000_000, 500));
    settle(&mut l);
    l.described(1, 42, 0);
    l.counts(1, c(0, 30_400_000, 600));
    let mut bytes = HashMap::new();
    assert!(l.settle(&mut bytes));
    let mut buf = SampleBuf::new();
    push_settled(&bytes, 2.0, &mut buf);
    let n = buf.process_net();
    assert_eq!(n.len(), 1);
    assert_eq!(
        (n[0].rx_bytes, n[0].tx_bytes, n[0].rx_bps, n[0].tx_bps),
        (400_000, 100, 200_000.0, 50.0)
    );
    assert_eq!((n[0].late_rx_bytes, n[0].late_tx_bytes), (30_000_000, 500));
}

#[test]
fn a_flow_from_before_the_baseline_named_late_drops_its_wait() {
    let mut l = Ledger::default();
    l.added(1);
    l.counts(1, c(0, 1_000, 0));
    settle(&mut l);
    l.counts(1, c(0, 50_000, 0));
    settle(&mut l);
    l.described(1, 42, 0);
    l.counts(1, c(0, 51_000, 0));
    assert_eq!(settle_late(&mut l), Some(vec![(42, 1_000, 0, 0, 0)]));
}

#[test]
fn an_owner_first_named_by_counts_is_charged_from_its_previous_counts() {
    let mut l = Ledger::default();
    l.added(1);
    l.counts(1, c(0, 1_000, 0));
    settle(&mut l);
    l.counts(1, c(0, 50_000, 0));
    assert_eq!(settle(&mut l), Some(vec![]));
    l.counts(1, c(42, 52_000, 10));
    assert_eq!(settle(&mut l), Some(vec![(42, 2_000, 10)]));
}

#[test]
fn a_description_before_the_baseline_keeps_the_baseline() {
    // The first sample describes, then settles: the baseline is the bytes then.
    let mut l = Ledger::default();
    l.added(1);
    l.counts(1, c(0, 1_000, 0));
    l.described(1, 42, 0);
    assert_eq!(settle(&mut l), None);
    l.counts(1, c(0, 1_500, 0));
    assert_eq!(settle(&mut l), Some(vec![(42, 500, 0)]));
}

fn cu(pid: i32, upid: u64, rx: u64, tx: u64) -> Counts {
    Counts {
        upid,
        ..c(pid, rx, tx)
    }
}

fn owner(pid: i32, upid: u64) -> Owner {
    Owner { pid, upid }
}

/// The first callback that names an owner asks for its identity once; the owner's
/// later flows take it from the cache, and every entry of one name shares one
/// allocation.
#[test]
fn an_identity_is_resolved_once_per_process() {
    let now = Instant::now();
    let mut l = Ledger::with_room();
    settle(&mut l);
    l.added(1);
    assert_eq!(l.counts(1, cu(7, 70, 100, 0)), Some(owner(7, 70)));
    l.identify(1, owner(7, 70), Some("Google Chrome"), now);
    assert_eq!(l.counts(1, cu(7, 70, 200, 0)), None, "already known");
    l.added(2);
    assert_eq!(l.counts(2, cu(7, 70, 50, 0)), None, "cached by unique id");
    // Another helper of the same app: its own lookup, the same interned name.
    l.added(3);
    assert_eq!(l.counts(3, cu(8, 80, 5, 5)), Some(owner(8, 80)));
    l.identify(3, owner(8, 80), Some("Google Chrome"), now);
    let mut out = HashMap::new();
    assert!(l.settle(&mut out));
    let names: Vec<_> = out.values().map(|s| s.ident.clone().unwrap()).collect();
    assert_eq!(names.len(), 2, "one entry per pid");
    assert!(Arc::ptr_eq(&names[0], &names[1]));
    let mut got: Vec<_> = out.into_iter().map(|((p, _), s)| (p, s.rx, s.tx)).collect();
    got.sort_unstable();
    assert_eq!(got, vec![(7, 250, 0), (8, 5, 5)]);
}

/// A short-lived process: its flow opens, moves bytes, gets its final counts and is
/// removed, and the process exits, all between two settles. Its bytes keep the name
/// resolved at the first callback.
#[test]
fn a_process_that_exits_between_samples_keeps_its_identity() {
    let mut l = Ledger::with_room();
    settle(&mut l);
    l.added(4);
    let need = l.counts(4, cu(900, 9_000, 1_000, 10)).unwrap();
    l.identify(4, need, Some("curl"), Instant::now());
    l.counts(4, cu(900, 9_000, 4_000_000, 600));
    l.removed(4);
    assert_eq!(
        settle_named(&mut l),
        Some(vec![(900, Some("curl".into()), 4_000_000, 600)])
    );
    assert_eq!(settle_named(&mut l), Some(vec![]), "counted once");
}

/// A reused pid is a new process: a new lookup, and its bytes stay apart from the
/// old process's in the same settle.
#[test]
fn a_reused_pid_is_named_again() {
    let now = Instant::now();
    let mut l = Ledger::with_room();
    settle(&mut l);
    l.added(1);
    let need = l.counts(1, cu(5, 50, 100, 0)).unwrap();
    l.identify(1, need, Some("curl"), now);
    l.removed(1);
    l.added(1);
    assert_eq!(l.counts(1, cu(5, 51, 30, 0)), Some(owner(5, 51)));
    l.identify(1, owner(5, 51), Some("git"), now);
    assert_eq!(
        settle_named(&mut l),
        Some(vec![
            (5, Some("curl".into()), 100, 0),
            (5, Some("git".into()), 30, 0)
        ])
    );
}

#[test]
fn an_unnamed_owner_still_counts_and_is_not_looked_up_again() {
    let mut l = Ledger::with_room();
    settle(&mut l);
    l.added(1);
    let need = l.counts(1, cu(3, 30, 10, 0)).unwrap();
    l.identify(1, need, None, Instant::now());
    assert_eq!(l.counts(1, cu(3, 30, 20, 0)), None);
    assert_eq!(settle_named(&mut l), Some(vec![(3, None, 20, 0)]));
}

#[test]
fn new_names_are_capped_per_hour() {
    let t0 = Instant::now();
    let mut l = Ledger::with_room();
    settle(&mut l);
    let n = NEW_NAMES_PER_HOUR as usize;
    for i in 0..=n {
        let src = i + 1;
        let pid = i as i32 + 1;
        l.added(src);
        let need = l.counts(src, cu(pid, pid as u64, 1, 0)).unwrap();
        l.identify(src, need, Some(&format!("app{i}")), t0);
    }
    let out = settle_named(&mut l).unwrap();
    assert_eq!(out.iter().filter(|r| r.1.is_some()).count(), n);
    assert_eq!(
        out.iter().find(|r| r.0 == n as i32 + 1).unwrap().1,
        None,
        "over the cap: other apps"
    );
    // A known name is not new; an hour later new names are taken again.
    l.added(10_000);
    let need = l.counts(10_000, cu(10_000, 10_000, 1, 0)).unwrap();
    l.identify(10_000, need, Some("app0"), t0);
    l.added(10_001);
    let need = l.counts(10_001, cu(10_001, 10_001, 1, 0)).unwrap();
    l.identify(10_001, need, Some("late"), t0 + HOUR);
    let out = settle_named(&mut l).unwrap();
    let name = |pid| out.iter().find(|r| r.0 == pid).unwrap().1.clone();
    assert_eq!(name(10_000).as_deref(), Some("app0"));
    assert_eq!(name(10_001).as_deref(), Some("late"));
}

/// Names of processes with no open flow are forgotten once the cache is full, and
/// a name nothing uses leaves the intern table.
#[test]
fn names_of_gone_processes_are_pruned() {
    let now = Instant::now();
    let mut l = Ledger::with_room();
    settle(&mut l);
    for i in 0..=PID_ROOM {
        let pid = i as i32 + 1;
        l.added(i);
        let need = l.counts(i, cu(pid, pid as u64, 1, 0)).unwrap();
        // Many processes, few apps, as with browser helpers.
        l.identify(i, need, Some(&format!("p{}", i % 4)), now);
        if i > 0 {
            l.removed(i);
        }
    }
    settle(&mut l);
    assert_eq!(l.names.by_owner.len(), 1, "only the open flow's owner");
    // The closed bytes still held their names during that prune; the next one
    // drops them.
    assert_eq!(l.names.interned.len(), 4);
    l.names.prune_at = 0;
    settle(&mut l);
    assert_eq!(l.names.interned.len(), 1);
    assert!(l.names.interned.contains("p0"));
    assert_eq!(l.names.prune_at, PID_ROOM);
}

/// A query whose completion never arrives must not make later queries wait for a
/// count that is one ahead forever.
#[test]
fn a_lost_completion_does_not_stall_later_waits() {
    let done = Done::default();
    let mark = done.mark();
    assert!(
        !done.wait_past(mark, Duration::from_millis(5)),
        "never completed"
    );
    let mark = done.mark();
    done.complete();
    assert!(done.wait_past(mark, Duration::from_millis(5)));
    let mark = done.mark();
    assert!(
        !done.wait_past(mark, Duration::from_millis(5)),
        "not yet again"
    );
}

#[test]
fn a_completion_from_another_thread_ends_the_wait() {
    let done = Arc::new(Done::default());
    let mark = done.mark();
    let d = Arc::clone(&done);
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        d.complete();
    });
    assert!(done.wait_past(mark, Duration::from_secs(5)));
    t.join().unwrap();
}

#[test]
fn repeated_failures_back_off_until_the_period_passes() {
    let sec = 1_000_000_000u64;
    let backoff = BACKOFF.as_secs() * sec;
    let mut b = Breaker::default();
    for i in 0..FAIL_LIMIT - 1 {
        b.failed(i as u64 * sec);
        assert!(b.allows(i as u64 * sec + 1), "failure {i} retries at once");
    }
    let t = 10 * sec;
    b.failed(t);
    assert!(!b.allows(t + 1));
    assert!(!b.allows(t + backoff - 1));
    assert!(b.allows(t + backoff));
    // Still failing after the backoff: one attempt, then the next backoff.
    b.failed(t + backoff);
    assert!(!b.allows(t + backoff + 1));
    assert!(b.allows(t + 2 * backoff));
    // A sample that worked clears it.
    b.succeeded();
    b.failed(t + 2 * backoff);
    assert!(b.allows(t + 2 * backoff + 1));
}

#[test]
fn uncounted_flows_charge_nothing() {
    let mut l = Ledger::default();
    settle(&mut l);
    l.added(1);
    l.counts(
        1,
        Counts {
            counted: false,
            ..c(4, 1 << 30, 1 << 30)
        },
    );
    l.added(2);
    l.counts(2, c(4, 10, 20));
    l.removed(1);
    assert_eq!(settle(&mut l), Some(vec![(4, 10, 20)]));
}

#[test]
fn a_counter_going_backwards_is_not_a_huge_rate() {
    let mut l = Ledger::default();
    l.added(1);
    l.counts(1, c(1, 1_000, 1_000));
    settle(&mut l);
    l.counts(1, c(1, 10, 10));
    assert_eq!(settle(&mut l), Some(vec![(1, 0, 0)]));
}

/// A counts dictionary the way NetworkStatistics shaped it on macOS 27 (D-081): SInt64
/// numbers, interface-type flags only when true, plus keys the parser ignores.
struct Fixture {
    names: [CFString; 7],
}

impl Fixture {
    fn new() -> Self {
        Self {
            names: [
                "processID",
                "rxBytes",
                "txBytes",
                "ifLoopback",
                "uniqueProcessID",
                "processName",
                "interface",
            ]
            .map(CFString::new),
        }
    }

    fn keys(&self) -> Keys {
        let k = |i: usize| Key(self.names[i].as_concrete_TypeRef());
        Keys {
            pid: k(0),
            rx: k(1),
            tx: k(2),
            loopback: k(3),
            upid: Some(k(4)),
            name: Some(k(5)),
            interface: k(6),
        }
    }

    fn with<T>(&self, pairs: &[(&str, CFType)], f: impl FnOnce(CFDictionaryRef, &Keys) -> T) -> T {
        let pairs: Vec<(CFString, CFType)> = pairs
            .iter()
            .map(|(k, v)| (CFString::new(k), v.clone()))
            .collect();
        let dict = CFDictionary::from_CFType_pairs(&pairs);
        f(dict.as_concrete_TypeRef(), &self.keys())
    }

    fn parse(&self, pairs: &[(&str, CFType)]) -> Option<Counts> {
        // SAFETY: a valid dictionary alive for the call; keys alive with `self`.
        self.with(pairs, |d, k| unsafe { parse_counts(d, k, |i| i == EN0) })
    }
}

/// The interface index the fixtures' `reported` accepts, as en0 is on the dev Mac.
const EN0: u32 = 14;

fn n(v: i64) -> CFType {
    CFNumber::from(v).as_CFType()
}

#[test]
fn parses_a_counts_dictionary() {
    let f = Fixture::new();
    let base = [
        ("processID", n(1912)),
        ("uniqueProcessID", n(1_032_823)),
        ("processName", CFString::new("curl").as_CFType()),
        ("provider", CFString::new("TCP").as_CFType()),
        ("rxBytes", n(24_527_869)),
        ("txBytes", n(520)),
        ("rxWiFiBytes", n(24_527_869)),
        ("ifWiFi", CFBoolean::true_value().as_CFType()),
        ("interface", n(i64::from(EN0))),
    ];
    assert_eq!(
        f.parse(&base),
        Some(Counts {
            upid: 1_032_823,
            ..c(1912, 24_527_869, 520)
        })
    );
    // SAFETY: a valid dictionary alive for the call.
    let recorded = f.with(&base, |d, k| unsafe { recorded_identity(d, k) });
    assert_eq!(recorded.as_deref(), Some("curl"));

    let mut not_lo = base.to_vec();
    not_lo.push(("ifLoopback", CFBoolean::false_value().as_CFType()));
    assert!(f.parse(&not_lo).unwrap().counted);
}

/// Regression: apps summed to 30x the interface on the Network page, shares past
/// 1,500%, because flows that never touch a reported interface were counted. The
/// dictionaries are the ones macOS 27 sent for a transfer to this Mac's own
/// Tailscale address and over 127.0.0.1.
#[test]
fn counts_only_flows_on_a_reported_interface() {
    let f = Fixture::new();
    let flow = |extra: &[(&str, CFType)]| {
        let mut pairs = vec![
            ("processID", n(1912)),
            ("rxBytes", n(20_971_520)),
            ("txBytes", n(0)),
        ];
        pairs.extend_from_slice(extra);
        f.parse(&pairs).unwrap().counted
    };
    let yes = || CFBoolean::true_value().as_CFType();
    assert!(flow(&[("ifWiFi", yes()), ("interface", n(i64::from(EN0)))]));
    // A tunnel reports the type of the link under it: utun4 says Wi-Fi.
    assert!(!flow(&[("ifWiFi", yes()), ("interface", n(23))]));
    assert!(!flow(&[("ifLoopback", yes()), ("interface", n(1))]));
    // Flagged loopback on a reported index still is not counted.
    assert!(!flow(&[
        ("ifLoopback", yes()),
        ("interface", n(i64::from(EN0)))
    ]));
    // No index, or 0: nothing says the bytes are in the interface totals.
    assert!(!flow(&[("ifWiFi", yes())]));
    assert!(!flow(&[("ifWiFi", yes()), ("interface", n(0))]));
    assert!(!flow(&[
        ("ifWiFi", yes()),
        ("interface", CFString::new("en0").as_CFType())
    ]));
}

#[test]
fn rejects_missing_or_mistyped_values() {
    let f = Fixture::new();
    let no_pid = [("rxBytes", n(1)), ("txBytes", n(1))];
    assert_eq!(f.parse(&no_pid), None);
    let text_rx = [
        ("processID", n(1)),
        ("rxBytes", CFString::new("1").as_CFType()),
        ("txBytes", n(1)),
    ];
    assert_eq!(f.parse(&text_rx), None);
    let negative = [("processID", n(1)), ("rxBytes", n(-5)), ("txBytes", n(1))];
    assert_eq!(f.parse(&negative), None);
    let huge_pid = [
        ("processID", n(i64::from(i32::MAX) + 1)),
        ("rxBytes", n(1)),
        ("txBytes", n(1)),
    ];
    assert_eq!(f.parse(&huge_pid), None);
    // SAFETY: null is allowed.
    assert_eq!(
        unsafe { parse_counts(std::ptr::null(), &f.keys(), |_| true) },
        None
    );
}

/// Reads the live framework while the machine moves traffic, and prints the top
/// processes by bytes over a few seconds next to the closed-flow and fresh-flow
/// counts. Compare with `nettop -P -L 1 -J bytes_in,bytes_out` run over the same span
/// (it reports cumulative totals; take two and subtract).
#[test]
#[ignore = "reads the live NetworkStatistics framework; run by hand with --ignored --nocapture"]
fn live_top_processes() {
    let mut c = NetPerProcess::new();
    assert!(matches!(c.probe(), Probe::Supported(_)), "API unavailable");
    let secs: u64 = std::env::var("NSTAT_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let tick = |n| Tick {
        n,
        wall_ms: 0,
        continuous_ns: super::super::sysctl::continuous_ns(),
        interval_ms: 1_000,
    };
    let mut buf = SampleBuf::new();
    let t0 = Instant::now();
    c.sample(&tick(0), &mut buf).unwrap();
    println!("create + baseline: {:?}", t0.elapsed());
    assert!(
        !buf.process_net_measured(),
        "the first sample is a baseline"
    );
    let mut totals: HashMap<i32, (f64, f64)> = HashMap::new();
    for i in 1..=secs {
        std::thread::sleep(Duration::from_secs(1));
        buf.clear();
        let t = Instant::now();
        c.sample(&tick(i), &mut buf).unwrap();
        let took = t.elapsed();
        assert!(buf.process_net_measured());
        for n in buf.process_net() {
            let e = totals.entry(n.pid).or_default();
            e.0 += f64::from(n.rx_bps);
            e.1 += f64::from(n.tx_bps);
        }
        println!(
            "sample {i}: {took:?}, {} pids with traffic",
            buf.process_net().len()
        );
    }
    let mut top: Vec<_> = totals.into_iter().collect();
    top.sort_by(|a, b| (b.1.0 + b.1.1).total_cmp(&(a.1.0 + a.1.1)));
    println!("top processes, mean bytes/s over {secs} s:");
    for (pid, (rx, tx)) in top.iter().take(10) {
        let name = super::super::libproc::bsd_info(*pid)
            .map(|b| super::super::libproc::name_of(&b).into_owned())
            .unwrap_or_default();
        println!(
            "  {pid:>6} {name:<28} rx {:>12.0} tx {:>12.0}",
            rx / secs as f64,
            tx / secs as f64
        );
    }
    c.release();
    assert!(!c.is_open());
}

/// A `curl` that downloads 4 MB and exits between two samples: the second sample
/// must still charge its bytes to "curl" (the identity resolved while it ran, or
/// the recorded process name once it was gone). Needs internet access.
#[test]
#[ignore = "reads the live NetworkStatistics framework and downloads 4 MB; run by hand with --ignored --nocapture"]
fn live_short_curl_keeps_its_identity() {
    let mut c = NetPerProcess::new();
    assert!(matches!(c.probe(), Probe::Supported(_)), "API unavailable");
    let tick = |n| Tick {
        n,
        wall_ms: 0,
        continuous_ns: super::super::sysctl::continuous_ns(),
        interval_ms: 1_000,
    };
    let mut buf = SampleBuf::new();
    c.sample(&tick(0), &mut buf).unwrap();
    assert!(!buf.process_net_measured(), "baseline");
    let t = Instant::now();
    let status = std::process::Command::new("/usr/bin/curl")
        .args([
            "-sS",
            "-o",
            "/dev/null",
            "https://speed.cloudflare.com/__down?bytes=4000000",
        ])
        .status()
        .unwrap();
    assert!(status.success(), "curl failed: {status}");
    println!("curl ran {:?} and exited", t.elapsed());
    // Its flows' final counts and removals arrive on the framework's queue.
    std::thread::sleep(Duration::from_secs(2));
    buf.clear();
    c.sample(&tick(1), &mut buf).unwrap();
    let interval = buf.process_net_interval().unwrap();
    assert!(interval.now_ns > interval.prev_ns);
    for n in buf.process_net() {
        println!(
            "  pid {:>6} {:<28} rx {:>10} tx {:>8}",
            n.pid,
            n.identity.as_deref().unwrap_or("-"),
            n.rx_bytes,
            n.tx_bytes
        );
    }
    let curl: u64 = buf
        .process_net()
        .iter()
        .filter(|n| n.identity.as_deref() == Some("curl"))
        .map(|n| n.rx_bytes + n.late_rx_bytes)
        .sum();
    println!("curl rx {curl} bytes");
    assert!(curl > 0, "no bytes charged to curl");
    c.release();
}

/// Regression for apps exceeding the interface: moves 20 MB to this process over
/// 127.0.0.1, ::1 and every IPv4 address on an interface the network collector does
/// not report (a Tailscale `utun`, a VM bridge), and requires none of it charged to
/// this process. Before the interface check, the tunnel's 20 MB was charged in both
/// directions and one run in four charged a loopback receiver.
#[test]
#[ignore = "reads the live NetworkStatistics framework; run by hand with --ignored --nocapture"]
fn live_traffic_off_the_reported_interfaces_is_not_charged() {
    use std::io::{Read, Write};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};

    let mut addrs = vec![
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ];
    // SAFETY: the list is freed below, once.
    let list = unsafe { libc::if_nameindex() };
    assert!(!list.is_null());
    let mut cur = list;
    // SAFETY: `if_nameindex` returns an array ended by a zero index and null name.
    while unsafe { (*cur).if_index } != 0 {
        // SAFETY: as above; `cur` is before the end entry.
        let (index, name) = unsafe { ((*cur).if_index, CStr::from_ptr((*cur).if_name)) };
        let name = name.to_string_lossy();
        if !network::is_reported_interface(index) && !name.starts_with("lo") {
            let found = super::super::ifaddrs::interface_addresses(&name);
            addrs.extend(found.ipv4.into_iter().map(IpAddr::V4));
        }
        // SAFETY: not past the end entry.
        cur = unsafe { cur.add(1) };
    }
    // SAFETY: from `if_nameindex` above.
    unsafe { libc::if_freenameindex(list) };

    let me = i32::try_from(std::process::id()).unwrap();
    let mut c = NetPerProcess::new();
    assert!(matches!(c.probe(), Probe::Supported(_)), "API unavailable");
    let tick = |n| Tick {
        n,
        wall_ms: 0,
        continuous_ns: super::super::sysctl::continuous_ns(),
        interval_ms: 1_000,
    };
    let mut buf = SampleBuf::new();
    c.sample(&tick(0), &mut buf).unwrap();
    let chunk = [7u8; 64 * 1024];
    for (i, addr) in addrs.iter().enumerate() {
        let Ok(listener) = TcpListener::bind((*addr, 0)) else {
            println!("{addr}: cannot listen, skipped");
            continue;
        };
        let to = listener.local_addr().unwrap();
        let Ok(mut s) = TcpStream::connect(to) else {
            println!("{addr}: cannot connect, skipped");
            continue;
        };
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut b = [0u8; 64 * 1024];
            while matches!(conn.read(&mut b), Ok(n) if n > 0) {}
        });
        for _ in 0..320 {
            s.write_all(&chunk).unwrap();
        }
        drop(s);
        server.join().unwrap();
        // Final counts arrive on the framework's queue a moment after the close.
        std::thread::sleep(Duration::from_secs(3));
        buf.clear();
        c.sample(&tick(i as u64 + 1), &mut buf).unwrap();
        let charged: u64 = buf
            .process_net()
            .iter()
            .filter(|n| n.pid == me)
            .map(|n| n.rx_bytes + n.tx_bytes + n.late_rx_bytes + n.late_tx_bytes)
            .sum();
        println!("{addr}: 20 MB moved, {charged} bytes charged");
        assert_eq!(
            charged, 0,
            "traffic over {addr} was charged to this process"
        );
    }
    c.release();
}

/// Opens and destroys managers in a tight loop while loopback connections open,
/// move data and close in another thread, so added, counts and removed callbacks are
/// queued when each manager is destroyed. Half the iterations destroy right after
/// `NStatManagerAddAll*`, with the added callbacks for every flow still pending.
/// Pass: no crash, every iteration starts. `NSTAT_CHURN` sets the iterations.
#[test]
#[ignore = "drives the live NetworkStatistics framework; run by hand with --ignored --nocapture"]
fn live_open_release_churn() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    let iterations: u64 = std::env::var("NSTAT_CHURN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let api = api().expect("API unavailable");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let moved = Arc::new(AtomicU64::new(0));
    let server = std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { break };
            std::thread::spawn(move || {
                let mut buf = [0u8; 64 * 1024];
                while matches!(conn.read(&mut buf), Ok(n) if n > 0) {}
            });
        }
    });
    let client = {
        let (stop, moved) = (Arc::clone(&stop), Arc::clone(&moved));
        std::thread::spawn(move || {
            let chunk = [7u8; 64 * 1024];
            while !stop.load(Ordering::Relaxed) {
                // A new flow every few chunks, so sources come and go throughout.
                let Ok(mut s) = TcpStream::connect(addr) else {
                    continue;
                };
                for _ in 0..8 {
                    if s.write_all(&chunk).is_err() {
                        break;
                    }
                    moved.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                }
            }
        })
    };
    let mut c = NetPerProcess::new();
    assert!(matches!(c.probe(), Probe::Supported(_)));
    let mut buf = SampleBuf::new();
    let t0 = Instant::now();
    let mut timeouts = 0;
    for i in 0..iterations {
        if i % 2 == 0 {
            drop(Session::start(api).unwrap());
        } else {
            buf.clear();
            let tick = Tick {
                n: i,
                wall_ms: 0,
                continuous_ns: super::super::sysctl::continuous_ns(),
                interval_ms: 1_000,
            };
            match c.sample(&tick, &mut buf) {
                Ok(()) => {}
                Err(CollectError::Timeout { .. }) => timeouts += 1,
                Err(e) => panic!("iteration {i}: {e}"),
            }
            c.release();
        }
    }
    let took = t0.elapsed();
    stop.store(true, Ordering::Relaxed);
    client.join().unwrap();
    // The server thread blocks in accept; it ends with the test process.
    drop(server);
    println!(
        "{iterations} open/destroy cycles in {took:?}, {timeouts} query timeouts, {} MB over loopback",
        moved.load(Ordering::Relaxed) / 1_000_000
    );
    assert!(moved.load(Ordering::Relaxed) > 0, "no traffic flowed");
}
