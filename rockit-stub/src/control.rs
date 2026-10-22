//! Test-control API of the fake library.
//!
//! These symbols don't exist in the real Rockit library. Tests declare them with
//! `unsafe extern "C"` to inspect and steer the fake.

use std::collections::VecDeque;
use std::ffi::{CStr, c_char, c_int};

use crate::state::{COND, DEFAULT_VI_FPS, control, lock};

pub const ROCKIT_STUB_LIVE_MB: c_int = 0;
pub const ROCKIT_STUB_LIVE_POOLS: c_int = 1;
pub const ROCKIT_STUB_LIVE_VENC_CHNS: c_int = 2;
pub const ROCKIT_STUB_LIVE_VPSS_GRPS: c_int = 3;
pub const ROCKIT_STUB_LIVE_BINDS: c_int = 4;
pub const ROCKIT_STUB_LIVE_VI_CHNS: c_int = 5;
pub const ROCKIT_STUB_LIVE_VI_DEVS: c_int = 6;
pub const ROCKIT_STUB_LIVE_AIQ_CTXS: c_int = 7;

/// Restores the initial state: drops every object, clears the call log and pending failures.
#[unsafe(no_mangle)]
pub extern "C" fn rockit_stub_reset() {
    let aiq_ctxs = std::mem::take(&mut lock().aiq_ctxs);
    for handle in aiq_ctxs.into_keys() {
        drop(unsafe { Box::from_raw(handle as *mut u8) });
    }
    lock().reset();
    let mut control = control();
    control.log.clear();
    control.fail.clear();
    COND.notify_all();
}

/// Makes the next call of the API function `name` fail with `code`.
/// Calls queue up: several calls for the same name fail consecutive invocations.
/// Functions that don't return an error code (e.g. `RK_MPI_MB_GetMB`) return their
/// failure value (null pointer, invalid pool) instead.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rockit_stub_fail_next(name: *const c_char, code: i32) {
    if name.is_null() {
        return;
    }
    let name = unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned();
    control().fail.entry(name).or_insert_with(VecDeque::new).push_back(code);
}

/// Copies the newline separated call log into `buf` (not NUL terminated) and returns
/// its full length. Pass a null `buf` to query the length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rockit_stub_call_log(buf: *mut c_char, len: usize) -> usize {
    let log = control().log.join("\n");
    if !buf.is_null() {
        let n = len.min(log.len());
        unsafe { std::ptr::copy_nonoverlapping(log.as_ptr(), buf as *mut u8, n) };
    }
    log.len()
}

#[unsafe(no_mangle)]
pub extern "C" fn rockit_stub_clear_call_log() {
    control().log.clear();
}

/// Number of live objects of a `ROCKIT_STUB_LIVE_*` kind, or -1 for an unknown kind.
#[unsafe(no_mangle)]
pub extern "C" fn rockit_stub_live_count(kind: c_int) -> i32 {
    let state = lock();
    let count = match kind {
        ROCKIT_STUB_LIVE_MB => state.blocks.len(),
        ROCKIT_STUB_LIVE_POOLS => state.pools.len(),
        ROCKIT_STUB_LIVE_VENC_CHNS => state.venc_chns.len(),
        ROCKIT_STUB_LIVE_VPSS_GRPS => state.vpss_grps.len(),
        ROCKIT_STUB_LIVE_BINDS => state.binds.len(),
        ROCKIT_STUB_LIVE_VI_CHNS => state.vi_chns.values().filter(|c| c.enabled()).count(),
        ROCKIT_STUB_LIVE_VI_DEVS => state.vi_devs.values().filter(|d| d.enabled()).count(),
        ROCKIT_STUB_LIVE_AIQ_CTXS => state.aiq_ctxs.len(),
        _ => return -1,
    };
    count as i32
}

/// Sets the frame rate of fake VI channels; 0 disables pacing. Defaults to 30 or
/// to the `ROCKIT_STUB_VI_FPS` environment variable.
#[unsafe(no_mangle)]
pub extern "C" fn rockit_stub_set_vi_fps(fps: u32) {
    lock().vi_fps = fps;
    COND.notify_all();
}

pub(crate) fn default_vi_fps() -> u32 {
    std::env::var("ROCKIT_STUB_VI_FPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_VI_FPS)
}
