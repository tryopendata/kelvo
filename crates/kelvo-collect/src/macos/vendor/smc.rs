//! AppleSMC user client (macmon `SMC`, `KeyData`, `smc_numeric_value`).
//!
//! The SMC is reached through the `AppleSMCKeysEndpoint` IOService and
//! `IOConnectCallStructMethod` selector 2 with the `KeyData` struct below. Neither the
//! struct layout nor the command codes are documented by Apple; they are the ones every
//! open-source SMC reader uses (smcFanControl, Stats, macmon).

use std::collections::HashMap;
use std::ffi::c_void;

use super::iokit::{self, IOServiceClose};

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceOpen(service: u32, owning_task: u32, kind: u32, connect: *mut u32) -> i32;
    fn IOConnectCallStructMethod(
        connect: u32,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> i32;
}

/// `IOConnectCallStructMethod` selector for SMC key commands.
const SELECTOR_HANDLE_YPC_EVENT: u32 = 2;
const CMD_READ_BYTES: u8 = 5;
const CMD_READ_KEYINFO: u8 = 9;
/// `result` byte for an unknown key.
const RESULT_KEY_NOT_FOUND: u8 = 132;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct KeyDataVer {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct PLimitData {
    version: u16,
    length: u16,
    cpu_p_limit: u32,
    gpu_p_limit: u32,
    mem_p_limit: u32,
}

/// Size and FourCC type of a key's value.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeyInfo {
    pub data_size: u32,
    pub data_type: u32,
    pub data_attributes: u8,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct KeyData {
    key: u32,
    vers: KeyDataVer,
    p_limit_data: PLimitData,
    key_info: KeyInfo,
    result: u8,
    status: u8,
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

/// The kernel checks the struct size; 80 bytes is what AppleSMC expects on arm64.
const _: () = assert!(size_of::<KeyData>() == 80);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum SmcError {
    #[error("AppleSMCKeysEndpoint not found")]
    NoService,
    #[error("IOServiceOpen failed with {0}")]
    Open(i32),
    #[error("IOConnectCallStructMethod failed with {0}")]
    Call(i32),
    #[error("SMC key not found")]
    KeyNotFound,
    #[error("SMC returned result {0}")]
    Result(u8),
}

/// An open SMC connection. Key infos are cached, so a steady-state read is one call.
pub(crate) struct Smc {
    conn: u32,
    infos: HashMap<u32, KeyInfo>,
}

impl Smc {
    pub(crate) fn open() -> Result<Self, SmcError> {
        let service = iokit::matching_services(c"AppleSMC")
            .into_iter()
            .find(|s| s.name().as_deref() == Some("AppleSMCKeysEndpoint"))
            .ok_or(SmcError::NoService)?;
        let mut conn = 0;
        // SAFETY: `service` is a live registry entry; mach_task_self() is our task port;
        // `conn` receives the connection on success.
        let rc =
            unsafe { IOServiceOpen(service.raw(), mach2::traps::mach_task_self(), 0, &mut conn) };
        if rc != 0 || conn == 0 {
            return Err(SmcError::Open(rc));
        }
        Ok(Self {
            conn,
            infos: HashMap::new(),
        })
    }

    fn call(&self, input: &KeyData) -> Result<KeyData, SmcError> {
        let mut output = KeyData::default();
        let mut out_size = size_of::<KeyData>();
        crate::calls::count(crate::calls::Api::Smc);
        // SAFETY: both pointers reference live `KeyData` values of the size passed; the
        // kernel writes at most `out_size` bytes into `output`.
        let rc = unsafe {
            IOConnectCallStructMethod(
                self.conn,
                SELECTOR_HANDLE_YPC_EVENT,
                (input as *const KeyData).cast(),
                size_of::<KeyData>(),
                (&mut output as *mut KeyData).cast(),
                &mut out_size,
            )
        };
        if rc != 0 {
            return Err(SmcError::Call(rc));
        }
        match output.result {
            0 => Ok(output),
            RESULT_KEY_NOT_FOUND => Err(SmcError::KeyNotFound),
            r => Err(SmcError::Result(r)),
        }
    }

    pub(crate) fn key_info(&mut self, key: u32) -> Result<KeyInfo, SmcError> {
        if let Some(info) = self.infos.get(&key) {
            return Ok(*info);
        }
        let out = self.call(&KeyData {
            key,
            data8: CMD_READ_KEYINFO,
            ..KeyData::default()
        })?;
        self.infos.insert(key, out.key_info);
        Ok(out.key_info)
    }

    /// The raw value of `key` and its type. The slice is `data_size` bytes long.
    pub(crate) fn read(&mut self, key: u32) -> Result<([u8; 32], KeyInfo), SmcError> {
        let info = self.key_info(key)?;
        let out = self.call(&KeyData {
            key,
            key_info: info,
            data8: CMD_READ_BYTES,
            ..KeyData::default()
        })?;
        Ok((out.bytes, info))
    }

    /// The value of `key` decoded as a number, if its type is one [`decode`] knows.
    pub(crate) fn read_f32(&mut self, key: u32) -> Option<f32> {
        let (bytes, info) = self.read(key).ok()?;
        let len = usize::try_from(info.data_size).ok()?;
        decode(bytes.get(..len)?, info.data_type)
    }
}

impl Drop for Smc {
    fn drop(&mut self) {
        // SAFETY: `conn` is the connection we opened and have not closed.
        unsafe {
            IOServiceClose(self.conn);
        }
    }
}

/// A four-character SMC key as the big-endian `u32` the SMC expects.
pub(crate) const fn four_cc_bytes(key: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*key)
}

/// [`four_cc_bytes`] for a runtime string; `None` unless it is exactly four bytes.
pub(crate) fn four_cc(key: &str) -> Option<u32> {
    let b: &[u8; 4] = key.as_bytes().try_into().ok()?;
    Some(four_cc_bytes(b))
}

const TYPE_FLT: u32 = four_cc_bytes(b"flt ");
const TYPE_FPE2: u32 = four_cc_bytes(b"fpe2");
const TYPE_UI8: u32 = four_cc_bytes(b"ui8 ");
const TYPE_UI16: u32 = four_cc_bytes(b"ui16");
const TYPE_UI32: u32 = four_cc_bytes(b"ui32");

/// Decodes an SMC value (macmon `smc_numeric_value`). Apple Silicon reports
/// temperatures, power and fan speeds as little-endian `flt `; the integer types are
/// big-endian.
pub(crate) fn decode(data: &[u8], data_type: u32) -> Option<f32> {
    match data_type {
        TYPE_FLT => Some(f32::from_le_bytes(data.get(..4)?.try_into().ok()?)),
        TYPE_FPE2 => {
            let (hi, lo) = (*data.first()?, *data.get(1)?);
            Some(f32::from((u16::from(hi) << 6) | (u16::from(lo) >> 2)))
        }
        TYPE_UI8 => Some(f32::from(*data.first()?)),
        TYPE_UI16 => Some(f32::from(u16::from_be_bytes(
            data.get(..2)?.try_into().ok()?,
        ))),
        TYPE_UI32 => Some(u32::from_be_bytes(data.get(..4)?.try_into().ok()?) as f32),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_values() {
        assert_eq!(decode(&42.5f32.to_le_bytes(), TYPE_FLT), Some(42.5));
        assert_eq!(decode(&[0x13, 0x88], TYPE_FPE2), Some(1250.0));
        assert_eq!(decode(&[7], TYPE_UI8), Some(7.0));
        assert_eq!(decode(&[0x04, 0xd2], TYPE_UI16), Some(1234.0));
        assert_eq!(decode(&[0, 0, 0x04, 0xd2], TYPE_UI32), Some(1234.0));
        assert_eq!(decode(&[1, 2], TYPE_FLT), None, "short flt");
        assert_eq!(
            decode(&[0; 8], four_cc_bytes(b"ioft")),
            None,
            "unknown type"
        );
    }

    #[test]
    fn keys_are_big_endian_four_cc() {
        assert_eq!(four_cc("PSTR"), Some(0x5053_5452));
        assert_eq!(four_cc("TOOLONG"), None);
    }
}
