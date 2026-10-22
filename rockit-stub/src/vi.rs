use std::time::{Duration, Instant};

use crate::ffi;
use crate::image::{self, Image};
use crate::mb::release_frame_block;
use crate::state::{self, Deadline, Pull, RK_SUCCESS, State, enter, lock, pull_blocking, rk_err};

const MOD: ffi::rkMOD_ID_E = ffi::rkMOD_ID_E_RK_ID_VI;

pub(crate) struct ViDev {
    attr: Option<ffi::VI_DEV_ATTR_S>,
    enabled: bool,
    pipes: Option<ffi::VI_DEV_BIND_PIPE_S>,
}

impl ViDev {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

pub(crate) struct ViChn {
    width: u32,
    height: u32,
    format: ffi::rkPIXEL_FORMAT_E,
    enabled: bool,
    seq: u32,
    next_due: Option<Instant>,
}

impl ViChn {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

fn check_dev(state: &State, dev: ffi::VI_DEV) -> Result<(), i32> {
    if !state.initialized {
        return Err(state::sys_not_ready(MOD));
    }
    if !(0..ffi::VI_MAX_DEV_NUM as i32).contains(&dev) {
        return Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_INVALID_DEVID));
    }
    Ok(())
}

fn check_chn(state: &State, pipe: ffi::VI_PIPE, chn: ffi::VI_CHN) -> Result<(), i32> {
    if !state.initialized {
        return Err(state::sys_not_ready(MOD));
    }
    if !(0..ffi::VI_MAX_PIPE_NUM as i32).contains(&pipe) {
        return Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_INVALID_PIPEID));
    }
    if !(0..ffi::VI_MAX_CHN_NUM as i32).contains(&chn) {
        return Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_INVALID_CHNID));
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VI_GetDevAttr(
    ViDev: ffi::VI_DEV, pstDevAttr: *mut ffi::VI_DEV_ATTR_S,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VI_GetDevAttr", format_args!("{ViDev}")) {
        return code;
    }
    let state = lock();
    try_rk!(check_dev(&state, ViDev));
    let Some(out) = (unsafe { pstDevAttr.as_mut() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    match state.vi_devs.get(&ViDev).and_then(|d| d.attr) {
        Some(attr) => {
            *out = attr;
            RK_SUCCESS
        }
        None => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VI_SetDevAttr(
    ViDev: ffi::VI_DEV, pstDevAttr: *const ffi::VI_DEV_ATTR_S,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VI_SetDevAttr", format_args!("{ViDev}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_dev(&state, ViDev));
    let Some(attr) = (unsafe { pstDevAttr.as_ref() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let dev = state
        .vi_devs
        .entry(ViDev)
        .or_insert(ViDev { attr: None, enabled: false, pipes: None });
    if dev.enabled {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_PERM);
    }
    dev.attr = Some(*attr);
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VI_GetDevIsEnable(ViDev: ffi::VI_DEV) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VI_GetDevIsEnable", format_args!("{ViDev}")) {
        return code;
    }
    let state = lock();
    try_rk!(check_dev(&state, ViDev));
    match state.vi_devs.get(&ViDev) {
        Some(dev) if dev.enabled => RK_SUCCESS,
        _ => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_DEV_UNEXIST),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VI_EnableDev(ViDev: ffi::VI_DEV) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VI_EnableDev", format_args!("{ViDev}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_dev(&state, ViDev));
    match state.vi_devs.get_mut(&ViDev) {
        Some(dev) if dev.attr.is_some() => {
            if dev.enabled {
                return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_DEV_EXIST);
            }
            dev.enabled = true;
            RK_SUCCESS
        }
        _ => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VI_DisableDev(ViDev: ffi::VI_DEV) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VI_DisableDev", format_args!("{ViDev}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_dev(&state, ViDev));
    match state.vi_devs.get_mut(&ViDev) {
        Some(dev) if dev.enabled => {
            dev.enabled = false;
            dev.pipes = None;
            RK_SUCCESS
        }
        _ => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_DEV_UNEXIST),
    }
}

/// Binds pipes to an enabled device. As in the real library, binding is static: it is
/// only possible once per enable and the pipes must not be bound to another device.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VI_SetDevBindPipe(
    ViDev: ffi::VI_DEV, pstDevBindPipe: *const ffi::VI_DEV_BIND_PIPE_S,
) -> ffi::RK_S32 {
    let Some(pipes) = (unsafe { pstDevBindPipe.as_ref() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let num = (pipes.u32Num as usize).min(pipes.PipeId.len());
    let pipe_ids = &pipes.PipeId[..num];
    if let Some(code) =
        enter("RK_MPI_VI_SetDevBindPipe", format_args!("{ViDev}, pipes={pipe_ids:?}"))
    {
        return code;
    }
    let mut state = lock();
    try_rk!(check_dev(&state, ViDev));
    if !(1..=ffi::VI_MAX_PHY_PIPE_NUM).contains(&pipes.u32Num)
        || pipe_ids.iter().any(|p| !(0..ffi::VI_MAX_PIPE_NUM as i32).contains(p))
    {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM);
    }
    if pipe_ids.iter().any(|&p| pipe_dev(&state, p).is_some_and(|d| d != ViDev)) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_EXIST);
    }
    match state.vi_devs.get_mut(&ViDev) {
        Some(dev) if dev.enabled && dev.pipes.is_none() => {
            dev.pipes = Some(*pipes);
            RK_SUCCESS
        }
        Some(dev) if dev.enabled => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_PERM),
        _ => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG),
    }
}

