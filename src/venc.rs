use core::slice;
use std::any::Any; 
use std::marker::PhantomData;
use std::mem::MaybeUninit;

use rockit_sys::mpi as ffi;

use crate::{Error, RockitSys, rk_check_err, rk_log_err};
use crate::vi::ViChannel;

type rkVENC_H265_CBR_S = ffi::rkVENC_H264_CBR_S;
type rkVENC_H265_VBR_S = ffi::rkVENC_H264_VBR_S;
type rkVENC_H265_AVBR_S = ffi::rkVENC_H264_AVBR_S;

pub mod state {
    pub struct Initialized;
    pub struct Started;
}

#[derive(Clone, Copy, Debug)]
pub enum Codec {
    H264 {
        rate_control: H26xRateControl,
        profile: H264Profile,
    },
    Hevc {
        rate_control: H26xRateControl,
        profile: HevcProfile,
    },
    // TODO: Mjpeg
}

impl Codec {
    fn native_id(&self) -> ffi::rkCODEC_ID_E {
        match self {
            Self::H264 { .. } => ffi::rkCODEC_ID_E_RK_VIDEO_ID_AVC,
            Self::Hevc { .. } => ffi::rkCODEC_ID_E_RK_VIDEO_ID_HEVC,
        }
    }

    fn native_rate_control_mode(&self) -> ffi::rkVENC_RC_MODE_E {
        match self {
            Self::H264 { rate_control: H26xRateControl::Cbr { .. }, .. } => {
                ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H264CBR
            }
            Self::H264 { rate_control: H26xRateControl::Vbr { .. }, .. } => {
                ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H264VBR
            }
            Self::H264 { rate_control: H26xRateControl::Avbr { .. }, .. } => {
                ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H264AVBR
            }
            Self::Hevc { rate_control: H26xRateControl::Cbr { .. }, .. } => {
                ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H265CBR
            }
            Self::Hevc { rate_control: H26xRateControl::Vbr { .. }, .. } => {
                ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H265VBR
            }
            Self::Hevc { rate_control: H26xRateControl::Avbr { .. }, .. } => {
                ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H265AVBR
            }
        }
    }

    fn native_profile(&self) -> u32 {
        match self {
            Self::H264 { profile: H264Profile::Baseline, .. } => {
                ffi::rkH264E_PROFILE_E_H264E_PROFILE_BASELINE
            }
            Self::H264 { profile: H264Profile::Main, .. } => {
                ffi::rkH264E_PROFILE_E_H264E_PROFILE_MAIN
            }
            Self::H264 { profile: H264Profile::High, .. } => {
                ffi::rkH264E_PROFILE_E_H264E_PROFILE_HIGH
            }
            Self::Hevc { profile: HevcProfile::Main, .. } => {
                ffi::rkH265E_PROFILE_E_H265E_PROFILE_MAIN
            }
            Self::Hevc { profile: HevcProfile::Main10, .. } => {
                ffi::rkH265E_PROFILE_E_H265E_PROFILE_MAIN10
            }
        }
    }

