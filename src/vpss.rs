use core::marker::PhantomData;
use core::slice;
use core::sync::atomic::{AtomicU8, Ordering};
use std::mem::MaybeUninit;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::mb::{MbFrame, MbFrameInner, MbFrameOwned};
use crate::{Error, PixelFormat, RockitSys, ffi, rk_check_err, rk_log_err};

#[derive(Debug, Clone, Copy)]
pub struct VpssGroupConfig {
    pub pixel_format: PixelFormat,
    pub max_width: u16,
    pub max_height: u16,
    pub frame_rate: FrameRateControl,
}

#[derive(Debug, Clone, Copy)]
pub struct VpssChannelConfig {
    pub pixel_format: PixelFormat,
    pub width: u16,
    pub height: u16,
    pub frame_rate: FrameRateControl,
    pub mirror: bool,
    pub flip: bool,
    pub queue_size: u8,
    pub frame_buffer_count: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameRateControl {
    pub src: u8,
    pub dst: u8,
}

pub mod state {
    pub struct Initialized;
    pub struct Started;

    #[repr(u8)]
    pub(crate) enum Runtime {
        Initialized,
        Started,
    }
}

pub struct VpssGroupInner {
    id: i32,
    state: Arc<AtomicU8>,
}

impl Drop for VpssGroupInner {
    fn drop(&mut self) {
        if self.state.load(Ordering::Relaxed) == state::Runtime::Started as u8 {
            log::debug!("Stopping VPSS group: {}", self.id);
            unsafe {
                rk_log_err!(
                    ffi::RK_MPI_VPSS_StopGrp(self.id),
                    "Error stopping VPSS group"
                );
            }
        }
        log::debug!("Destroying VPSS group: {}", self.id);
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VPSS_DestroyGrp(self.id),
                "Error destroying VPSS group"
            );
        }
    }
}

impl VpssGroupInner {
    fn new(id: i32, cfg: &VpssGroupConfig) -> Result<Self, Error> {
        unsafe {
            let attrs = ffi::rkVPSS_GRP_ATTR_S {
                u32MaxW: cfg.max_width as _,
                u32MaxH: cfg.max_height as _,
                enPixelFormat: cfg.pixel_format.native_format(),
                enDynamicRange: ffi::rkDYNAMIC_RANGE_E_DYNAMIC_RANGE_SDR8,
                stFrameRate: ffi::rkFRAME_RATE_CTRL_S {
                    s32SrcFrameRate: -1,
                    s32DstFrameRate: -1,
                },
                enCompressMode: ffi::rkCOMPRESS_MODE_E_COMPRESS_MODE_NONE,
            };
            rk_check_err!(ffi::RK_MPI_VPSS_CreateGrp(id, &attrs as *const _));
        }
        Ok(Self {
            id, state: Arc::new(AtomicU8::new(state::Runtime::Initialized as u8)),
        })
    }

    fn set_channel(
        &self, channel_id: i32, cfg: &VpssChannelConfig
    ) -> Result<VpssChannelInner, Error> {
        unsafe {
            let channel_attrs = ffi::rkVPSS_CHN_ATTR_S {
                enChnMode: ffi::rkVPSS_CHN_MODE_E_VPSS_CHN_MODE_USER,
                u32Width: cfg.width as _,
                u32Height: cfg.height as _,
                enVideoFormat: ffi::rkVIDEO_FORMAT_E_VIDEO_FORMAT_LINEAR,
                enPixelFormat: cfg.pixel_format.native_format(),
                enDynamicRange: ffi::rkDYNAMIC_RANGE_E_DYNAMIC_RANGE_SDR8,
                enCompressMode: ffi::rkCOMPRESS_MODE_E_COMPRESS_MODE_NONE,
                stFrameRate: ffi::rkFRAME_RATE_CTRL_S {
                    s32SrcFrameRate: -1,
                    s32DstFrameRate: -1,
                },
                bMirror: cfg.mirror as _,
                bFlip: cfg.flip as _,
                u32Depth: cfg.queue_size as _,
                stAspectRatio: ffi::rkASPECT_RATIO_S {
                    enMode: ffi::rkASPECT_RATIO_E_ASPECT_RATIO_NONE,
                    u32BgColor: 0,
                    stVideoRect: ffi::rkRECT_S {
                        s32X: 0,
                        s32Y: 0,
                        u32Width: 0,
                        u32Height: 0,
                        // u32Width: cfg.width as _,
                        // u32Height: cfg.height as _,
                    }
                },
                u32FrameBufCnt: cfg.frame_buffer_count as _,
            };
            rk_check_err!(
                ffi::RK_MPI_VPSS_SetChnAttr(self.id, channel_id, &channel_attrs as *const _)
            );
        }
        Ok(VpssChannelInner {
            id: channel_id,
            group_id: self.id,
            state: Arc::new(AtomicU8::new(channel_state::Runtime::Disabed as _)),
        })
    }
    
