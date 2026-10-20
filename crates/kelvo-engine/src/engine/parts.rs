//! The platform pieces an engine runs on: collectors, timer, power signals, hints.

use std::sync::Arc;

use kelvo_collect::Collector;

use crate::hints::DeviceHints;
use crate::power::PowerSignals;
use crate::ticker::Ticker;

/// Everything platform-specific the engine runs on.
pub struct EngineParts {
    pub collectors: Vec<Box<dyn Collector>>,
    pub ticker: Box<dyn Ticker>,
    pub power: Box<dyn PowerSignals>,
    pub hints: Option<Box<dyn DeviceHints>>,
}

impl EngineParts {
    /// This platform's collectors (filtered by the build's entitlements), timer, power
    /// signals and device hints. Learned CPU power scales persist through `scales`
    /// (D-065).
    pub fn platform(scales: Arc<dyn kelvo_collect::calib::ScaleStore>) -> Self {
        Self::platform_with(scales, kelvo_collect::ContainerNames::default())
    }

    /// [`EngineParts::platform`] with the container names a refresher fills: the Linux
    /// agent's Docker task (D-116). Without one, containers show short ids.
    pub fn platform_with(
        scales: Arc<dyn kelvo_collect::calib::ScaleStore>,
        names: kelvo_collect::ContainerNames,
    ) -> Self {
        let collectors = kelvo_collect::filter_by_entitlements(
            kelvo_collect::platform_collectors_with(scales, names),
            kelvo_collect::allowed_entitlements(),
        );
        #[cfg(target_os = "macos")]
        {
            Self {
                collectors,
                ticker: Box::new(crate::macos::GcdTicker::new()),
                power: Box::new(crate::macos::MacPowerSignals::new()),
                hints: Some(Box::new(crate::macos::IoKitDeviceHints::new())),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self {
                collectors,
                ticker: Box::new(crate::ticker::ThreadTicker::new()),
                power: Box::new(crate::power::NoPowerSignals),
                hints: None,
            }
        }
    }
}
