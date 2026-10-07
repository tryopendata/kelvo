//! Small safe wrappers over `sysctlbyname` and the Mach timebase.

use std::ffi::{CStr, c_int};
use std::mem::MaybeUninit;

/// Reads a fixed-size sysctl value. Returns `None` if the name is unknown or the kernel
/// returned a different size.
fn read_pod<T: Copy>(name: &CStr) -> Option<T> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut len = size_of::<T>();
    crate::calls::count(crate::calls::Api::Kernel);
    // SAFETY: `name` is NUL-terminated; `value` points at `len` writable bytes, and the
    // kernel writes at most `len` bytes. We only read `value` when the call succeeded and
    // filled exactly `size_of::<T>()` bytes, and every `T` used here is plain old data.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc == 0 && len == size_of::<T>() {
        // SAFETY: fully initialized by the successful call above (checked length).
        Some(unsafe { value.assume_init() })
    } else {
        None
    }
}

/// An integer sysctl. Accepts 4- and 8-byte values (the kernel is not consistent).
/// Public, like [`string`], for the app shell's host facts.
pub fn int(name: &CStr) -> Option<i64> {
    let mut buf = [0u8; 8];
    let mut len = buf.len();
    crate::calls::count(crate::calls::Api::Kernel);
    // SAFETY: `buf` is 8 writable bytes and `len` says so; the kernel writes at most that.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    match len {
        4 => {
            let b: [u8; 4] = buf.get(..4)?.try_into().ok()?;
            Some(i64::from(i32::from_ne_bytes(b)))
        }
        8 => Some(i64::from_ne_bytes(buf)),
        _ => None,
    }
}

/// A string sysctl, without the trailing NUL. Public so the engine can re-export it for
/// the app shell's host facts.
pub fn string(name: &CStr) -> Option<String> {
    crate::calls::count(crate::calls::Api::Kernel);
    string_in::<256>(name)
}

/// [`string`] read into an `N`-byte buffer and not counted; `None` when the value does
/// not fit.
pub(crate) fn string_in<const N: usize>(name: &CStr) -> Option<String> {
    let mut buf = [0u8; N];
    let mut len = buf.len();
    // SAFETY: `name` is NUL-terminated; `buf` is `N` writable bytes and `len` says so;
    // the kernel writes at most that many.
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return None;
    }
    let bytes = buf.get(..len)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8(bytes.get(..end)?.to_vec()).ok()
}

/// The bytes `sysctl(mib)` would return: the size query of a size-then-fill read.
/// `None` when the call fails; errno is left for the caller.
pub(crate) fn mib_len(mib: &mut [c_int]) -> Option<usize> {
    let mut len = 0usize;
    crate::calls::count(crate::calls::Api::Kernel);
    // SAFETY: a size query: null output buffer, `len` receives the needed size; `mib`
    // has `mib.len()` valid entries.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0).then_some(len)
}

/// Reads `sysctl(mib)` into `buf`, returning how many bytes the kernel wrote: the fill
/// of a size-then-fill read after [`mib_len`]. `None` when the call fails; errno is left
/// for the caller (`ENOMEM` when `buf` is too small).
pub(crate) fn mib_into(mib: &mut [c_int], buf: &mut [u8]) -> Option<usize> {
    let mut len = buf.len();
    crate::calls::count(crate::calls::Api::Kernel);
    // SAFETY: `buf` has `len` writable bytes; the kernel writes at most `len`. `mib` has
    // `mib.len()` valid entries.
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (rc == 0).then_some(len)
}

/// `vm.swapusage`.
pub(crate) fn swap_usage() -> Option<libc::xsw_usage> {
    read_pod::<libc::xsw_usage>(c"vm.swapusage")
}

/// `kern.boottime`, for the app shell's host facts.
pub fn boot_time() -> Option<libc::timeval> {
    read_pod::<libc::timeval>(c"kern.boottime")
}

/// Multiplier from Mach absolute time units to nanoseconds (1.0 on Intel, 125/3 on
/// Apple Silicon). `rusage_info` CPU times are in these units, not nanoseconds.
pub(crate) fn mach_ticks_to_ns() -> f64 {
    let mut info = mach2::mach_time::mach_timebase_info { numer: 0, denom: 0 };
    // SAFETY: `info` is a valid out-pointer for the duration of the call.
    let rc = unsafe { mach2::mach_time::mach_timebase_info(&mut info) };
    if rc == 0 && info.denom != 0 {
        f64::from(info.numer) / f64::from(info.denom)
    } else {
        1.0
    }
}

/// `mach_continuous_time` in nanoseconds; advances during sleep. The engine owns the
/// clock; collectors only use this in live tests.
#[cfg(test)]
pub(crate) fn continuous_ns() -> u64 {
    // SAFETY: no arguments, no preconditions.
    let t = unsafe { mach2::mach_time::mach_continuous_time() };
    (t as f64 * mach_ticks_to_ns()) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_known_sysctls() {
        assert!(int(c"hw.ncpu").is_some_and(|n| n > 0));
        assert!(int(c"hw.memsize").is_some_and(|n| n > 0));
        assert!(string(c"kern.ostype").is_some_and(|s| s == "Darwin"));
        assert!(int(c"no.such.sysctl").is_none());
        assert!(mach_ticks_to_ns() > 0.0);
    }
}
