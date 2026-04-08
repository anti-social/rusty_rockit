use core::slice;
use std::any::Any; 
use std::marker::PhantomData;
use std::mem::MaybeUninit;

use rockit_sys::mpi as ffi;

use crate::{Error, RockitSys, rk_check_err, rk_log_err};
use crate::vi::ViChannel;

pub mod state {
    pub struct Initialized;
    pub struct Started;
}

struct VencChannelInner {
    id: i32,
    state: Box<dyn Any>,
}

impl Drop for VencChannelInner {
    fn drop(&mut self) {
        println!("Dropping encoder channel in state {:?}: {}", self.state, self.id);
        if self.state.is::<state::Started>() {
            unsafe {
                rk_log_err!(
                    ffi::RK_MPI_VENC_StopRecvFrame(self.id),
                    "Error stopping encoder"
                );
            }
        }
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VENC_DestroyChn(self.id),
                "Error destroying encoder channel"
            );
        }
    }
}

pub struct VencChannel<'a, S> {
    inner: VencChannelInner,
    _mpi: &'a RockitSys,
    _marker: PhantomData<S>,
}

impl<'a, S> VencChannel<'a, S> {
    fn id(&self) -> i32 {
        self.inner.id
    }
}

impl<'a> VencChannel<'a, state::Initialized> {
    pub fn new(
        mpi: &'a RockitSys, channel_id: u8, width: u16, height: u16
    ) -> Result<VencChannel<'a, state::Initialized>, Error> {
        let channel_id = channel_id as i32;
        let width = width as u32;
        let height = height as u32;
        unsafe {
            let channel_attr = ffi::rkVENC_CHN_ATTR_S {
                stRcAttr: ffi::rkVENC_RC_ATTR_S {
                    enRcMode: ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H264CBR,
                    __bindgen_anon_1: ffi::rkVENC_RC_ATTR_S__bindgen_ty_1 {
                        stH264Cbr: ffi::rkVENC_H264_CBR_S {
                            u32BitRate: 8 * 1024,
                            u32Gop: 60,
                            fr32DstFrameRateDen: 1,
                            fr32DstFrameRateNum: 30,
                            u32SrcFrameRateDen: 1,
                            u32SrcFrameRateNum: 30,
                            u32StatTime: 0,
                        },
                    }
                },
                stGopAttr: ffi::rkVENC_GOP_ATTR_S {
                    enGopMode: ffi::rkVENC_GOP_MODE_E_VENC_GOPMODE_INIT,
                    s32VirIdrLen: 0,
                    u32MaxLtrCount: 0,
                    u32TsvcPreload: 0,
                },
                stVencAttr: ffi::rkVENC_ATTR_S {
                    enType: ffi::rkCODEC_ID_E_RK_VIDEO_ID_AVC,
                    enPixelFormat: ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV420SP,
                    u32Profile: ffi::rkH264E_PROFILE_E_H264E_PROFILE_HIGH,
                    u32PicWidth: width,
                    u32PicHeight: height,
                    u32MaxPicWidth: width,
                    u32MaxPicHeight: height,
                    u32VirWidth: width,
                    u32VirHeight: height,
                    u32StreamBufCnt: 3,
                    u32BufSize: width * height * 3 / 2,
                    bByFrame: true as u32,
                    enMirror: ffi::rkMIRROR_E_MIRROR_NONE,
                    __bindgen_anon_1: ffi::rkVENC_ATTR_S__bindgen_ty_1 {
                        stAttrH264e: ffi::rkVENC_ATTR_H264_S {
                            u32Level: 0,
                        }
                    },
                },
            };
            rk_check_err!(
                ffi::RK_MPI_VENC_CreateChn(channel_id, &channel_attr as *const _)
            );
        }

        Ok(Self {
            inner: VencChannelInner {
                id: channel_id,
                state: Box::new(state::Initialized),
            },
            _mpi: mpi,
            _marker: PhantomData,
        })
    }

    pub fn start(self) -> Result<VencChannel<'a, state::Started>, Error> {
        unsafe {
            let recv_param = ffi::rkVENC_RECV_PIC_PARAM_S {
                s32RecvPicNum: -1,
            };
            rk_check_err!(
                ffi::RK_MPI_VENC_StartRecvFrame(self.id(), &recv_param as *const _)
            );
        }
        let mut inner = self.inner;
        inner.state = Box::new(state::Started);
        Ok(VencChannel {
            inner: inner,
            _mpi: self._mpi,
            _marker: PhantomData,
        })
    }
}

