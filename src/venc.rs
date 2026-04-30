use core::slice;
use std::sync::atomic::{AtomicU8, Ordering};
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;

use rockit_sys::mpi as ffi;

use crate::vpss::VpssChannelOwned;
use crate::{AcquiredResource, ChannelBind, PixelFormat, ResourceManager};
use crate::mb::{MbFrame, MbFrameInner, MbFrameOwned};
use crate::{Error, RockitMpi, rk_check_err, rk_log_err};
use crate::vi::{CameraInner, ViChannel, ViChannelInner, ViChannelOwned};
use crate::vpss::{self, VpssChannel, VpssChannelInner, VpssGroupInner};

#[allow(non_camel_case_types)]
type rkVENC_H265_CBR_S = ffi::rkVENC_H264_CBR_S;
#[allow(non_camel_case_types)]
type rkVENC_H265_VBR_S = ffi::rkVENC_H264_VBR_S;
#[allow(non_camel_case_types)]
type rkVENC_H265_AVBR_S = ffi::rkVENC_H264_AVBR_S;

pub(crate) type VencChannelResourceManager = ResourceManager<{ ffi::VENC_MAX_CHN_NUM as usize }>;
pub(crate) type VencChannelAcquired = AcquiredResource<{ ffi::VENC_MAX_CHN_NUM as usize }>;

pub mod state {
    pub struct Initialized;
    impl Initialized {
        pub(super) const VALUE: u8 = 0;
    }

    pub struct Started;
    impl Started {
        pub(super) const VALUE: u8 = 1;
    }
}

#[derive(Clone, Debug)]
pub struct VencConfig {
    pub pixel_format: PixelFormat,
    pub width: u16,
    pub height: u16,
    pub codec: Codec,
    pub buf_count: u8,
}

impl VencConfig {
    pub fn calc_buffer_size(&self) -> u32 {
        let bytes_per_pixel = self.pixel_format.bytes_per_pixel();
        self.width as u32 * self.height as u32 *
            bytes_per_pixel.0 as u32 / bytes_per_pixel.1 as u32
    }
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

    pub fn framerate(&self) -> u8 {
        match self {
            Self::H264 { rate_control: H26xRateControl::Cbr { framerate, .. }, .. } => {
                *framerate
            }
            Self::H264 { rate_control: H26xRateControl::Vbr { framerate, .. }, .. } => {
                *framerate
            }
            Self::H264 { rate_control: H26xRateControl::Avbr { framerate, .. }, .. } => {
                *framerate
            }
            Self::Hevc { rate_control: H26xRateControl::Cbr { framerate, .. }, .. } => {
                *framerate
            }
            Self::Hevc { rate_control: H26xRateControl::Vbr { framerate, .. }, .. } => {
                *framerate
            }
            Self::Hevc { rate_control: H26xRateControl::Avbr { framerate, .. }, .. } => {
                *framerate
            }
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
    state: AtomicU8,
    _resource: VencChannelAcquired,
}

impl Drop for VencChannelInner {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            log::error!("Error stopping VENC channel [id = {}]: {e}", self.id);
        }
        log::debug!("Destroying VENC channel [id = {}]", self.id);
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VENC_DestroyChn(self.id),
                "Error destroying encoder channel"
            );
        }
    }
}

impl VencChannelInner {
    pub fn start(&self) -> Result<(), Error> {
        unsafe {
            let recv_param = ffi::rkVENC_RECV_PIC_PARAM_S {
                s32RecvPicNum: -1,
            };
            rk_check_err!(
                ffi::RK_MPI_VENC_StartRecvFrame(self.id, &recv_param as *const _)
            );
        }
        self.state.store(state::Started::VALUE, Ordering::Relaxed);
        Ok(())
    }
    
    fn stop(&self) -> Result<(), Error> {
        log::debug!("Stopping VENC channel [id = {}]", self.id);
        if self.state.load(Ordering::Relaxed) != state::Started::VALUE {
            return Ok(());
        }
        unsafe {
            rk_check_err!(ffi::RK_MPI_VENC_StopRecvFrame(self.id));
        }
        self.state.store(state::Initialized::VALUE, Ordering::Relaxed);
        Ok(())
    }

