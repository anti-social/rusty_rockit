use std::collections::VecDeque;

use crate::clip::{self, Clip};
use crate::ffi;
use crate::image::{self, Image};
use crate::state::{
    self, COND, ChnKey, Deadline, Pull, RK_SUCCESS, State, enter, lock, pull_blocking, pull_bound,
    rk_err,
};

const MOD: ffi::rkMOD_ID_E = ffi::rkMOD_ID_E_RK_ID_VENC;
const INPUT_QUEUE_DEPTH: usize = 4;
const DEFAULT_GOP: u32 = 30;

/// What `GetStream` returns, chosen when a channel is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VencSource {
    /// Fake, non-decodable access units derived from the input frames,
    /// with an IDR every configured GOP.
    Synthetic,
    /// Frames of a pre-encoded clip, looped. Falls back to `Synthetic` without clips.
    Clip,
}

pub(crate) struct VencChn {
    codec: ffi::rkCODEC_ID_E,
    width: u32,
    height: u32,
    gop: u32,
    started: bool,
    queue: VecDeque<Image>,
    seq: u32,
    outstanding: u32,
    clip: Option<&'static Clip>,
}

fn gop(attr: &ffi::VENC_RC_ATTR_S) -> u32 {
    let rc = &attr.__bindgen_anon_1;
    let gop = unsafe {
        match attr.enRcMode {
            ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H264CBR => rc.stH264Cbr.u32Gop,
            ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H264VBR => rc.stH264Vbr.u32Gop,
            ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H264AVBR => rc.stH264Avbr.u32Gop,
            ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H265CBR => rc.stH265Cbr.u32Gop,
            ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H265VBR => rc.stH265Vbr.u32Gop,
            ffi::rkVENC_RC_MODE_E_VENC_RC_MODE_H265AVBR => rc.stH265Avbr.u32Gop,
            _ => 0,
        }
    };
    if gop == 0 { DEFAULT_GOP } else { gop }
}

fn check_chn(state: &State, chn: ffi::VENC_CHN) -> Result<(), i32> {
    if !state.initialized {
        return Err(state::sys_not_ready(MOD));
    }
    if !(0..ffi::VENC_MAX_CHN_NUM as i32).contains(&chn) {
        return Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_INVALID_CHNID));
    }
    if !state.venc_chns.contains_key(&chn) {
        return Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_UNEXIST));
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VENC_CreateChn(
    VeChn: ffi::VENC_CHN, pstAttr: *const ffi::VENC_CHN_ATTR_S,
) -> ffi::RK_S32 {
    let Some(attr) = (unsafe { pstAttr.as_ref() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let venc = &attr.stVencAttr;
    if let Some(code) = enter(
        "RK_MPI_VENC_CreateChn",
        format_args!(
            "{VeChn}, codec={}, {}x{}, fmt={}",
            venc.enType, venc.u32PicWidth, venc.u32PicHeight, venc.enPixelFormat
        ),
    ) {
        return code;
    }
    let mut state = lock();
    if !state.initialized {
        return state::sys_not_ready(MOD);
    }
    if !(0..ffi::VENC_MAX_CHN_NUM as i32).contains(&VeChn) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_INVALID_CHNID);
    }
    if state.venc_chns.contains_key(&VeChn) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_EXIST);
    }
    if !matches!(venc.enType, ffi::rkCODEC_ID_E_RK_VIDEO_ID_AVC | ffi::rkCODEC_ID_E_RK_VIDEO_ID_HEVC)
        || !image::is_supported(venc.enPixelFormat)
    {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_SUPPORT);
    }
    if venc.u32PicWidth == 0 || venc.u32PicHeight == 0 {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM);
    }
    let clip = match state.venc_source {
        VencSource::Synthetic => None,
        VencSource::Clip => {
            let codec = match venc.enType {
                ffi::rkCODEC_ID_E_RK_VIDEO_ID_AVC => clip::Codec::H264,
                _ => clip::Codec::Hevc,
            };
            clip::select(codec, venc.u32PicWidth, venc.u32PicHeight)
        }
    };
    state.venc_chns.insert(
        VeChn,
        VencChn {
            codec: venc.enType,
            width: venc.u32PicWidth,
            height: venc.u32PicHeight,
            gop: gop(&attr.stRcAttr),
            started: false,
            queue: VecDeque::new(),
            seq: 0,
            outstanding: 0,
            clip,
        },
    );
    RK_SUCCESS
}

