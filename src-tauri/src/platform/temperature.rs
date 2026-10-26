//! Resolve the temperature override using public Foundation formatting, not the region.
use kelvo_schema::settings::TemperatureUnit;
use objc2::{AllocAnyThread, rc::Retained};
use objc2_foundation::{
    NSLocale, NSMeasurement, NSMeasurementFormatter, NSMeasurementFormatterUnitOptions,
    NSUnitTemperature,
};

fn resolve(locale: &NSLocale) -> Option<TemperatureUnit> {
    let formatter = NSMeasurementFormatter::new();
    formatter.setLocale(Some(locale));
    let celsius = NSMeasurement::<NSUnitTemperature>::initWithDoubleValue_unit(
        NSMeasurement::<NSUnitTemperature>::alloc(),
        0.0,
        &NSUnitTemperature::celsius(),
    );
    let fahrenheit = NSMeasurement::<NSUnitTemperature>::initWithDoubleValue_unit(
        NSMeasurement::<NSUnitTemperature>::alloc(),
        32.0,
        &NSUnitTemperature::fahrenheit(),
    );
    // SAFETY: Erasing the lightweight generic retains the NSUnitTemperature objects.
    let celsius: Retained<NSMeasurement> = unsafe { Retained::cast_unchecked(celsius) };
    // SAFETY: Erasing the same lightweight generic for Fahrenheit is valid too.
    let fahrenheit: Retained<NSMeasurement> = unsafe { Retained::cast_unchecked(fahrenheit) };
    // SAFETY: Both measurements use NSUnitTemperature, a subclass of NSUnit.
    let automatic = unsafe { formatter.stringFromMeasurement(&celsius) };
    formatter.setUnitOptions(NSMeasurementFormatterUnitOptions::ProvidedUnit);
    // SAFETY: These measurements have the same valid unit class as above.
    let c = unsafe { formatter.stringFromMeasurement(&celsius) };
    // SAFETY: Fahrenheit is also a valid NSUnit subclass.
    let f = unsafe { formatter.stringFromMeasurement(&fahrenheit) };
    if automatic == c {
        Some(TemperatureUnit::Celsius)
    } else if automatic == f {
        Some(TemperatureUnit::Fahrenheit)
    } else {
        None
    }
}

pub fn default_unit() -> Option<TemperatureUnit> {
    resolve(&NSLocale::currentLocale())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_foundation::NSString;
    #[test]
    fn region_and_temperature_override_are_independent() {
        for (id, unit) in [
            ("en_US", TemperatureUnit::Fahrenheit),
            ("en_GB", TemperatureUnit::Celsius),
            ("en_US@mu=celsius", TemperatureUnit::Celsius),
            ("en_GB@mu=fahrenhe", TemperatureUnit::Fahrenheit),
        ] {
            let locale =
                NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &NSString::from_str(id));
            assert_eq!(resolve(&locale), Some(unit), "{id}");
        }
    }
}