    fn start(&self) -> Result<(), Error> {
        unsafe {
            rk_check_err!(ffi::RK_MPI_VPSS_StartGrp(self.id));
        }
        self.state.store(state::Runtime::Started as u8, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn send_frame(
        &self, pipe_id: i32, frame: &mut MbFrameInner, timeout: Duration
    ) -> Result<(), Error> {
        log::trace!("Sending VPSS frame: group = {}", self.id);
        unsafe {
            rk_check_err!(
                ffi::RK_MPI_VPSS_SendFrame(
                    self.id,
                    pipe_id,
                    frame.frame() as *const _,
                    timeout.as_millis() as i32,
                )
            );
        }
        Ok(())
    }
}

pub struct VpssGroup<'a, S> {
    inner: VpssGroupInner,
    _mpi: &'a RockitSys,
    _marker: PhantomData<S>,
}

impl<'a, S> VpssGroup<'a, S> {
    pub fn id(&self) -> i32 {
        self.inner.id
    }

    pub fn set_channel(
        &'a self, channel_id: u8, cfg: &VpssChannelConfig
    ) -> Result<VpssChannel<'a, channel_state::Disabled>, Error> {
        self.inner.set_channel(channel_id as i32, cfg)
            .map(|inner| VpssChannel {
                inner,
                group: &self.inner,
                _mpi: &self._mpi,
                _marker: PhantomData,
            })
    }    
}

impl<'a> VpssGroup<'a, state::Initialized> {
    pub(crate) fn new(
        mpi: &'a RockitSys, id: u8, cfg: &VpssGroupConfig
    ) -> Result<VpssGroup<'a, state::Initialized>, Error> {
        VpssGroupInner::new(id as i32, cfg)
            .map(|inner| VpssGroup {
                inner: inner,
                _mpi: mpi,
                _marker: PhantomData,
            })
    }

    pub fn start(self) -> Result<VpssGroup<'a, state::Started>, Error> {
        self.inner.start()?;
        Ok(VpssGroup {
            inner: self.inner,
            _mpi: self._mpi,
            _marker: PhantomData,
        })
    }

    pub fn into_owned(self) -> VpssGroupOwned<state::Initialized> {
        VpssGroupOwned {
            inner: Rc::new(self.inner),
            _mpi: self._mpi.clone(),
            _marker: self._marker,
        }
    }
}

impl<'a> VpssGroup<'a, state::Started> {
    pub fn send_frame(
        &self, pipe_id: u8, frame: &mut MbFrame, timeout: Duration
    ) -> Result<(), Error> {
        self.inner.send_frame(pipe_id as i32, &mut frame.inner, timeout)
    }
}

pub struct VpssGroupOwned<S> {
    inner: Rc<VpssGroupInner>,
    _mpi: RockitSys,
    _marker: PhantomData<S>,
}

impl<S> VpssGroupOwned<S> {
    pub fn id(&self) -> i32 {
        self.inner.id
    }