/// The enabled device a pipe is bound to.
fn pipe_dev(state: &State, pipe: ffi::VI_PIPE) -> Option<ffi::VI_DEV> {
    state.vi_devs.iter().find_map(|(&id, dev)| {
        let pipes = dev.pipes.as_ref().filter(|_| dev.enabled)?;
        pipes.PipeId[..pipes.u32Num as usize].contains(&pipe).then_some(id)
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VI_SetChnAttr(
    ViPipe: ffi::VI_PIPE, ViChn: ffi::VI_CHN, pstChnAttr: *const ffi::VI_CHN_ATTR_S,
) -> ffi::RK_S32 {
    let Some(attr) = (unsafe { pstChnAttr.as_ref() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let (width, height) = (attr.stSize.u32Width, attr.stSize.u32Height);
    if let Some(code) = enter(
        "RK_MPI_VI_SetChnAttr",
        format_args!("{ViPipe}, {ViChn}, {width}x{height}, fmt={}", attr.enPixelFormat),
    ) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, ViPipe, ViChn));
    if pipe_dev(&state, ViPipe).is_none() {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG);
    }
    if width == 0 || height == 0 {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM);
    }
    if !image::is_supported(attr.enPixelFormat) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_SUPPORT);
    }
    if state.vi_chns.get(&(ViPipe, ViChn)).is_some_and(|c| c.enabled) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_PERM);
    }
    state.vi_chns.insert(
        (ViPipe, ViChn),
        ViChn { width, height, format: attr.enPixelFormat, enabled: false, seq: 0, next_due: None },
    );
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VI_EnableChn(ViPipe: ffi::VI_PIPE, ViChn: ffi::VI_CHN) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VI_EnableChn", format_args!("{ViPipe}, {ViChn}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, ViPipe, ViChn));
    if pipe_dev(&state, ViPipe).is_none() {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG);
    }
    match state.vi_chns.get_mut(&(ViPipe, ViChn)) {
        Some(chn) if chn.enabled => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_EXIST),
        Some(chn) => {
            chn.enabled = true;
            chn.next_due = None;
            RK_SUCCESS
        }
        None => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VI_DisableChn(ViPipe: ffi::VI_PIPE, ViChn: ffi::VI_CHN) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VI_DisableChn", format_args!("{ViPipe}, {ViChn}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, ViPipe, ViChn));
    match state.vi_chns.get_mut(&(ViPipe, ViChn)) {
        Some(chn) if chn.enabled => {
            chn.enabled = false;
            RK_SUCCESS
        }
        _ => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_UNEXIST),
    }
}

/// Produces the next test pattern frame of a channel, paced to the configured fps.
pub(crate) fn pull(state: &mut State, pipe: ffi::VI_PIPE, chn: ffi::VI_CHN) -> Pull<Image> {
    if let Err(code) = check_chn(state, pipe, chn) {
        return Pull::Err(code);
    }
    let fps = state.vi_fps;
    let pts = state.now_us();
    let Some(channel) = state.vi_chns.get_mut(&(pipe, chn)).filter(|c| c.enabled) else {
        return Pull::Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_UNEXIST));
    };
    if fps > 0 {
        let now = Instant::now();
        let period = Duration::from_secs(1) / fps;
        let due = channel.next_due.unwrap_or(now);
        if now < due {
            return Pull::NotYet(Some(due));
        }
        channel.next_due = Some((due + period).max(now));
    }
    channel.seq += 1;
    Pull::Ready(Image::test_pattern(channel.format, channel.width, channel.height, channel.seq, pts))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VI_GetChnFrame(
    ViPipe: ffi::VI_PIPE,
    ViChn: ffi::VI_CHN,
    pstFrameInfo: *mut ffi::VIDEO_FRAME_INFO_S,
    s32MilliSec: ffi::RK_S32,
) -> ffi::RK_S32 {
    if let Some(code) =
        enter("RK_MPI_VI_GetChnFrame", format_args!("{ViPipe}, {ViChn}, {s32MilliSec}"))
    {
        return code;
    }
    let Some(out) = (unsafe { pstFrameInfo.as_mut() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let deadline = Deadline::from_ms(s32MilliSec);
    match pull_blocking(MOD, deadline, |state| pull(state, ViPipe, ViChn)) {
        Ok((mut state, image)) => {
            let time_ref = state.vi_chns[&(ViPipe, ViChn)].seq * 2;
            *out = state.export_frame(image, time_ref);
            RK_SUCCESS
        }
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VI_ReleaseChnFrame(
    ViPipe: ffi::VI_PIPE, ViChn: ffi::VI_CHN, pstFrameInfo: *const ffi::VIDEO_FRAME_INFO_S,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VI_ReleaseChnFrame", format_args!("{ViPipe}, {ViChn}")) {
        return code;
    }
    release_frame_block(MOD, pstFrameInfo)
}
