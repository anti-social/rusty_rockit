use core::slice;
use std::mem::MaybeUninit;

use rockit_sys::mpi as ffi;

use crate::{Error, rk_check_err, rk_log_err, RK_SUCCESS, RK_ERR_VI_NOT_CONFIG, RockitSys};

pub struct Camera<'a> {
    _mpi: &'a RockitSys,
    _dev: ffi::rkVI_DEV_ATTR_S,
    id: i32,
    pipe: ffi::rkVI_DEV_BIND_PIPE_S,
}

impl<'a> Drop for Camera<'a> {
    fn drop(&mut self) {
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_DisableDev(self.id),
                "Error disabling rockit device"
            );
        }
    }
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
                PipeId: [0; 16usize],
                bDataOffline: 0,
                bUserStartPipe: [0; 16],
            };
            rk_check_err!(ffi::RK_MPI_VI_SetDevBindPipe(dev_id, &mut pipe as *mut _));

            (dev.assume_init(), pipe)
        };

        Ok(Self { _mpi: mpi, id: dev_id, _dev: dev, pipe })
    }

    pub fn get_pipe(&self, pipe_id: u8) -> Option<ViPipe<'_>> {
        if pipe_id as u32 >= self.pipe.u32Num {
            return None;
        }
        Some(ViPipe::new(self, pipe_id as i32))
    }
}

pub struct ViPipe<'a> {
    _dev: &'a Camera<'a>,
    id: i32,
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
        ViChannel::new(self, channel_id, width, height)
    } 
}

pub struct ViChannel<'a> {
    pipe: &'a ViPipe<'a>,
    id: i32,
}

impl<'a> ViChannel<'a> {
    fn new(
        pipe: &'a ViPipe<'a>,
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
                ffi::RK_MPI_VI_SetChnAttr(pipe.id, channel_id, &mut channel as *mut _)
            );
            rk_check_err!(ffi::RK_MPI_VI_EnableChn(pipe.id, channel_id));
            channel
        };
        
        Ok(Self { pipe, id: channel_id })
    }

    pub fn id(&self) -> i32 {
        self.id
    }

    pub fn pipe_id(&self) -> i32 {
        self.pipe.id
    }

    pub fn get_frame(&self) -> Result<ViFrame<'_>, Error> {
        let vi_frame = unsafe {
            let mut vi_frame = MaybeUninit::zeroed();
            rk_check_err!(
                ffi::RK_MPI_VI_GetChnFrame(0, self.id, vi_frame.as_mut_ptr(), 1000)
            );
            vi_frame.assume_init()
        };

        Ok(ViFrame { channel: self, frame: vi_frame })
    }
}

impl<'a> Drop for ViChannel<'a> {
    fn drop(&mut self) {
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_DisableChn(0, self.id),
                "Error disabling rockit channel"
            );
        }
    }
}

pub struct ViFrame<'a> {
    channel: &'a ViChannel<'a>,
    frame: ffi::rkVIDEO_FRAME_INFO_S,
}

impl<'a> Drop for ViFrame<'a> {
    fn drop(&mut self) {
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VI_ReleaseChnFrame(
                    self.channel.pipe.id, self.channel.id, &self.frame as *const _
                ),
                "Error releasing channel frame"
            );
        }
    }
}

impl<'a> ViFrame<'a> {
    pub fn width(&self) -> u32 {
        self.frame.stVFrame.u32Width
    }

    pub fn height(&self) -> u32 {
        self.frame.stVFrame.u32Height
    }

    pub fn data(&self) -> Result<&[u8], Error> {
        let data = unsafe {
            let data_ptr = ffi::RK_MPI_MB_Handle2VirAddr(self.frame.stVFrame.pMbBlk);
            if data_ptr.is_null() {
                return Err(Error::InvalidFramePointer);
            }
            slice::from_raw_parts(
                data_ptr as *const u8,
                self.frame.stVFrame.u32Width as usize * self.frame.stVFrame.u32Height as usize * 3 / 2
            )
        };
        Ok(data)
    }
}
