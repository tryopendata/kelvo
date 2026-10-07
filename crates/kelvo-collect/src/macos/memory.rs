//! Memory composition, pressure and swap.
//!
//! Composition follows Activity Monitor's definitions (the same ones Stats uses):
//!
//! | Metric | Pages |
//! |---|---|
//! | `mem.app` | internal - purgeable |
//! | `mem.wired` | wired |
//! | `mem.compressed` | occupied by compressor |
//! | `mem.used` | app + wired + compressed ("Memory Used") |
//! | `mem.cached` | external (file-backed) + purgeable ("Cached Files") |
//! | `mem.free` | total - used - cached, floored at 0 |
//!
//! `mem.free` is the remainder rather than `free_count`, so used + cached + free adds up
//! to physical memory for the Overview composition bar.
//!
//! Pressure (open question Q2): `mem.pressure` is `100 - kern.memorystatus_level`. Apple's own
//! `memory_pressure` tool prints `kern.memorystatus_level` as "System-wide memory free
//! percentage", so 100 minus it is the used share the kernel's pressure logic works from.
//! `mem.pressure_level` maps `kern.memorystatus_vm_pressure_level` (1 normal, 2 warn,
//! 4 critical, the `NOTE_MEMORYSTATUS_PRESSURE_*` values) to 0, 1, 2.

use kelvo_schema::{Entitlement, MetricCode, MetricId, Module, PressureLevel, SeriesKey};

use super::sysctl;
use crate::{Cadence, CollectError, Collector, CollectorId, Probe, SampleBuf, Tick};

struct Keys {
    used: SeriesKey,
    app: SeriesKey,
    wired: SeriesKey,
    compressed: SeriesKey,
    cached: SeriesKey,
    free: SeriesKey,
    pressure: SeriesKey,
    pressure_level: SeriesKey,
    swap_used: SeriesKey,
    swap_in: SeriesKey,
    swap_out: SeriesKey,
}

impl Keys {
    fn new() -> Self {
        let k = |id| SeriesKey::bare(MetricId::from_static(id));
        Self {
            used: k("mem.used"),
            app: k("mem.app"),
            wired: k("mem.wired"),
            compressed: k("mem.compressed"),
            cached: k("mem.cached"),
            free: k("mem.free"),
            pressure: k("mem.pressure"),
            pressure_level: k("mem.pressure_level"),
            swap_used: k("mem.swap_used"),
            swap_in: k("mem.swap_in"),
            swap_out: k("mem.swap_out"),
        }
    }

    fn all(&self) -> Vec<SeriesKey> {
        [
            &self.used,
            &self.app,
            &self.wired,
            &self.compressed,
            &self.cached,
            &self.free,
            &self.pressure,
            &self.pressure_level,
            &self.swap_used,
            &self.swap_in,
            &self.swap_out,
        ]
        .into_iter()
        .cloned()
        .collect()
    }
}

/// Page counts the composition is computed from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Pages {
    internal: u64,
    purgeable: u64,
    wired: u64,
    compressor: u64,
    external: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Composition {
    used: u64,
    app: u64,
    wired: u64,
    compressed: u64,
    cached: u64,
    free: u64,
}

fn composition(p: Pages, page_size: u64, total: u64) -> Composition {
    let b = |pages: u64| pages.saturating_mul(page_size);
    let app = b(p.internal.saturating_sub(p.purgeable));
    let wired = b(p.wired);
    let compressed = b(p.compressor);
    let used = app + wired + compressed;
    let cached = b(p.external + p.purgeable);
    Composition {
        used,
        app,
        wired,
        compressed,
        cached,
        free: total.saturating_sub(used).saturating_sub(cached),
    }
}

/// `kern.memorystatus_vm_pressure_level` to the catalog's codes.
fn pressure_level(raw: i64) -> Option<f32> {
    let level = match raw {
        1 => PressureLevel::Normal,
        2 => PressureLevel::Warn,
        4 => PressureLevel::Critical,
        _ => return None,
    };
    Some(level.value())
}

pub struct Memory {
    keys: Keys,
    page_size: u64,
    total: u64,
    /// Previous swapins, swapouts and continuous time.
    prev: Option<(u64, u64, u64)>,
}

impl Default for Memory {
    fn default() -> Self {
        Self::new()
    }
}

impl Memory {
    pub fn new() -> Self {
        Self {
            keys: Keys::new(),
            page_size: 0,
            total: 0,
            prev: None,
        }
    }
}