/// Fails with `BUSY` while the channel still receives frames or is bound to a source.
#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VENC_DestroyChn(VeChn: ffi::VENC_CHN) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VENC_DestroyChn", format_args!("{VeChn}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, VeChn));
    let bound = state.binds.contains_key(&ChnKey { module: MOD, dev: 0, chn: VeChn });
    if state.venc_chns[&VeChn].started || bound {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_BUSY);
    }
    state.venc_chns.remove(&VeChn);
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VENC_StartRecvFrame(
    VeChn: ffi::VENC_CHN, pstRecvParam: *const ffi::VENC_RECV_PIC_PARAM_S,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VENC_StartRecvFrame", format_args!("{VeChn}")) {
        return code;
    }
    if pstRecvParam.is_null() {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    }
    let mut state = lock();
    try_rk!(check_chn(&state, VeChn));
    state.venc_chns.get_mut(&VeChn).unwrap().started = true;
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VENC_StopRecvFrame(VeChn: ffi::VENC_CHN) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VENC_StopRecvFrame", format_args!("{VeChn}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, VeChn));
    let chn = state.venc_chns.get_mut(&VeChn).unwrap();
    chn.started = false;
    chn.queue.clear();
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VENC_SendFrame(
    VeChn: ffi::VENC_CHN, pstFrame: *const ffi::VIDEO_FRAME_INFO_S, s32MilliSec: ffi::RK_S32,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VENC_SendFrame", format_args!("{VeChn}, {s32MilliSec}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, VeChn));
    if !state.venc_chns[&VeChn].started {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_PERM);
    }
    let image = match state.import_frame(MOD, pstFrame) {
        Ok(image) => image,
        Err(code) => return code,
    };
    let chn = state.venc_chns.get_mut(&VeChn).unwrap();
    if chn.queue.len() >= INPUT_QUEUE_DEPTH {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_BUF_FULL);
    }
    chn.queue.push_back(image);
    COND.notify_all();
    RK_SUCCESS
}

fn pull(state: &mut State, chn: ffi::VENC_CHN) -> Pull<Image> {
    if let Err(code) = check_chn(state, chn) {
        return Pull::Err(code);
    }
    let channel = state.venc_chns.get_mut(&chn).unwrap();
    if !channel.started {
        return Pull::Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_PERM));
    }
    if let Some(image) = channel.queue.pop_front() {
        return Pull::Ready(image);
    }
    pull_bound(state, ChnKey { module: MOD, dev: 0, chn }).unwrap_or(Pull::NotYet(None))
}

/// Bytes that never form a start code or need emulation prevention.
fn payload_byte(b: u8) -> u8 {
    0x80 | (b & 0x7f)
}

