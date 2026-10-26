//! Partial presentation settings, independent of collector configuration.
use super::set;
use kelvo_schema::settings::PopoverSettings;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, specta::Type)]
#[serde(default)]
pub struct PopoverPatch {
    #[specta(optional)]
    pub cpu: Option<kelvo_schema::settings::CardDensity>,
    #[specta(optional)]
    pub memory: Option<kelvo_schema::settings::CardDensity>,
    #[specta(optional)]
    pub gpu: Option<kelvo_schema::settings::CardDensity>,
    #[specta(optional)]
    pub power: Option<kelvo_schema::settings::CardDensity>,
    #[specta(optional)]
    pub network: Option<kelvo_schema::settings::CardDensity>,
    #[specta(optional)]
    pub battery: Option<kelvo_schema::settings::CardDensity>,
    #[specta(optional)]
    pub containers: Option<kelvo_schema::settings::CardDensity>,
}

impl PopoverPatch {
    pub fn apply(self, target: &mut PopoverSettings) {
        set(&mut target.cpu, self.cpu);
        set(&mut target.memory, self.memory);
        set(&mut target.gpu, self.gpu);
        set(&mut target.power, self.power);
        set(&mut target.network, self.network);
        set(&mut target.battery, self.battery);
        set(&mut target.containers, self.containers);
    }
}