impl<'a> VencChannel<'a, state::Started> {
    pub fn bind(&'a self, vi_channel: &'a ViChannel) -> Result<VencChannelBind<'a>, Error> {
        unsafe {
            let src_channel = ffi::rkMPP_CHN_S {
                enModId: ffi::rkMOD_ID_E_RK_ID_VI,
                s32DevId: vi_channel.pipe_id(),
                s32ChnId: vi_channel.id(),
            };
            let dst_channel = ffi::rkMPP_CHN_S {
                enModId: ffi::rkMOD_ID_E_RK_ID_VENC,
                s32DevId: 0,
                s32ChnId: self.id(),
            };
            rk_check_err!(
                ffi::RK_MPI_SYS_Bind(&src_channel as *const _, &dst_channel as *const _)
            );
        }

        Ok(VencChannelBind { vi_channel, venc_channel: self })
    }

    pub fn stop(self) -> Result<VencChannel<'a, state::Initialized>, Error> {
        println!("Stopping encoder: {}", self.id());
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VENC_StopRecvFrame(self.id()),
                "Error stopping receiving frames by encoder"
            );
        }
        let mut inner = self.inner;
        inner.state = Box::new(state::Started);
        Ok(VencChannel {
            inner,
            _mpi: self._mpi,
            _marker: PhantomData,
        })
    }
}

pub struct VencChannelBind<'a> {
    vi_channel: &'a ViChannel<'a>,
    venc_channel: &'a VencChannel<'a, state::Started>,
}

impl<'a> Drop for VencChannelBind<'a> {
    fn drop(&mut self) {
        println!(
            "Dropping bound encoder channel: vi channel = {}, venc channel = {}",
            self.vi_channel.id(),
            self.venc_channel.id(),
        );
        unsafe {
            let src_channel = ffi::rkMPP_CHN_S {
                enModId: ffi::rkMOD_ID_E_RK_ID_VI,
                s32DevId: 0,
                s32ChnId: self.vi_channel.id(),
            };
            let dst_channel = ffi::rkMPP_CHN_S {
                enModId: ffi::rkMOD_ID_E_RK_ID_VENC,
                s32DevId: 0,
                s32ChnId: self.venc_channel.id(),
            };
            rk_log_err!(
                ffi::RK_MPI_SYS_UnBind(&src_channel as *const _, &dst_channel as *const _),
                "Error unbinding encoder channel"
            );
        }
    }
}

impl<'a> VencChannelBind<'a> {
    pub fn alloc_frame(&self) -> StreamFrame {
        unsafe {
            let mut frame = MaybeUninit::<ffi::rkVENC_STREAM_S>::zeroed();
            let mut packet = Box::new(std::mem::zeroed::<ffi::VENC_PACK_S>());
            (*frame.as_mut_ptr()).pstPack = &mut *packet;
            StreamFrame {
                frame: frame.assume_init(), _packet: packet,
            }
        }
    }

    pub fn get_stream<'b>(&'a self, frame: &'b mut StreamFrame) -> Result<VencStream<'a, 'b>, Error> {
        unsafe {
            rk_check_err!(
                ffi::RK_MPI_VENC_GetStream(
                    self.venc_channel.id(), &mut frame.frame as *mut _, 200
                )
            );
        }

        Ok(VencStream { channel: self, frame })
    }
}

pub struct StreamFrame {
    frame: ffi::rkVENC_STREAM_S,
    _packet: Box<ffi::VENC_PACK_S>,
}

pub struct VencStream<'a, 'b> {
    channel: &'a VencChannelBind<'a>,
    frame: &'b mut StreamFrame,
}

impl<'a, 'b> Drop for VencStream<'a, 'b> {
    fn drop(&mut self) {
        println!(
            "Releasing encoder stream: channel = {}",
            self.channel.venc_channel.id(),
        );
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VENC_ReleaseStream(
                    self.channel.venc_channel.id(), &mut self.frame.frame as *mut _
                ),
                "Error releasing encoder stream"
            );
        }
    }
}

impl<'a, 'b> VencStream<'a, 'b> {
    pub fn data(&self) -> Result<&'b [u8], Error> {
        println!("Getting data from stream: {}", self.channel.venc_channel.id());
        let data = unsafe {
            let packet = *self.frame.frame.pstPack;
            let data_ptr = ffi::RK_MPI_MB_Handle2VirAddr(packet.pMbBlk);
            if data_ptr.is_null() {
                return Err(Error::InvalidFramePointer);
            }
            println!("!!! {:x?} of len {}", data_ptr, packet.u32Len);
            slice::from_raw_parts(
                data_ptr as *const u8,
                packet.u32Len as usize
            )
        };
        Ok(data)
    }
}
