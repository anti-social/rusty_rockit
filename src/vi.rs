use core::slice;
use std::mem::MaybeUninit;
use std::rc::Rc;
use std::time::Duration;

use rockit_sys::mpi as ffi;

use crate::{
    AcquiredResource, Error, RK_ERR_VI_NOT_CONFIG, RK_SUCCESS, ResourceManager, RockitSys,
    rk_check_err, rk_log_err,
};

pub(crate) type ViCameraResourceManager = ResourceManager<{ ffi::VI_MAX_DEV_NUM as usize }>;
pub(crate) type ViCameraAcquired = AcquiredResource<{ ffi::VI_MAX_DEV_NUM as usize }>;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default)]
pub enum CameraId {
    #[default]
    Zero = 0,
    One = 1,
    Two = 2,
}

impl TryFrom<u8> for CameraId {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Zero,
            1 => Self::One,
            2 => Self::Two,
            _ => return Err(Error::InvalidCameraId { id: value }),
        })
    }
}

pub(crate) struct CameraInner {
    _dev: ffi::rkVI_DEV_ATTR_S,
    id: i32,
    pipe: ffi::rkVI_DEV_BIND_PIPE_S,
    _resource: ViCameraAcquired,
}

impl Drop for CameraInner {
    fn drop(&mut self) {
        log::debug!("Disabling VI device [id = {}]", self.id);
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_DisableDev(self.id),
                "Error disabling rockit device"
            );
        }
    }
}

pub struct CameraOwned {
    inner: Rc<CameraInner>,
    _mpi: RockitSys,
}

impl CameraOwned {
    pub fn get_pipe(&self, pipe_id: u8) -> Option<ViPipeOwned> {
        if pipe_id as u32 >= self.inner.pipe.u32Num {
            return None;
        }
        Some(ViPipeOwned {
            _mpi: self._mpi.clone(),
            camera: Rc::clone(&self.inner),
            id: pipe_id as i32,
        })
    }
}

pub struct Camera<'a> {
    inner: CameraInner,
    _mpi: &'a RockitSys,
}

impl<'a> Camera<'a> {
    pub(crate) fn new(
        mpi: &'a RockitSys,
        dev_id: u8,
        num_pipes: u8, 
    ) -> Result<Self, Error> {
        if dev_id as u32 > ffi::VI_MAX_DEV_NUM {
            return Err(Error::InvalidDevId { id: dev_id });
        }
        if num_pipes as u32 > ffi::VI_MAX_PIPE_NUM {
            return Err(Error::RequestedTooManyPipes { num: num_pipes });
        }
        
        let dev_id = dev_id as i32;
        let (dev, pipe) = unsafe {
            let mut dev = MaybeUninit::zeroed();
            let res = ffi::RK_MPI_VI_GetDevAttr(dev_id, dev.as_mut_ptr());
            if res == RK_ERR_VI_NOT_CONFIG {
                rk_check_err!(ffi::RK_MPI_VI_SetDevAttr(dev_id, dev.as_mut_ptr()));
            }

            if ffi::RK_MPI_VI_GetDevIsEnable(dev_id) != RK_SUCCESS {
                rk_check_err!(ffi::RK_MPI_VI_EnableDev(dev_id));
            }

            let mut pipe = ffi::rkVI_DEV_BIND_PIPE_S {
                u32Num: num_pipes as u32,
                PipeId: [0; ffi::MAX_VI_BIND_PIPE_NUM as usize],
                bDataOffline: 0,
                bUserStartPipe: [0; ffi::MAX_VI_BIND_PIPE_NUM as usize],
            };
            rk_check_err!(ffi::RK_MPI_VI_SetDevBindPipe(dev_id, &mut pipe as *mut _));

            (dev.assume_init(), pipe)
        };

        Ok(Self {
            _mpi: mpi,
            inner: CameraInner {
                id: dev_id,
                _dev: dev,
                pipe,
                _resource: mpi.cameras.acqure_specific(dev_id as usize)?,
            },
        })
    }

    pub fn get_pipe(&self, pipe_id: u8) -> Option<ViPipe<'_>> {
        if pipe_id as u32 >= self.inner.pipe.u32Num {
            return None;
        }
        Some(ViPipe::new(self, pipe_id as i32))
    }

    pub fn into_owned(self) -> CameraOwned {
        CameraOwned {
            _mpi: self._mpi.clone(),
            inner: Rc::new(self.inner),
        }
    }
}

pub struct ViPipe<'a> {
    _dev: &'a Camera<'a>,
    id: i32,
}

pub struct ViPipeOwned {
    id: i32,
    camera: Rc<CameraInner>,
    _mpi: RockitSys,
}

impl ViPipeOwned {
    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn create_channel(
        &self, channel_id: u8, width: u16, height: u16
    ) -> Result<ViChannelOwned, Error> {
        ViChannelInner::new(self.id, channel_id, width, height)
            .map(|inner| ViChannelOwned {
                _mpi: self._mpi.clone(),
                camera: Rc::clone(&self.camera),
                inner: Rc::new(inner),
            })
    } 
}

impl<'a> ViPipe<'a> {
    fn new(dev: &'a Camera<'a>, id: i32) -> Self {
        Self { _dev: dev, id }
    }

    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn create_channel(
        &self, channel_id: u8, width: u16, height: u16
    ) -> Result<ViChannel<'_>, Error> {
        ViChannelInner::new(self.id, channel_id, width, height)
            .map(|inner| ViChannel { pipe: self, inner })
    } 
}