    pub fn set_channel(
        &self, channel_id: u8, cfg: &VpssChannelConfig
    ) -> Result<VpssChannelOwned<channel_state::Disabled>, Error> {
        self.inner.set_channel(channel_id as i32, cfg)
            .map(|inner| VpssChannelOwned {
                inner: Rc::new(inner),
                group: Rc::clone(&self.inner),
                _mpi: self._mpi.clone(),
                _marker: PhantomData,
            })
    }    
}

impl VpssGroupOwned<state::Initialized> {
    pub fn start(self) -> Result<VpssGroupOwned<state::Started>, Error> {
        self.inner.start()?;
        Ok(VpssGroupOwned {
            inner: self.inner,
            _mpi: self._mpi,
            _marker: PhantomData,
        })
    }
}

impl VpssGroupOwned<state::Started> {
    pub fn send_frame(
        &self, pipe_id: u8, frame: &mut MbFrameOwned, timeout: Duration
    ) -> Result<(), Error> {
        self.inner.send_frame(pipe_id as i32, &mut frame.inner, timeout)
    }
}

pub mod channel_state {
    pub struct Disabled;
    pub struct Enabled;

    #[repr(u8)]
    pub(crate) enum Runtime {
        Disabed,
        Enabled,
    }
}

pub(crate) struct VpssChannelInner {
    id: i32,
    group_id: i32,
    state: Arc<AtomicU8>,
}

impl Drop for VpssChannelInner {
    fn drop(&mut self) {
        if self.state.load(Ordering::Relaxed) == channel_state::Runtime::Enabled as u8 {
            if let Err(e) = self.disable() {
                log::error!("Error disabling VPSS channel: {e}");
            }
        }
    }
}

impl VpssChannelInner {
    fn enable(&self) -> Result<(), Error> {
        log::debug!(
            "Enabling VPSS channel: group = {}, channel = {}", self.group_id, self.id
        );
        unsafe {
            rk_check_err!(
                ffi::RK_MPI_VPSS_EnableChn(self.group_id, self.id)
            );
        }
        self.state.store(channel_state::Runtime::Enabled as u8, Ordering::Relaxed);
        Ok(())
    }

    fn disable(&self) -> Result<(), Error> {
        log::debug!(
            "Disabling VPSS channel: group = {}, channel = {}", self.group_id, self.id
        );
        unsafe {
            rk_check_err!(
                ffi::RK_MPI_VPSS_DisableChn(self.group_id, self.id)
            );
        }
        self.state.store(channel_state::Runtime::Disabed as u8, Ordering::Relaxed);
        Ok(())
    }

    fn get_frame(&self, timeout: Duration) -> Result<VpssFrameInner, Error> {
        log::trace!(
            "Getting VPSS channel frame: group = {}, channel = {}",
            self.group_id, self.id
        );
        let frame = unsafe {
            let mut frame = MaybeUninit::zeroed();
            rk_check_err!(
                ffi::RK_MPI_VPSS_GetChnFrame(
                    self.group_id, self.id, frame.as_mut_ptr(), timeout.as_millis() as i32
                )
            );
            frame.assume_init()
        };
        Ok(VpssFrameInner {
            frame,
            group_id: self.group_id,
            channel_id: self.id,
        })
    }
}

pub struct VpssChannel<'a, S> {
    pub(crate) inner: VpssChannelInner,
    pub(crate) group: &'a VpssGroupInner,
    _mpi: &'a RockitSys,
    _marker: PhantomData<S>,
}

impl<'a, S> VpssChannel<'a, S> {
    pub fn id(&self) -> i32 {
        self.inner.id
    }
}

impl<'a> VpssChannel<'a, channel_state::Disabled> {
    pub fn enable(self) -> Result<VpssChannel<'a, channel_state::Enabled>, Error> {
        self.inner.enable()?;
        Ok(VpssChannel {
            inner: self.inner,
            group: &self.group,
            _mpi: &self._mpi,
            _marker: PhantomData,
        })
    }
}

