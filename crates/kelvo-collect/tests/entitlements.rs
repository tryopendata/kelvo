//! The `appstore` gating (architecture.md, Platform and entitlement gating): with only
//! `Entitlement::None` allowed, collectors on private APIs are replaced by stand-ins that
//! report `MissingEntitlement`, and public-API collectors pass through untouched.
//!
//! The private-API collectors are fakes declaring the entitlements the real IOReport,
//! SMC, HID and NetworkStatistics collectors declare, so this test does not depend on
//! which of them are registered yet; `platform_collectors_need_only_public_apis_or_declare`
//! covers the real list.

use kelvo_collect::{
    Cadence, CollectError, Collector, CollectorId, Entitlement, Module, Probe, SampleBuf, Tick,
    UnsupportedReason, allowed_entitlements, filter_by_entitlements, platform_collectors,
};

struct Fake {
    id: &'static str,
    needs: &'static [Entitlement],
    modules: &'static [Module],
}

impl Collector for Fake {
    fn id(&self) -> CollectorId {
        CollectorId(self.id)
    }
    fn cadence(&self) -> Cadence {
        Cadence::EveryTick
    }
    fn required_entitlements(&self) -> &'static [Entitlement] {
        self.needs
    }
    fn modules(&self) -> &'static [Module] {
        self.modules
    }
    fn probe(&mut self) -> Probe {
        Probe::Supported(Vec::new())
    }
    fn sample(&mut self, _: &Tick, _: &mut SampleBuf) -> Result<(), CollectError> {
        Ok(())
    }
}

fn fakes() -> Vec<Box<dyn Collector>> {
    let f = |id, needs, modules| Box::new(Fake { id, needs, modules }) as Box<dyn Collector>;
    vec![
        f("cpu", &[Entitlement::None], &[Module::Cpu]),
        f(
            "ioreport",
            &[Entitlement::IoReport],
            &[Module::Cpu, Module::Gpu, Module::Power],
        ),
        f("smc", &[Entitlement::SmcUserClient], &[Module::Sensors]),
        f("hid", &[Entitlement::HidSensors], &[Module::Sensors]),
        f(
            "net_per_process",
            &[Entitlement::NetworkStatistics],
            &[Module::Network],
        ),
        f(
            "gpu_per_process",
            &[Entitlement::IoRegistryGpuClients],
            &[Module::Gpu],
        ),
        f(
            "both",
            &[Entitlement::None, Entitlement::IoReport],
            &[Module::Power],
        ),
    ]
}

const APPSTORE: &[Entitlement] = &[Entitlement::None];

#[test]
fn appstore_set_drops_private_api_collectors() {
    let mut filtered = filter_by_entitlements(fakes(), APPSTORE);
    let ids: Vec<_> = filtered.iter().map(|c| c.id().as_str()).collect();
    assert_eq!(
        ids,
        [
            "cpu",
            "ioreport",
            "smc",
            "hid",
            "net_per_process",
            "gpu_per_process",
            "both"
        ]
    );

    let mut missing = Vec::new();
    for c in &mut filtered {
        match c.probe() {
            Probe::Unsupported {
                reason: UnsupportedReason::MissingEntitlement,
            } => {
                missing.push(c.id().as_str());
                // Denied stand-ins keep their modules so capabilities can say which
                // modules are unavailable in this edition, and never sample.
                assert!(!c.modules().is_empty());
                let tick = Tick {
                    n: 0,
                    wall_ms: 0,
                    continuous_ns: 0,
                    interval_ms: 1_000,
                };
                assert!(c.sample(&tick, &mut SampleBuf::new()).is_err());
            }
            Probe::Supported(_) => assert_eq!(c.id().as_str(), "cpu"),
            other => panic!("unexpected probe {other:?}"),
        }
    }
    assert_eq!(
        missing,
        [
            "ioreport",
            "smc",
            "hid",
            "net_per_process",
            "gpu_per_process",
            "both"
        ]
    );
}

#[test]
fn full_set_keeps_everything() {
    let all = &[
        Entitlement::None,
        Entitlement::IoReport,
        Entitlement::SmcUserClient,
        Entitlement::HidSensors,
        Entitlement::NetworkStatistics,
        Entitlement::IoRegistryGpuClients,
    ];
    for mut c in filter_by_entitlements(fakes(), all) {
        assert!(matches!(c.probe(), Probe::Supported(_)), "{}", c.id());
    }
}

#[test]
fn allowed_set_matches_the_build() {
    if cfg!(feature = "appstore") {
        assert_eq!(allowed_entitlements(), APPSTORE);
    } else {
        assert!(allowed_entitlements().contains(&Entitlement::IoReport));
        assert!(allowed_entitlements().contains(&Entitlement::SmcUserClient));
    }
}

#[test]
fn platform_collectors_need_only_public_apis_or_declare() {
    // Every registered collector declares at least one entitlement, and the ones the
    // appstore set drops are exactly those declaring something besides `None`.
    let collectors = platform_collectors(std::sync::Arc::new(kelvo_collect::calib::NoScaleStore));
    for c in &collectors {
        assert!(
            !c.required_entitlements().is_empty(),
            "{} declares nothing",
            c.id()
        );
    }
    let needs_private: Vec<_> = collectors
        .iter()
        .filter(|c| {
            c.required_entitlements()
                .iter()
                .any(|e| *e != Entitlement::None)
        })
        .map(|c| c.id())
        .collect();
    let denied: Vec<_> = filter_by_entitlements(collectors, APPSTORE)
        .into_iter()
        .filter(|c| {
            c.required_entitlements()
                .iter()
                .any(|e| *e != Entitlement::None)
        })
        .map(|c| c.id())
        .collect();
    assert_eq!(needs_private, denied);
}