pub(crate) struct ViChannelInner {
    id: i32,
    pipe_id: i32,
}

impl Drop for ViChannelInner {
    fn drop(&mut self) {
        log::debug!("Disabling VI channel [id = {}]", self.id);
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_DisableChn(0, self.id),
                "Error disabling rockit channel"
            );
        }
    }
}

impl ViChannelInner {
    fn new(
        pipe_id: i32,
        channel_id: u8,
        width: u16,
        height: u16,
    ) -> Result<Self, Error> {
        if channel_id as u32 > ffi::VI_MAX_CHN_NUM {
            return Err(Error::InvalidDevId { id: channel_id });
        }
        let channel_id = channel_id as i32;

        unsafe {
            let mut channel = ffi::VI_CHN_ATTR_S {
                stSize: ffi::SIZE_S {
                    u32Width: width as _,
                    u32Height: height as _,
                },
                enPixelFormat: ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV420SP,
                enDynamicRange: ffi::rkDYNAMIC_RANGE_E_DYNAMIC_RANGE_SDR8,
                enVideoFormat: ffi::rkVIDEO_FORMAT_E_VIDEO_FORMAT_LINEAR,
                enCompressMode: ffi::rkCOMPRESS_MODE_E_COMPRESS_MODE_NONE,
                bMirror: 0,
                bFlip: 0,
                u32Depth: 0,
                stFrameRate: ffi::FRAME_RATE_CTRL_S {
                    s32SrcFrameRate: 30,
                    s32DstFrameRate: 30,
                },
                enAllocBufType: ffi::rkVI_ALLOC_BUF_TYPE_E_VI_ALLOC_BUF_TYPE_INTERNAL,
                stIspOpt: ffi::VI_ISP_OPT_S {
                    u32BufCount: 2,
                    u32BufSize: 0,
                    enCaptureType: 0,
                    enMemoryType: ffi::rkVI_V4L2_MEMORY_TYPE_VI_V4L2_MEMORY_TYPE_DMABUF,
                    aEntityName: [0; _],
                    bNoUseLibV4L2: 0,
                    stMaxSize: ffi::SIZE_S {
                        u32Width: 0,
                        u32Height: 0,
                    },
                    stWindow: ffi::RECT_S {
                        s32X: 0,
                        s32Y: 0,
                        u32Width: 0,
                        u32Height: 0,
                    },
                },
                stShareBufChn: ffi::MPP_CHN_S {
                    enModId: ffi::rkMOD_ID_E_RK_ID_CMPI,
                    s32DevId: 0,
                    s32ChnId: 0,
                },
            };
            rk_check_err!(
                ffi::RK_MPI_VI_SetChnAttr(pipe_id, channel_id, &mut channel as *mut _)
            );
            rk_check_err!(ffi::RK_MPI_VI_EnableChn(pipe_id, channel_id));
            channel
        };
        
        Ok(Self { id: channel_id, pipe_id })
    }

    fn get_frame(&self, timeout: Duration) -> Result<ViFrameInner, Error> {
        let frame = unsafe {
            let mut frame = MaybeUninit::zeroed();
            rk_check_err!(
                ffi::RK_MPI_VI_GetChnFrame(
                    0, self.id, frame.as_mut_ptr(), timeout.as_millis() as i32
                )
            );
            frame.assume_init()
        };
        
        Ok(ViFrameInner { frame, pipe_id: self.pipe_id, channel_id: self.id })
    }
}

pub struct ViChannelOwned {
    pub(crate) inner: Rc<ViChannelInner>,
    pub(crate) camera: Rc<CameraInner>,
    _mpi: RockitSys,
}

impl ViChannelOwned {
    pub fn id(&self) -> i32 {
        self.inner.id
    }

    pub fn pipe_id(&self) -> i32 {
        self.inner.pipe_id
    }

    pub fn get_frame(&self, timeout: Duration) -> Result<ViFrameOwned, Error> {
        self.inner.get_frame(timeout)
            .map(|inner| ViFrameOwned {
                _mpi: self._mpi.clone(),
                _camera: Rc::clone(&self.camera),
                _channel: Rc::clone(&self.inner),
                inner,
            })
    }
}

pub struct ViChannel<'a> {
    pipe: &'a ViPipe<'a>,
    inner: ViChannelInner,
}

impl<'a> ViChannel<'a> {
    pub fn id(&self) -> i32 {
        self.inner.id
    }

    pub fn pipe_id(&self) -> i32 {
        self.pipe.id
    }

    pub fn get_frame(&self, timeout: Duration) -> Result<ViFrame<'_>, Error> {
        self.inner.get_frame(timeout)
            .map(|inner| ViFrame { _channel: self, inner })
    }
}

struct ViFrameInner {
    frame: ffi::rkVIDEO_FRAME_INFO_S,
    pipe_id: i32,
    channel_id: i32,
}

impl Drop for ViFrameInner {
    fn drop(&mut self) {
        log::trace!("Releasing VI frame [channel = {}]", self.channel_id);
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_ReleaseChnFrame(
                    self.pipe_id, self.channel_id, &self.frame as *const _
                ),
                "Error releasing channel frame"
            );
        }
    }
}

impl ViFrameInner {
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

pub struct ViFrame<'a> {
    inner: ViFrameInner,
    _channel: &'a ViChannel<'a>,
}

impl<'a> ViFrame<'a> {
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

pub struct ViFrameOwned {
    inner: ViFrameInner,
    _channel: Rc<ViChannelInner>,
    _camera: Rc<CameraInner>,
    _mpi: RockitSys,
}

impl ViFrameOwned {
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
