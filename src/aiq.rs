use std::mem::MaybeUninit;

use rockit_sys::aiq as ffi;

pub struct AiqContext {
    ctx: *mut ffi::rk_aiq_sys_ctx_s,
}

impl AiqContext {
    pub fn init(cam_id: u8) -> Self {
        let ctx = unsafe {
            let mut aiq_static_info = MaybeUninit::zeroed();
            ffi::rk_aiq_uapi2_sysctl_enumStaticMetas(
                cam_id as i32,
                aiq_static_info.as_mut_ptr(),
            );
            let aiq_static_info = aiq_static_info.assume_init();

            ffi::rk_aiq_uapi2_sysctl_init(
                &aiq_static_info.sensor_info.sensor_name as *const u8,
                c"/etc/iqfiles".as_ptr(),
                Some(isp_err_callback),
                Some(isp_sof_callback),
            )
        };

        Self { ctx }
    }

    pub fn start(&mut self) {
        unsafe {
            let ret_code = ffi::rk_aiq_uapi2_sysctl_prepare(
                self.ctx,
                0,
                0,
                ffi::rk_aiq_working_mode_t_RK_AIQ_WORKING_MODE_NORMAL,
            );
            if ret_code != 0 {
                
            }

            let ret_code = ffi::rk_aiq_uapi2_sysctl_start(self.ctx);
            if ret_code != 0 {
                
            }
        }
    }

    pub fn stop(&mut self) {
        unsafe {
            ffi::rk_aiq_uapi2_sysctl_stop(self.ctx, false);
        }
    }
}

impl Drop for AiqContext {
    fn drop(&mut self) {
        unsafe {
            ffi::rk_aiq_uapi2_sysctl_deinit(self.ctx);
        }
    }
}

extern "C" fn isp_err_callback(msg: *mut ffi::rk_aiq_err_msg_t) -> i32 {
    return ffi::XCamReturn_XCAM_RETURN_NO_ERROR;
}

extern "C" fn isp_sof_callback(meta: *mut ffi::rk_aiq_metas_t) -> i32 {
    return ffi::XCamReturn_XCAM_RETURN_NO_ERROR;
}