    fn bind(
        &self, module: ffi::rkMOD_ID_E, src_channel_id: i32, src_pipe_id: i32
    ) -> Result<ChannelBind, Error> {
        let src_channel = ffi::rkMPP_CHN_S {
            enModId: module,
            s32DevId: src_pipe_id,
            s32ChnId: src_channel_id,
        };
        let dst_channel = ffi::rkMPP_CHN_S {
            enModId: ffi::rkMOD_ID_E_RK_ID_VENC,
            s32DevId: 0,
            s32ChnId: self.id,
        };
        ChannelBind::new(src_channel, dst_channel)
    }

    pub fn send_frame(&self, frame: &MbFrameInner, timeout: Duration) -> Result<(), Error> {
        unsafe {
            rk_check_err!(
                ffi::RK_MPI_VENC_SendFrame(
                    self.id,
                    frame.frame() as *const _,
                    timeout.as_millis() as i32,
                )
            );
        }
        Ok(())
    }
    
    pub fn get_stream<'a>(
        &self,
        frame: &'a mut StreamFrame,
        timeout: Duration,
    ) -> Result<VencStreamInner<'a>, Error> {
        unsafe {
            rk_check_err!(
                ffi::RK_MPI_VENC_GetStream(
                    self.id, &mut frame.frame as *mut _, timeout.as_millis() as i32
                )
            );
        }
        Ok(VencStreamInner { frame, channel_id: self.id })
    }
}

pub struct VencChannel<'a, S> {
    inner: VencChannelInner,
    _mpi: &'a RockitMpi,
    _marker: PhantomData<S>,
}

impl<'a, S> VencChannel<'a, S> {
    pub fn id(&self) -> i32 {
        self.inner.id
    }
}

impl<'a> VencChannel<'a, state::Initialized> {
    pub fn new(
        mpi: &'a RockitMpi, channel_id: i32, cfg: &VencConfig
    ) -> Result<VencChannel<'a, state::Initialized>, Error> {
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
                    enPixelFormat: cfg.pixel_format.native_format(),
                    u32Profile: cfg.codec.native_profile(),
                    u32PicWidth: width,
                    u32PicHeight: height,
                    u32MaxPicWidth: width,
                    u32MaxPicHeight: height,
                    u32VirWidth: width,
                    u32VirHeight: height,
                    u32StreamBufCnt: cfg.buf_count as u32,
                    u32BufSize: cfg.calc_buffer_size(),
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
                state: AtomicU8::new(state::Initialized::VALUE),
                _resource: mpi.venc_channels.acqure()?,
            },
            _mpi: mpi,
            _marker: PhantomData,
        })
    }

    pub fn start(self) -> Result<VencChannel<'a, state::Started>, Error> {
        self.inner.start()?;
        Ok(VencChannel {
            inner: self.inner,
            _mpi: self._mpi,
            _marker: PhantomData,
        })
    }

    pub fn into_owned(self) -> VencChannelOwned<state::Initialized> {
        VencChannelOwned {
            inner: Rc::new(self.inner),
            _mpi: self._mpi.clone(),
            _marker: self._marker,
        }
    }
}

impl<'a> VencChannel<'a, state::Started> {
    pub fn bind_vi<'b>(&'a self, vi_channel: &'b ViChannel) -> Result<ViVencBind<'a, 'b>, Error> {
        self.inner.bind(ffi::rkMOD_ID_E_RK_ID_VI, vi_channel.id(), vi_channel.pipe_id())
            .map(|inner| ViVencBind {
                _inner: inner,
                venc_channel: self,
                _vi_channel: vi_channel,
            })
    }

    pub fn bind_vpss<'b>(
        &'a self, vpss_channel: &'b VpssChannel<'b, vpss::channel_state::Enabled>
    ) -> Result<VpssVencBind<'a, 'b>, Error> {
        self.inner.bind(ffi::rkMOD_ID_E_RK_ID_VI, vpss_channel.id(), 0)
            .map(|inner| VpssVencBind {
                _inner: inner,
                venc_channel: self,
                vpss_channel,
            })
    }

    pub fn send_frame(&self, frame: &mut MbFrame, timeout: Duration) -> Result<(), Error> {
        self.inner.send_frame(&frame.inner, timeout)
    }

    pub fn get_stream<'b>(
        &'a self,
        frame: &'b mut StreamFrame,
        timeout: Duration,
    ) -> Result<VencStream<'a, 'b>, Error> {
        self.inner.get_stream(frame, timeout)
            .map(|inner| VencStream { _channel: self, inner })
    }

    pub fn stop(self) -> Result<VencChannel<'a, state::Initialized>, Error> {
        self.inner.stop()?;
        Ok(VencChannel {
            inner: self.inner,
            _mpi: self._mpi,
            _marker: PhantomData,
        })
    }
}