    fn native_rate_control_attrs(&self) -> ffi::rkVENC_RC_ATTR_S__bindgen_ty_1 {
        match *self {
            Self::H264 {
                rate_control: H26xRateControl::Cbr { framerate, bitrate_kbps, gop },
                ..
            } => {
                ffi::rkVENC_RC_ATTR_S__bindgen_ty_1 {
                    stH264Cbr: ffi::rkVENC_H264_CBR_S {
                        u32Gop: gop as _,
                        fr32DstFrameRateNum: framerate as _,
                        fr32DstFrameRateDen: 1,
                        u32SrcFrameRateNum: framerate as _,
                        u32SrcFrameRateDen: 1,
                        u32BitRate: bitrate_kbps,
                        u32StatTime: 0,
                    }
                }
            }
            Self::H264 {
                rate_control: H26xRateControl::Vbr {
                    gop, framerate, bitrate_kbps, max_bitrate_kbps, min_bitrate_kbps
                },
                ..
            } => {
                ffi::rkVENC_RC_ATTR_S__bindgen_ty_1 {
                    stH264Vbr: ffi::rkVENC_H264_VBR_S {
                        u32Gop: gop as _,
                        fr32DstFrameRateNum: framerate as _,
                        fr32DstFrameRateDen: 1,
                        u32SrcFrameRateNum: framerate as _,
                        u32SrcFrameRateDen: 1,
                        u32BitRate: bitrate_kbps,
                        u32MaxBitRate: max_bitrate_kbps,
                        u32MinBitRate: min_bitrate_kbps,
                        u32StatTime: 0,
                    }
                }
            }
            Self::H264 {
                rate_control: H26xRateControl::Avbr {
                    gop, framerate, bitrate_kbps, max_bitrate_kbps, min_bitrate_kbps
                },
                ..
            } => {
                ffi::rkVENC_RC_ATTR_S__bindgen_ty_1 {
                    stH264Avbr: ffi::rkVENC_H264_AVBR_S {
                        u32Gop: gop as _,
                        fr32DstFrameRateNum: framerate as _,
                        fr32DstFrameRateDen: 1,
                        u32SrcFrameRateNum: framerate as _,
                        u32SrcFrameRateDen: 1,
                        u32BitRate: bitrate_kbps,
                        u32MaxBitRate: max_bitrate_kbps,
                        u32MinBitRate: min_bitrate_kbps,
                        u32StatTime: 0,
                    }
                }
            }
            Self::Hevc {
                rate_control: H26xRateControl::Cbr { framerate, bitrate_kbps, gop },
                ..
            } => {
                ffi::rkVENC_RC_ATTR_S__bindgen_ty_1 {
                    stH265Cbr: rkVENC_H265_CBR_S {
                        u32Gop: gop as _,
                        fr32DstFrameRateNum: framerate as _,
                        fr32DstFrameRateDen: 1,
                        u32SrcFrameRateNum: framerate as _,
                        u32SrcFrameRateDen: 1,
                        u32BitRate: bitrate_kbps,
                        u32StatTime: 0,
                    }
                }
            }
            Self::Hevc {
                rate_control: H26xRateControl::Vbr {
                    gop, framerate, bitrate_kbps, max_bitrate_kbps, min_bitrate_kbps
                },
                ..
            } => {
                ffi::rkVENC_RC_ATTR_S__bindgen_ty_1 {
                    stH265Vbr: rkVENC_H265_VBR_S {
                        u32Gop: gop as _,
                        fr32DstFrameRateNum: framerate as _,
                        fr32DstFrameRateDen: 1,
                        u32SrcFrameRateNum: framerate as _,
                        u32SrcFrameRateDen: 1,
                        u32BitRate: bitrate_kbps,
                        u32MaxBitRate: max_bitrate_kbps,
                        u32MinBitRate: min_bitrate_kbps,
                        u32StatTime: 0,
                    }
                }
            }
            Self::Hevc {
                rate_control: H26xRateControl::Avbr {
                    gop, framerate, bitrate_kbps, max_bitrate_kbps, min_bitrate_kbps
                },
                ..
            } => {
                ffi::rkVENC_RC_ATTR_S__bindgen_ty_1 {
                    stH265Avbr: rkVENC_H265_AVBR_S {
                        u32Gop: gop as _,
                        fr32DstFrameRateNum: framerate as _,
                        fr32DstFrameRateDen: 1,
                        u32SrcFrameRateNum: framerate as _,
                        u32SrcFrameRateDen: 1,
                        u32BitRate: bitrate_kbps,
                        u32MaxBitRate: max_bitrate_kbps,
                        u32MinBitRate: min_bitrate_kbps,
                        u32StatTime: 0,
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum H26xRateControl {
    Cbr {
        gop: u16,
        framerate: u8,
        bitrate_kbps: u32,
    },
    Vbr {
        gop: u16,
        framerate: u8,
        bitrate_kbps: u32,
        max_bitrate_kbps: u32,
        min_bitrate_kbps: u32,
    },
    Avbr {
        gop: u16,
        framerate: u8,
        bitrate_kbps: u32,
        max_bitrate_kbps: u32,
        min_bitrate_kbps: u32,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum H264Profile {
    Baseline,
    Main,
    High,
}

#[derive(Clone, Copy, Debug)]
pub enum HevcProfile {
    Main,
    Main10,
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

#[derive(Clone, Debug)]
pub struct VencConfig {
    pub width: u16,
    pub height: u16,
    pub codec: Codec,
    pub buf_count: u8,
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
        mpi: &'a RockitSys, channel_id: u8, cfg: &VencConfig
    ) -> Result<VencChannel<'a, state::Initialized>, Error> {
        let channel_id = channel_id as i32;
        let width = cfg.width as u32;
        let height = cfg.height as u32;
        unsafe {
            let channel_attr = ffi::rkVENC_CHN_ATTR_S {
                stRcAttr: ffi::rkVENC_RC_ATTR_S {
                    enRcMode: cfg.codec.native_rate_control_mode(),
                    __bindgen_anon_1: cfg.codec.native_rate_control_attrs(),
                },
                stGopAttr: ffi::rkVENC_GOP_ATTR_S {
                    enGopMode: ffi::rkVENC_GOP_MODE_E_VENC_GOPMODE_INIT,
                    s32VirIdrLen: 0,
                    u32MaxLtrCount: 0,
                    u32TsvcPreload: 0,
                },
                stVencAttr: ffi::rkVENC_ATTR_S {
                    enType: cfg.codec.native_id(),
                    enPixelFormat: ffi::rkPIXEL_FORMAT_E_RK_FMT_YUV420SP,
                    u32Profile: cfg.codec.native_profile(),
                    u32PicWidth: width,
                    u32PicHeight: height,
                    u32MaxPicWidth: width,
                    u32MaxPicHeight: height,
                    u32VirWidth: width,
                    u32VirHeight: height,
                    u32StreamBufCnt: 3,
                    u32BufSize: width * height * 3 / 2,
                    bByFrame: false as u32,
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
        // println!(
        //     "Releasing encoder stream: channel = {}",
        //     self.channel.venc_channel.id(),
        // );
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
        let data = unsafe {
            let packet = *self.frame.frame.pstPack;
            let data_ptr = ffi::RK_MPI_MB_Handle2VirAddr(packet.pMbBlk);
            if data_ptr.is_null() {
                return Err(Error::InvalidFramePointer);
            }
            slice::from_raw_parts(
                data_ptr as *const u8,
                packet.u32Len as usize
            )
        };
        Ok(data)
    }
}
