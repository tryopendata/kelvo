//! Saved presentation for display cards, independent of module enablement.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CardDensity {
    #[default]
    Expanded,
    Compact,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct PopoverSettings {
    pub cpu: CardDensity,
    pub memory: CardDensity,
    pub gpu: CardDensity,
    pub power: CardDensity,
    pub network: CardDensity,
    pub battery: CardDensity,
    pub containers: CardDensity,
}
