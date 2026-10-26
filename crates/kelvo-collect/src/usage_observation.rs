//! Source evidence for durable attribution, independent from emitted process rows.
use kelvo_schema::durable_usage::UsageIncompleteCause;

/// Resource order: CPU, GPU, RAM, CPU energy, network. `None` means not sampled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageObservation {
    pub span_ms: u64,
    pub causes: [Option<UsageIncompleteCause>; 4],
}

impl crate::SampleBuf {
    pub fn set_usage_observation(&mut self, resource: usize, observation: UsageObservation) {
        if let Some(slot) = self.usage.get_mut(resource) {
            *slot = Some(observation);
        }
    }

    pub fn usage_observations(&self) -> &[Option<UsageObservation>; 5] {
        &self.usage
    }

    pub fn set_net_session_from_ns(&mut self, from_ns: u64) {
        self.net_session_from_ns = Some(from_ns);
    }
    pub fn net_session_from_ns(&self) -> Option<u64> {
        self.net_session_from_ns
    }

    /// Process source visibility includes rows skipped for missing rate baselines.
    pub fn observe_processes(&mut self, span_ms: u64, listed: u32, readable: u32, energy: bool) {
        use kelvo_schema::durable_usage::UsageIncompleteCause;
        let causes = [
            (listed > readable).then_some(UsageIncompleteCause::UnreadableProcess),
            (span_ms == 0 || self.processes.len() < readable as usize)
                .then_some(UsageIncompleteCause::NoBaseline),
            self.processes
                .iter()
                .any(|p| p.app.is_none())
                .then_some(UsageIncompleteCause::UnresolvedIdentity),
            None,
        ];
        let observation = UsageObservation { span_ms, causes };
        for resource in [0, 2] {
            self.set_usage_observation(resource, observation.clone());
        }
        if energy {
            self.set_usage_observation(3, observation);
        }
    }
}