/// Builds a fake Annex-B access unit: parameter sets + IDR slice, or a single P slice.
fn encode(chn: &VencChn, image: &Image, seq: u32) -> (Vec<u8>, bool) {
    let idr = seq % chn.gop == 0;
    let hevc = chn.codec == ffi::rkCODEC_ID_E_RK_VIDEO_ID_HEVC;
    let nal = |header: &[u8], payload: &[u8]| {
        let mut out = vec![0, 0, 0, 1];
        out.extend_from_slice(header);
        out.extend(payload.iter().copied().map(payload_byte));
        out
    };
    let dims: Vec<u8> = [chn.width, chn.height]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .flat_map(|b| [b >> 4, b & 0xf])
        .collect();
    let samples = (image.width as usize * image.height as usize / 256).clamp(16, 64 * 1024);
    let mut slice: Vec<u8> = seq.to_be_bytes().iter().flat_map(|b| [b >> 4, b & 0xf]).collect();
    slice.extend((0..samples).map(|i| image.luma_sample(i, samples) >> 1));

    let mut out = Vec::new();
    match (hevc, idr) {
        (false, true) => {
            out.extend(nal(&[0x67], &dims));
            out.extend(nal(&[0x68], &[0x4e]));
            out.extend(nal(&[0x65], &slice));
        }
        (false, false) => out.extend(nal(&[0x41], &slice)),
        (true, true) => {
            out.extend(nal(&[0x40, 0x01], &[0x0c]));
            out.extend(nal(&[0x42, 0x01], &dims));
            out.extend(nal(&[0x44, 0x01], &[0x41]));
            out.extend(nal(&[0x26, 0x01], &slice));
        }
        (true, false) => out.extend(nal(&[0x02, 0x01], &slice)),
    }
    (out, idr)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VENC_GetStream(
    VeChn: ffi::VENC_CHN, pstStream: *mut ffi::VENC_STREAM_S, s32MilliSec: ffi::RK_S32,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VENC_GetStream", format_args!("{VeChn}, {s32MilliSec}")) {
        return code;
    }
    let Some(stream) = (unsafe { pstStream.as_mut() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let Some(pack) = (unsafe { stream.pstPack.as_mut() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let deadline = Deadline::from_ms(s32MilliSec);
    let (mut state, image) = match pull_blocking(MOD, deadline, |state| pull(state, VeChn)) {
        Ok(result) => result,
        Err(code) => return code,
    };

    let chn = state.venc_chns.get_mut(&VeChn).unwrap();
    let seq = chn.seq;
    chn.seq += 1;
    chn.outstanding += 1;
    let (data, idr) = match chn.clip {
        Some(clip) => {
            let frame = &clip.frames[seq as usize % clip.frames.len()];
            (frame.data.clone(), frame.idr)
        }
        None => encode(chn, &image, seq),
    };
    let hevc = chn.codec == ffi::rkCODEC_ID_E_RK_VIDEO_ID_HEVC;

    let mb = state.alloc_block(None, data.len());
    state.block_mut(mb).unwrap().data.copy_from_slice(&data);

    *pack = unsafe { std::mem::zeroed() };
    pack.pMbBlk = mb;
    pack.u32Len = data.len() as u32;
    pack.u64PTS = image.pts;
    pack.bFrameEnd = 1;
    pack.DataType = match (hevc, idr) {
        (false, true) => ffi::VENC_DATA_TYPE_U {
            enH264EType: ffi::rkH264E_NALU_TYPE_E_H264E_NALU_IDRSLICE,
        },
        (false, false) => ffi::VENC_DATA_TYPE_U {
            enH264EType: ffi::rkH264E_NALU_TYPE_E_H264E_NALU_PSLICE,
        },
        (true, true) => ffi::VENC_DATA_TYPE_U {
            enH265EType: ffi::rkH265E_NALU_TYPE_E_H265E_NALU_IDRSLICE,
        },
        (true, false) => ffi::VENC_DATA_TYPE_U {
            enH265EType: ffi::rkH265E_NALU_TYPE_E_H265E_NALU_PSLICE,
        },
    };
    stream.u32PackCount = 1;
    stream.u32Seq = seq;
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VENC_ReleaseStream(
    VeChn: ffi::VENC_CHN, pstStream: *mut ffi::VENC_STREAM_S,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VENC_ReleaseStream", format_args!("{VeChn}")) {
        return code;
    }
    let Some(stream) = (unsafe { pstStream.as_mut() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let mut state = lock();
    try_rk!(check_chn(&state, VeChn));
    if stream.pstPack.is_null() || stream.u32PackCount == 0 {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM);
    }
    let packs =
        unsafe { std::slice::from_raw_parts_mut(stream.pstPack, stream.u32PackCount as usize) };
    for pack in packs {
        if !state.release_block(pack.pMbBlk) {
            return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM);
        }
        pack.pMbBlk = std::ptr::null_mut();
    }
    let chn = state.venc_chns.get_mut(&VeChn).unwrap();
    chn.outstanding = chn.outstanding.saturating_sub(1);
    stream.u32PackCount = 0;
    COND.notify_all();
    RK_SUCCESS
}
