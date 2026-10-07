//! IOHID temperature sensors (macmon `IOHIDSensors`, after freedomtan/sensors).
//!
//! `IOHIDEventSystemClient` and the event accessors are private IOKit SPI. The matching
//! dictionary selects Apple vendor page 0xff00, usage 5 (temperature sensor).
//!
//! Upstream created a new event-system client and copied the service list on every read.
//! Here the client and services are created once in [`HidSensors::open`] and only the
//! events are read per sample.

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFAllocatorRef, CFType, CFTypeRef, TCFType, kCFAllocatorDefault};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};

type ClientRef = CFTypeRef;
type ServiceRef = CFTypeRef;
type EventRef = CFTypeRef;

const PAGE_APPLE_VENDOR: i32 = 0xff00;
const USAGE_TEMPERATURE_SENSOR: i32 = 0x0005;
const EVENT_TYPE_TEMPERATURE: i64 = 15;
/// `IOHIDEventFieldBase(kIOHIDEventTypeTemperature)`.
const FIELD_TEMPERATURE_LEVEL: i64 = EVENT_TYPE_TEMPERATURE << 16;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDEventSystemClientCreate(allocator: CFAllocatorRef) -> ClientRef;
    fn IOHIDEventSystemClientSetMatching(client: ClientRef, matching: CFDictionaryRef) -> i32;
    fn IOHIDEventSystemClientCopyServices(client: ClientRef) -> CFArrayRef;
    fn IOHIDServiceClientCopyProperty(service: ServiceRef, key: CFStringRef) -> CFTypeRef;
    fn IOHIDServiceClientCopyEvent(service: ServiceRef, kind: i64, a: i32, b: i64) -> EventRef;
    fn IOHIDEventGetFloatValue(event: EventRef, field: i64) -> f64;
}

/// The temperature sensors present at [`HidSensors::open`] time.
pub(crate) struct HidSensors {
    _client: CFType,
    /// Owns the service references in `services`.
    _array: CFArray<CFType>,
    /// `(service, Product name)`, in the system's order.
    services: Vec<(ServiceRef, String)>,
}

// SAFETY: the client and services are only used through `&self`/`&mut self` from one
// thread at a time (the engine's sampler thread); CF retain/release is thread-safe.
unsafe impl Send for HidSensors {}

impl HidSensors {
    /// Opens the event system and lists temperature services. `None` if the SPI is
    /// missing or returns nothing.
    pub(crate) fn open() -> Option<Self> {
        let matching = CFDictionary::from_CFType_pairs(&[
            (
                CFString::new("PrimaryUsagePage"),
                CFNumber::from(PAGE_APPLE_VENDOR),
            ),
            (
                CFString::new("PrimaryUsage"),
                CFNumber::from(USAGE_TEMPERATURE_SENSOR),
            ),
        ]);
        // SAFETY: creates a client we own (create rule) or returns null.
        let client = unsafe { IOHIDEventSystemClientCreate(kCFAllocatorDefault) };
        if client.is_null() {
            return None;
        }
        // SAFETY: non-null, create rule.
        let client = unsafe { CFType::wrap_under_create_rule(client) };
        // SAFETY: `client` is live; `matching` is a valid dictionary the call copies.
        unsafe {
            IOHIDEventSystemClientSetMatching(
                client.as_CFTypeRef(),
                matching.as_concrete_TypeRef(),
            );
        }
        // SAFETY: `client` is live; the result is null or an array we own (copy rule).
        let array = unsafe { IOHIDEventSystemClientCopyServices(client.as_CFTypeRef()) };
        if array.is_null() {
            return None;
        }
        // SAFETY: non-null, copy rule.
        let array: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(array) };
        let product = CFString::new("Product");
        let mut services = Vec::new();
        for service in array.iter() {
            let sc = service.as_CFTypeRef();
            crate::calls::count(crate::calls::Api::Hid);
            // SAFETY: `sc` is a live service client owned by `array`; the property is
            // null or an object we own (copy rule).
            let name = unsafe { IOHIDServiceClientCopyProperty(sc, product.as_concrete_TypeRef()) };
            if name.is_null() {
                continue;
            }
            // SAFETY: non-null, copy rule.
            let name = unsafe { CFType::wrap_under_create_rule(name) };
            let Some(name) = name.downcast::<CFString>() else {
                continue;
            };
            services.push((sc, name.to_string()));
        }
        Some(Self {
            _client: client,
            _array: array,
            services,
        })
    }

    /// Sensor names in system order. Names repeat on some chips (an M3 Max lists
    /// "PMU tdie1" twice, once per PMU).
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.services.iter().map(|(_, n)| n.as_str())
    }

    /// Reads every sensor, in the order of [`HidSensors::names`]. `None` for a sensor
    /// that returned no event this time.
    pub(crate) fn read(&self, out: &mut Vec<Option<f32>>) {
        out.clear();
        for (sc, _) in &self.services {
            crate::calls::count(crate::calls::Api::Hid);
            // SAFETY: `sc` is kept alive by `_array`; the event is null or owned by us.
            let event = unsafe { IOHIDServiceClientCopyEvent(*sc, EVENT_TYPE_TEMPERATURE, 0, 0) };
            if event.is_null() {
                out.push(None);
                continue;
            }
            // SAFETY: non-null, copy rule; released when `event` drops.
            let event = unsafe { CFType::wrap_under_create_rule(event) };
            // SAFETY: `event` is a live temperature event.
            let v =
                unsafe { IOHIDEventGetFloatValue(event.as_CFTypeRef(), FIELD_TEMPERATURE_LEVEL) };
            out.push(Some(v as f32));
        }
    }
}