pub struct VencChannelOwned<S> {
    inner: Rc<VencChannelInner>,
    _mpi: RockitMpi,
    _marker: PhantomData<S>,
}

impl<S> VencChannelOwned<S> {
    pub fn id(&self) -> i32 {
        self.inner.id
    }
}

impl VencChannelOwned<state::Initialized> {
    pub fn start(self) -> Result<VencChannelOwned<state::Started>, Error> {
        self.inner.start()?;
        Ok(VencChannelOwned {
            _mpi: self._mpi.clone(),
            inner: self.inner,
            _marker: PhantomData,
        })
    }
}

impl VencChannelOwned<state::Started> {
    pub fn bind_vi(&self, vi_channel: &ViChannelOwned) -> Result<ViVencBindOwned, Error> {
        self.inner.bind(ffi::rkMOD_ID_E_RK_ID_VI, vi_channel.id(), vi_channel.pipe_id())
            .map(|inner| ViVencBindOwned {
                _inner: Rc::new(inner),
                venc_channel: Rc::clone(&self.inner),
                _vi_channel: Rc::clone(&vi_channel.inner),
                _camera: Rc::clone(&vi_channel.camera),
                _mpi: self._mpi.clone(),
            })
    }

    pub fn bind_vpss(
        &self, vpss_channel: &VpssChannelOwned<vpss::channel_state::Enabled>
    ) -> Result<VpssVencBindOwned, Error> {
        self.inner.bind(ffi::rkMOD_ID_E_RK_ID_VPSS, vpss_channel.id(), 0)
            .map(|inner| VpssVencBindOwned {
                _inner: Rc::new(inner),
                venc_channel: Rc::clone(&self.inner),
                _vpss_channel: Rc::clone(&vpss_channel.inner),
                vpss_group: Rc::clone(&vpss_channel.group),
                _mpi: self._mpi.clone(),
            })
    }

    pub fn send_frame(&self, frame: &mut MbFrameOwned, timeout: Duration) -> Result<(), Error> {
        self.inner.send_frame(&frame.inner, timeout)
    }

    pub fn get_stream<'a>(
        &self,
        frame: &'a mut StreamFrame,
        timeout: Duration,
    ) -> Result<VencStreamOwned<'a>, Error> {
        self.inner.get_stream(frame, timeout)
            .map(|inner| VencStreamOwned {
                inner,
                _channel: Rc::clone(&self.inner),
                _mpi: self._mpi.clone(),
            })
    }
}

pub struct ViVencBindOwned {
    _inner: Rc<ChannelBind>,
    venc_channel: Rc<VencChannelInner>,
    _vi_channel: Rc<ViChannelInner>,
    _camera: Rc<CameraInner>,
    _mpi: RockitMpi,
}

impl ViVencBindOwned {
    pub fn get_stream<'a>(
        &self, frame: &'a mut StreamFrame, timeout: Duration
    ) -> Result<VencStreamOwned<'a>, Error> {
        self.venc_channel.get_stream(frame, timeout)
            .map(|inner| VencStreamOwned {
                _mpi: self._mpi.clone(),
                _channel: Rc::clone(&self.venc_channel),
                inner,                
            })
    }
}

pub struct VpssVencBindOwned {
    _inner: Rc<ChannelBind>,
    venc_channel: Rc<VencChannelInner>,
    _vpss_channel: Rc<VpssChannelInner>,
    vpss_group: Rc<VpssGroupInner>,
    _mpi: RockitMpi,
}

