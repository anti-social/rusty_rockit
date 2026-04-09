use std::any::Any;
use std::marker::PhantomData;
use std::mem::MaybeUninit;

use rockit_sys::aiq as ffi;
use snafu::Snafu;

#[derive(Clone, Debug, Snafu)]
pub enum Error {
    #[snafu(display("AIQ error code: {code}"))]
    Aiq { code: i32 },
    #[snafu(display("Error initializing AIQ system context"))]
    AiqSystemContext,
}

pub mod state {
    pub struct Initialized;
    pub struct Started;
}

pub struct AiqContext<S> {
    inner: AiqContextInner,
    _marker: PhantomData<S>, 
}

impl AiqContext<state::Initialized> {
    pub fn init(cam_id: u8) -> Result<AiqContext<state::Initialized>, Error> {
        let ctx = unsafe {
            let mut aiq_static_info = MaybeUninit::zeroed();
            let res = ffi::rk_aiq_uapi2_sysctl_enumStaticMetas(
                cam_id as i32,
                aiq_static_info.as_mut_ptr(),
            );
            if res != ffi::XCamReturn_XCAM_RETURN_NO_ERROR {
                return Err(Error::Aiq { code: res });
            }
            let aiq_static_info = aiq_static_info.assume_init();

            let ctx_ptr = ffi::rk_aiq_uapi2_sysctl_init(
                &aiq_static_info.sensor_info.sensor_name as *const u8,
                c"/etc/iqfiles".as_ptr(),
                Some(isp_err_callback),
                Some(isp_sof_callback),
            );
            if ctx_ptr.is_null() {
                return Err(Error::AiqSystemContext);
            }
            ctx_ptr
        };

        Ok(Self {
            inner: AiqContextInner {
                ctx,
                state: Box::new(state::Initialized),
            },
            _marker: PhantomData,
        })
    }

    pub fn start(self) -> Result<AiqContext<state::Started>, Error> {
        unsafe {
            let res = ffi::rk_aiq_uapi2_sysctl_prepare(
                self.inner.ctx,
                0,
                0,
                ffi::rk_aiq_working_mode_t_RK_AIQ_WORKING_MODE_NORMAL,
            );
            if res != ffi::XCamReturn_XCAM_RETURN_NO_ERROR {
                return Err(Error::Aiq { code: res });
            }

            let res = ffi::rk_aiq_uapi2_sysctl_start(self.inner.ctx);
            if res != ffi::XCamReturn_XCAM_RETURN_NO_ERROR {
                return Err(Error::Aiq { code: res });
            }
        }

        let mut inner = self.inner;
        inner.state = Box::new(state::Started);
        Ok(AiqContext { inner: inner, _marker: PhantomData })
    }
}

impl AiqContext<state::Started> {
    pub fn stop(self) -> Result<AiqContext<state::Initialized>, Error> {
        self.inner.stop()?;

        let mut inner = self.inner;
        inner.state = Box::new(state::Initialized);
        Ok(AiqContext { inner, _marker: PhantomData })
    }
}

struct AiqContextInner {
    ctx: *mut ffi::rk_aiq_sys_ctx_s,
    state: Box<dyn Any>,
}

impl Drop for AiqContextInner {
    fn drop(&mut self) {
        println!("Dropping AIQ context...");
        if self.state.is::<state::Started>() {
            if let Err(e) = self.stop() {
                eprintln!("Error stopping AIQ: {e}");
            }
        }
        unsafe {
            ffi::rk_aiq_uapi2_sysctl_deinit(self.ctx);
        }
    }
}

impl AiqContextInner {
    fn stop(&self) -> Result<(), Error> {
        unsafe {
            let res = ffi::rk_aiq_uapi2_sysctl_stop(self.ctx, false);
            if res != ffi::XCamReturn_XCAM_RETURN_NO_ERROR {
                return Err(Error::Aiq { code: res });
            }
        }
        Ok(())
    }
}

extern "C" fn isp_err_callback(msg: *mut ffi::rk_aiq_err_msg_t) -> i32 {
    let err_code = unsafe { (*msg).err_code };
    if err_code == ffi::XCamReturn_XCAM_RETURN_BYPASS {
        eprintln!("What should we do on xcam return bypass?");
    }
    return ffi::XCamReturn_XCAM_RETURN_NO_ERROR;
}

extern "C" fn isp_sof_callback(_meta: *mut ffi::rk_aiq_metas_t) -> i32 {
    return ffi::XCamReturn_XCAM_RETURN_NO_ERROR;
}
