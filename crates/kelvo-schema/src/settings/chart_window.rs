use serde::{Deserialize, Serialize};

/// The dashboard chart window. Longer and fixed ranges read retained history.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
pub enum ChartWindow {
    #[serde(rename = "5m")]
    M5,
    #[default]
    #[serde(rename = "15m")]
    M15,
    #[serde(rename = "30m")]
    M30,
    #[serde(rename = "1h")]
    H1,
    /// A rolling duration in milliseconds.
    #[serde(rename = "relative")]
    Relative(#[specta(type = crate::JsSafeInt)] i64),
    /// A fixed interval in Unix milliseconds.
    #[serde(rename = "absolute")]
    Absolute {
        #[specta(type = crate::JsSafeInt)]
        from_ms: i64,
        #[specta(type = crate::JsSafeInt)]
        to_ms: i64,
    },
}

impl ChartWindow {
    pub fn valid(self) -> bool {
        let duration = match self {
            Self::Relative(ms) => ms,
            Self::Absolute { from_ms, to_ms } => {
                if from_ms < 0 || to_ms > 9_007_199_254_740_991 {
                    return false;
                }
                to_ms.saturating_sub(from_ms)
            }
            _ => return true,
        };
        (60_000..=90 * 86_400_000).contains(&duration)
    }
}
