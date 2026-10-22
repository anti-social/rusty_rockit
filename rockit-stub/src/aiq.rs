use std::ptr;

use rockit_bindings::aiq as ffi;

use crate::state::{enter, lock};

const NO_ERROR: ffi::XCamReturn = ffi::XCamReturn_XCAM_RETURN_NO_ERROR;
const ERROR_PARAM: ffi::XCamReturn = ffi::XCamReturn_XCAM_RETURN_ERROR_PARAM;
const ERROR_FAILED: ffi::XCamReturn = ffi::XCamReturn_XCAM_RETURN_ERROR_FAILED;

/// Number of fake sensors reported by `enumStaticMetas`.
const SENSOR_COUNT: i32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiqState {
    Initialized,
    Prepared,
    Started,
}

pub(crate) struct AiqCtx {
    state: AiqState,
}

fn sensor_name(index: i32) -> String {
    format!("m00_b_fake-sensor {index}-0036")
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rk_aiq_uapi2_sysctl_enumStaticMetas(
    index: ::std::os::raw::c_int, static_info: *mut ffi::rk_aiq_static_info_t,
) -> ffi::XCamReturn {
    if let Some(code) = enter("rk_aiq_uapi2_sysctl_enumStaticMetas", format_args!("{index}")) {
        return code;
    }
    let Some(info) = (unsafe { static_info.as_mut() }) else {
        return ERROR_PARAM;
    };
    if !(0..SENSOR_COUNT).contains(&index) {
        return ERROR_PARAM;
    }
    *info = unsafe { std::mem::zeroed() };
    let name = sensor_name(index);
    let dst = &mut info.sensor_info.sensor_name;
    let max_len = dst.len() - 1;
    for (d, s) in dst.iter_mut().zip(name.bytes().take(max_len)) {
        *d = s as _;
    }
    info.sensor_info.phyId = index;
    NO_ERROR
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rk_aiq_uapi2_sysctl_init(
    sns_ent_name: *const ::std::os::raw::c_char,
    _iq_file_dir: *const ::std::os::raw::c_char,
    _err_cb: ffi::rk_aiq_error_cb,
    _metas_cb: ffi::rk_aiq_metas_cb,
) -> *mut ffi::rk_aiq_sys_ctx_t {
    if sns_ent_name.is_null() {
        return ptr::null_mut();
    }
    let name = unsafe { std::ffi::CStr::from_ptr(sns_ent_name) }.to_string_lossy();
    if enter("rk_aiq_uapi2_sysctl_init", format_args!("{name}")).is_some() {
        return ptr::null_mut();
    }
    if !(0..SENSOR_COUNT).any(|i| sensor_name(i) == name) {
        return ptr::null_mut();
    }
    // The context is opaque to the caller: hand out a unique heap address as a handle.
    let handle = Box::into_raw(Box::new(0u8)) as usize;
    lock().aiq_ctxs.insert(handle, AiqCtx { state: AiqState::Initialized });
    handle as *mut ffi::rk_aiq_sys_ctx_t
}

fn transition(
    name: &str, ctx: *const ffi::rk_aiq_sys_ctx_t, from: &[AiqState], to: AiqState,
) -> ffi::XCamReturn {
    if let Some(code) = enter(name, format_args!("{ctx:p}")) {
        return code;
    }
    let mut state = lock();
    let Some(aiq) = state.aiq_ctxs.get_mut(&(ctx as usize)) else {
        return ERROR_PARAM;
    };
    if !from.contains(&aiq.state) {
        return ERROR_FAILED;
    }
    aiq.state = to;
    NO_ERROR
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rk_aiq_uapi2_sysctl_prepare(
    ctx: *const ffi::rk_aiq_sys_ctx_t,
    _width: u32,
    _height: u32,
    _mode: ffi::rk_aiq_working_mode_t,
) -> ffi::XCamReturn {
    transition(
        "rk_aiq_uapi2_sysctl_prepare",
        ctx,
        &[AiqState::Initialized, AiqState::Prepared],
        AiqState::Prepared,
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rk_aiq_uapi2_sysctl_start(
    ctx: *const ffi::rk_aiq_sys_ctx_t,
) -> ffi::XCamReturn {
    transition("rk_aiq_uapi2_sysctl_start", ctx, &[AiqState::Prepared], AiqState::Started)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rk_aiq_uapi2_sysctl_stop(
    ctx: *const ffi::rk_aiq_sys_ctx_t, _keep_ext_hw_st: bool,
) -> ffi::XCamReturn {
    transition("rk_aiq_uapi2_sysctl_stop", ctx, &[AiqState::Started], AiqState::Prepared)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rk_aiq_uapi2_sysctl_deinit(ctx: *mut ffi::rk_aiq_sys_ctx_t) {
    if enter("rk_aiq_uapi2_sysctl_deinit", format_args!("{ctx:p}")).is_some() {
        return;
    }
    if lock().aiq_ctxs.remove(&(ctx as usize)).is_some() {
        drop(unsafe { Box::from_raw(ctx as *mut u8) });
    }
}