impl<'a> VpssChannel<'a, channel_state::Enabled> {
    pub fn get_frame(&'a self, timeout: Duration) -> Result<VpssFrame<'a>, Error> {
        self.inner.get_frame(timeout)
            .map(|inner| VpssFrame {
                inner,
                _channel: &self.inner,
                _group: &self.group,
                _mpi: self._mpi.clone(),
            })
    }

    pub fn disable(self) -> Result<VpssChannel<'a, channel_state::Disabled>, Error> {
        self.inner.disable()?;
        Ok(VpssChannel {
            inner: self.inner,
            group: &self.group,
            _mpi: &self._mpi,
            _marker: PhantomData,
        })
    }
}

pub struct VpssChannelOwned<S> {
    pub(crate) inner: Rc<VpssChannelInner>,
    pub(crate) group: Rc<VpssGroupInner>,
    _mpi: RockitSys,
    _marker: PhantomData<S>,
}

impl<S> VpssChannelOwned<S> {
    pub fn id(&self) -> i32 {
        self.inner.id
    }
    
    pub fn enable(self) -> Result<VpssChannelOwned<channel_state::Enabled>, Error> {
        self.inner.enable()?;
        Ok(VpssChannelOwned {
            inner: self.inner,
            group: self.group,
            _mpi: self._mpi,
            _marker: PhantomData,
        })
    }
}

impl VpssChannelOwned<channel_state::Enabled> {
    pub fn get_frame(&self, timeout: Duration) -> Result<VpssFrameOwned, Error> {
        self.inner.get_frame(timeout)
            .map(|inner| VpssFrameOwned {
                inner,
                _channel: Rc::clone(&self.inner),
                _group: Rc::clone(&self.group),
                _mpi: self._mpi.clone(),
            })
    }

    pub fn disable(self) -> Result<VpssChannelOwned<channel_state::Disabled>, Error> {
        self.inner.disable()?;
        Ok(VpssChannelOwned {
            inner: Rc::clone(&self.inner),
            group: Rc::clone(&self.group),
            _mpi: self._mpi.clone(),
            _marker: PhantomData,
        })
    }
}

struct VpssFrameInner {
    frame: ffi::rkVIDEO_FRAME_INFO_S,
    group_id: i32,
    channel_id: i32,
}

impl Drop for VpssFrameInner {
    fn drop(&mut self) {
        log::trace!(
            "Releasing VPSS frame: group = {}, channel = {}",
            self.group_id,
            self.channel_id,
        );
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VPSS_ReleaseChnFrame(
                    self.group_id, self.channel_id, &self.frame as *const _
                ),
                "Error releasing VPSS frame"
            );
        }
    }
}

impl VpssFrameInner {
    pub fn data(&self) -> Result<&[u8], Error> {
        let frame = self.frame;
        let data = unsafe {
            let data_ptr = ffi::RK_MPI_MB_Handle2VirAddr(frame.stVFrame.pMbBlk);
            if data_ptr.is_null() {
                return Err(Error::InvalidFramePointer);
            }
            slice::from_raw_parts(
                data_ptr as *const u8,
                frame.stVFrame.u32Width as usize * frame.stVFrame.u32Height as usize * 3 / 2
            )
        };
        Ok(data)
    }
}

pub struct VpssFrame<'a> {
    inner: VpssFrameInner,
    _channel: &'a VpssChannelInner,
    _group: &'a VpssGroupInner,
    _mpi: RockitSys,
}

impl<'a> VpssFrame<'a> {
    pub fn width(&self) -> u32 {
        self.inner.frame.stVFrame.u32Width
    }

    pub fn height(&self) -> u32 {
        self.inner.frame.stVFrame.u32Height
    }

    pub fn data(&self) -> Result<&[u8], Error> {
        self.inner.data()
    }
}

pub struct VpssFrameOwned {
    inner: VpssFrameInner,
    _channel: Rc<VpssChannelInner>,
    _group: Rc<VpssGroupInner>,
    _mpi: RockitSys,
}

impl VpssFrameOwned {
    pub fn width(&self) -> u32 {
        self.inner.frame.stVFrame.u32Width
    }

    pub fn height(&self) -> u32 {
        self.inner.frame.stVFrame.u32Height
    }

    pub fn data(&self) -> Result<&[u8], Error> {
        self.inner.data()
    }
}
