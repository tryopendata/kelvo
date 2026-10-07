//! Text the menu bar shows and VoiceOver reads: percentages, temperatures, watts and
//! rates, in the user's units.

use kelvo_schema::Module;
use kelvo_schema::settings::{NetworkUnit, TemperatureUnit};

pub(super) fn pct_text(v: f32) -> String {
    format!("{}%", v.clamp(0.0, 999.0).round() as i32)
}

pub(super) fn temp_text(c: f32, unit: TemperatureUnit) -> String {
    let t = match unit {
        TemperatureUnit::Celsius => c,
        TemperatureUnit::Fahrenheit => c * 9.0 / 5.0 + 32.0,
    };
    format!("{}°", t.round() as i32)
}

pub(super) fn watts_text(w: f32) -> String {
    if w < 100.0 {
        format!("{w:.1}W")
    } else {
        format!("{w:.0}W")
    }
}

/// Compact rate for the menu bar: "38.4MB", "512KB", "0KB"; bits use a lowercase "b".
pub(super) fn rate_text(bytes_per_sec: f32, unit: NetworkUnit) -> String {
    let (v, suffix) = match unit {
        NetworkUnit::BytesPerSec => (bytes_per_sec, "B"),
        NetworkUnit::BitsPerSec => (bytes_per_sec * 8.0, "b"),
    };
    let v = v.max(0.0);
    if v >= 1e9 {
        format!("{:.1}G{suffix}", v / 1e9)
    } else if v >= 1e6 {
        format!("{:.1}M{suffix}", v / 1e6)
    } else {
        format!("{:.0}K{suffix}", (v / 1e3).floor())
    }
}

/// A rate in the stacked graph form: "38.4 MB/s", "512 KB/s", "9.6 Mb/s".
pub(super) fn rate_line(bytes_per_sec: f32, unit: NetworkUnit) -> String {
    let (v, suffix) = match unit {
        NetworkUnit::BytesPerSec => (bytes_per_sec, "B"),
        NetworkUnit::BitsPerSec => (bytes_per_sec * 8.0, "b"),
    };
    let v = v.max(0.0);
    let scaled = |v: f32, prefix: &str| {
        // Decided after rounding: 99.96 would print "100.0" with one decimal.
        if (v * 10.0).round() >= 1000.0 {
            format!("{v:.0} {prefix}{suffix}/s")
        } else {
            format!("{v:.1} {prefix}{suffix}/s")
        }
    };
    if v >= 1e9 {
        scaled(v / 1e9, "G")
    } else if v >= 1e6 {
        scaled(v / 1e6, "M")
    } else {
        format!("{:.0} K{suffix}/s", (v / 1e3).floor())
    }
}

pub(super) fn rate_words(bytes_per_sec: f32, unit: NetworkUnit) -> String {
    let (v, word) = match unit {
        NetworkUnit::BytesPerSec => (bytes_per_sec, "bytes"),
        NetworkUnit::BitsPerSec => (bytes_per_sec * 8.0, "bits"),
    };
    if v >= 1e6 {
        format!("{:.1} mega{word} per second", v / 1e6)
    } else {
        format!("{:.0} kilo{word} per second", (v / 1e3).floor())
    }
}

pub(super) fn words_name(module: Module) -> &'static str {
    match module {
        Module::Cpu => "CPU",
        Module::Gpu => "GPU",
        Module::Memory => "memory",
        Module::Power | Module::Sensors => "power",
        Module::Network => "network",
        Module::Disk => "disk",
        Module::Battery => "battery",
        Module::Unknown => "unknown",
    }
}