fn vm_stats() -> Result<libc::vm_statistics64, CollectError> {
    // SAFETY: an all-zero vm_statistics64 is valid (plain integers).
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    crate::calls::count(crate::calls::Api::Kernel);
    // SAFETY: `stats` is a writable vm_statistics64 and `count` is its size in
    // integer_t units, so the kernel writes at most that much.
    let kr = unsafe {
        libc::host_statistics64(
            mach2::mach_init::mach_host_self(),
            libc::HOST_VM_INFO64,
            (&mut stats as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    if kr == libc::KERN_SUCCESS {
        Ok(stats)
    } else {
        Err(CollectError::Os {
            call: "host_statistics64",
            code: i64::from(kr),
        })
    }
}

impl Collector for Memory {
    fn id(&self) -> CollectorId {
        CollectorId("memory")
    }

    fn cadence(&self) -> Cadence {
        // Every tick, also with nothing on screen: memory figures are instantaneous gauges, so a
        // 10 s idle period would leave one point per S10 bucket and an inexact M1 average.
        // Only counter-derived collectors use LIVE_OR_IDLE (D-070).
        Cadence::EveryTick
    }

    fn required_entitlements(&self) -> &'static [Entitlement] {
        &[Entitlement::None]
    }

    fn modules(&self) -> &'static [Module] {
        &[Module::Memory]
    }

    fn probe(&mut self) -> Probe {
        self.page_size = sysctl::int(c"hw.pagesize").unwrap_or(0).max(0) as u64;
        self.total = sysctl::int(c"hw.memsize").unwrap_or(0).max(0) as u64;
        self.prev = None;
        if self.page_size == 0 || self.total == 0 || vm_stats().is_err() {
            return Probe::Unsupported {
                reason: kelvo_schema::UnsupportedReason::NoHardware,
            };
        }
        Probe::Supported(self.keys.all())
    }

    fn sample(&mut self, tick: &Tick, out: &mut SampleBuf) -> Result<(), CollectError> {
        if self.page_size == 0 {
            return Err(CollectError::NotProbed);
        }
        let s = vm_stats()?;
        let c = composition(
            Pages {
                internal: u64::from(s.internal_page_count),
                purgeable: u64::from(s.purgeable_count),
                wired: u64::from(s.wire_count),
                compressor: u64::from(s.compressor_page_count),
                external: u64::from(s.external_page_count),
            },
            self.page_size,
            self.total,
        );
        let k = &self.keys;
        out.push(&k.used, c.used as f32);
        out.push(&k.app, c.app as f32);
        out.push(&k.wired, c.wired as f32);
        out.push(&k.compressed, c.compressed as f32);
        out.push(&k.cached, c.cached as f32);
        out.push(&k.free, c.free as f32);

        if let Some(level) = sysctl::int(c"kern.memorystatus_level") {
            out.push(&k.pressure, (100 - level.clamp(0, 100)) as f32);
        }
        if let Some(level) =
            sysctl::int(c"kern.memorystatus_vm_pressure_level").and_then(pressure_level)
        {
            out.push(&k.pressure_level, level);
        }
        if let Some(swap) = sysctl::swap_usage() {
            out.push(&k.swap_used, swap.xsu_used as f32);
        }

        let now = tick.continuous_ns;
        if let Some((pin, pout, pt)) = self.prev
            && now > pt
        {
            let secs = (now - pt) as f64 / 1e9;
            let rate = |a: u64, b: u64| (a.saturating_sub(b) as f64 / secs) as f32;
            out.push(&k.swap_in, rate(s.swapins, pin));
            out.push(&k.swap_out, rate(s.swapouts, pout));
        }
        self.prev = Some((s.swapins, s.swapouts, now));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_adds_up_to_total() {
        let p = Pages {
            internal: 100,
            purgeable: 10,
            wired: 20,
            compressor: 5,
            external: 30,
        };
        let c = composition(p, 16384, 200 * 16384);
        assert_eq!(c.app, 90 * 16384);
        assert_eq!(c.used, 115 * 16384);
        assert_eq!(c.cached, 40 * 16384);
        assert_eq!(c.used + c.cached + c.free, 200 * 16384);
    }

    #[test]
    fn free_floors_at_zero() {
        let p = Pages {
            internal: 100,
            external: 100,
            ..Pages::default()
        };
        assert_eq!(composition(p, 1, 150).free, 0);
    }

    #[test]
    fn pressure_levels_map_to_catalog_codes() {
        assert_eq!(pressure_level(1), Some(0.0));
        assert_eq!(pressure_level(2), Some(1.0));
        assert_eq!(pressure_level(4), Some(2.0));
        assert_eq!(pressure_level(0), None);
    }

    #[test]
    #[ignore = "reads live VM statistics; run by hand on a Mac"]
    fn live_smoke() {
        let mut m = Memory::new();
        assert!(matches!(m.probe(), Probe::Supported(_)));
        let mut buf = SampleBuf::new();
        for n in 0..2 {
            buf.clear();
            let tick = Tick {
                n,
                wall_ms: 0,
                continuous_ns: sysctl::continuous_ns(),
                interval_ms: 1_000,
            };
            m.sample(&tick, &mut buf).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        for s in buf.values() {
            let gib = s.value as f64 / (1u64 << 30) as f64;
            println!("{} = {} ({gib:.2} GiB)", s.key, s.value);
        }
        assert!(buf.get(&m.keys.swap_in).is_some());
    }
}