impl VpssVencBindOwned {
    pub fn send_frame(
        &self, pipe_id: u8, frame: &mut MbFrameOwned, timeout: Duration
    ) -> Result<(), Error> {
        self.vpss_group.send_frame(pipe_id as i32, &mut frame.inner, timeout)
    }

    pub fn get_stream<'a>(
        &self, frame: &'a mut StreamFrame, timeout: Duration
    ) -> Result<VencStreamOwned<'a>, Error> {
        self.venc_channel.get_stream(frame, timeout)
            .map(|inner| VencStreamOwned {
                _mpi: self._mpi.clone(),
                _channel: Rc::clone(&self.venc_channel),
                inner,
            })
    }
}

pub struct ViVencBind<'a, 'b> {
    _inner: ChannelBind,
    venc_channel: &'a VencChannel<'a, state::Started>,
    _vi_channel: &'b ViChannel<'b>,
}

impl<'a, 'b> ViVencBind<'a, 'b> {
    pub fn get_stream<'c>(
        &self,
        frame: &'c mut StreamFrame,
        timeout: Duration,
    ) -> Result<VencStream<'a, 'c>, Error> {
        self.venc_channel.get_stream(frame, timeout)
    }
}

pub struct VpssVencBind<'a, 'b> {
    _inner: ChannelBind,
    venc_channel: &'a VencChannel<'a, state::Started>,
    vpss_channel: &'b VpssChannel<'b, vpss::channel_state::Enabled>,
}

impl<'a, 'b> VpssVencBind<'a, 'b> {
    pub fn send_frame(
        &self, pipe_id: u8, frame: &mut MbFrameOwned, timeout: Duration
    ) -> Result<(), Error> {
        self.vpss_channel.group.send_frame(pipe_id as i32, &mut frame.inner, timeout)
    }

    pub fn get_stream<'c>(
        &self,
        frame: &'c mut StreamFrame,
        timeout: Duration,
    ) -> Result<VencStream<'a, 'c>, Error> {
        self.venc_channel.get_stream(frame, timeout)
    }
}

pub struct StreamFrame {
    frame: ffi::rkVENC_STREAM_S,
    _packet: Box<ffi::VENC_PACK_S>,
}

impl StreamFrame {
    pub fn new() -> Self {
        unsafe {
            let mut frame = std::mem::zeroed::<ffi::rkVENC_STREAM_S>();
            // Allocate it on the heap so it has a stable address
            // then we can store its pointer to a `pstPack` field.
            // Could we update a `pstPack` field just before getting encoder stream?
            let mut packet = Box::new(std::mem::zeroed::<ffi::rkVENC_PACK_S>());
            frame.pstPack = &mut *packet;
            StreamFrame { frame, _packet: packet }
        }
    }
}

struct VencStreamInner<'a> {
    frame: &'a mut StreamFrame,
    channel_id: i32,
}

impl<'a> Drop for VencStreamInner<'a> {
    fn drop(&mut self) {
        log::trace!(
            "Releasing VENC stream [channel = {}]", self.channel_id,
        );
        unsafe {
            rk_log_err!(
                ffi::RK_MPI_VENC_ReleaseStream(
                    self.channel_id, &mut self.frame.frame as *mut _
                ),
                "Error releasing encoder stream"
            );
        }
    }
}

impl<'a> VencStreamInner<'a> {
    pub fn data(&self) -> Result<&[u8], Error> {
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

pub struct VencStreamOwned<'a> {
    inner: VencStreamInner<'a>,
    _channel: Rc<VencChannelInner>,
    _mpi: RockitMpi,
}

impl<'a> VencStreamOwned<'a> {
    pub fn data(&self) -> Result<&[u8], Error> {
        self.inner.data()
    }
}

pub struct VencStream<'a, 'b> {
    inner: VencStreamInner<'b>,
    _channel: &'a VencChannel<'a, state::Started>,
}

impl<'a, 'b> VencStream<'a, 'b> {
    pub fn data(&self) -> Result<&[u8], Error> {
        self.inner.data()
    }
}
