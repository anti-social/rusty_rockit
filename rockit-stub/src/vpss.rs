use std::collections::{HashMap, VecDeque};

use crate::ffi;
use crate::image::{self, Image};
use crate::mb::release_frame_block;
use crate::state::{
    self, COND, ChnKey, Deadline, Pull, RK_SUCCESS, State, enter, lock, pull_blocking, pull_bound,
    rk_err,
};

const MOD: ffi::rkMOD_ID_E = ffi::rkMOD_ID_E_RK_ID_VPSS;

pub(crate) struct VpssGrp {
    started: bool,
    chns: HashMap<ffi::VPSS_CHN, VpssChn>,
}

struct VpssChn {
    width: u32,
    height: u32,
    format: ffi::rkPIXEL_FORMAT_E,
    mirror: bool,
    flip: bool,
    depth: usize,
    enabled: bool,
    seq: u32,
    queue: VecDeque<Image>,
}

impl VpssGrp {
    /// Converts an input frame for every enabled channel, dropping the oldest frames
    /// of full queues.
    fn process(&mut self, image: &Image) {
        for chn in self.chns.values_mut().filter(|c| c.enabled) {
            let out = image
                .clone()
                .convert(image::NV12)
                .scale(chn.width, chn.height, chn.mirror, chn.flip)
                .convert(chn.format);
            while chn.queue.len() >= chn.depth.max(1) {
                chn.queue.pop_front();
            }
            chn.queue.push_back(out);
        }
    }
}

fn check_grp(state: &State, grp: ffi::VPSS_GRP) -> Result<(), i32> {
    if !state.initialized {
        return Err(state::sys_not_ready(MOD));
    }
    if !(0..ffi::VPSS_MAX_GRP_NUM as i32).contains(&grp) {
        return Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_INVALID_DEVID));
    }
    if !state.vpss_grps.contains_key(&grp) {
        return Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_GROUP_UNEXIST));
    }
    Ok(())
}

fn check_chn(state: &State, grp: ffi::VPSS_GRP, chn: ffi::VPSS_CHN) -> Result<(), i32> {
    check_grp(state, grp)?;
    if !(0..ffi::VPSS_MAX_CHN_NUM as i32).contains(&chn) {
        return Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_INVALID_CHNID));
    }
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VPSS_CreateGrp(
    VpssGrp: ffi::VPSS_GRP, pstGrpAttr: *const ffi::VPSS_GRP_ATTR_S,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VPSS_CreateGrp", format_args!("{VpssGrp}")) {
        return code;
    }
    let mut state = lock();
    if !state.initialized {
        return state::sys_not_ready(MOD);
    }
    if !(0..ffi::VPSS_MAX_GRP_NUM as i32).contains(&VpssGrp) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_INVALID_DEVID);
    }
    let Some(attr) = (unsafe { pstGrpAttr.as_ref() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    if !image::is_supported(attr.enPixelFormat) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_SUPPORT);
    }
    if state.vpss_grps.contains_key(&VpssGrp) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_GROUP_EXIST);
    }
    state.vpss_grps.insert(VpssGrp, VpssGrp { started: false, chns: HashMap::new() });
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VPSS_DestroyGrp(VpssGrp: ffi::VPSS_GRP) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VPSS_DestroyGrp", format_args!("{VpssGrp}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_grp(&state, VpssGrp));
    if state.vpss_grps[&VpssGrp].started {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_BUSY);
    }
    state.vpss_grps.remove(&VpssGrp);
    RK_SUCCESS
}

fn set_started(name: &str, grp: ffi::VPSS_GRP, started: bool) -> ffi::RK_S32 {
    if let Some(code) = enter(name, format_args!("{grp}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_grp(&state, grp));
    let group = state.vpss_grps.get_mut(&grp).unwrap();
    if group.started == started {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_PERM);
    }
    group.started = started;
    if !started {
        group.chns.values_mut().for_each(|c| c.queue.clear());
    }
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VPSS_StartGrp(VpssGrp: ffi::VPSS_GRP) -> ffi::RK_S32 {
    set_started("RK_MPI_VPSS_StartGrp", VpssGrp, true)
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VPSS_StopGrp(VpssGrp: ffi::VPSS_GRP) -> ffi::RK_S32 {
    set_started("RK_MPI_VPSS_StopGrp", VpssGrp, false)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VPSS_SetChnAttr(
    VpssGrp: ffi::VPSS_GRP, VpssChn: ffi::VPSS_CHN, pstChnAttr: *const ffi::VPSS_CHN_ATTR_S,
) -> ffi::RK_S32 {
    let Some(attr) = (unsafe { pstChnAttr.as_ref() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    if let Some(code) = enter(
        "RK_MPI_VPSS_SetChnAttr",
        format_args!(
            "{VpssGrp}, {VpssChn}, {}x{}, fmt={}",
            attr.u32Width, attr.u32Height, attr.enPixelFormat
        ),
    ) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, VpssGrp, VpssChn));
    if attr.u32Width == 0 || attr.u32Height == 0 {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM);
    }
    if !image::is_supported(attr.enPixelFormat) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_SUPPORT);
    }
    let group = state.vpss_grps.get_mut(&VpssGrp).unwrap();
    let enabled = group.chns.get(&VpssChn).is_some_and(|c| c.enabled);
    group.chns.insert(
        VpssChn,
        VpssChn {
            width: attr.u32Width,
            height: attr.u32Height,
            format: attr.enPixelFormat,
            mirror: attr.bMirror != 0,
            flip: attr.bFlip != 0,
            depth: attr.u32Depth as usize,
            enabled,
            seq: 0,
            queue: VecDeque::new(),
        },
    );
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VPSS_EnableChn(
    VpssGrp: ffi::VPSS_GRP, VpssChn: ffi::VPSS_CHN,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VPSS_EnableChn", format_args!("{VpssGrp}, {VpssChn}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, VpssGrp, VpssChn));
    match state.vpss_grps.get_mut(&VpssGrp).unwrap().chns.get_mut(&VpssChn) {
        Some(chn) => {
            chn.enabled = true;
            RK_SUCCESS
        }
        None => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_CONFIG),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_VPSS_DisableChn(
    VpssGrp: ffi::VPSS_GRP, VpssChn: ffi::VPSS_CHN,
) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_VPSS_DisableChn", format_args!("{VpssGrp}, {VpssChn}")) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_chn(&state, VpssGrp, VpssChn));
    match state.vpss_grps.get_mut(&VpssGrp).unwrap().chns.get_mut(&VpssChn) {
        Some(chn) if chn.enabled => {
            chn.enabled = false;
            chn.queue.clear();
            RK_SUCCESS
        }
        _ => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_UNEXIST),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VPSS_SendFrame(
    VpssGrp: ffi::VPSS_GRP,
    VpssGrpPipe: ffi::VPSS_GRP_PIPE,
    pstVideoFrame: *const ffi::VIDEO_FRAME_INFO_S,
    s32MilliSec: ffi::RK_S32,
) -> ffi::RK_S32 {
    if let Some(code) = enter(
        "RK_MPI_VPSS_SendFrame",
        format_args!("{VpssGrp}, {VpssGrpPipe}, {s32MilliSec}"),
    ) {
        return code;
    }
    let mut state = lock();
    try_rk!(check_grp(&state, VpssGrp));
    if !state.vpss_grps[&VpssGrp].started {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_PERM);
    }
    let image = match state.import_frame(MOD, pstVideoFrame) {
        Ok(image) => image,
        Err(code) => return code,
    };
    state.vpss_grps.get_mut(&VpssGrp).unwrap().process(&image);
    COND.notify_all();
    RK_SUCCESS
}

/// Takes the next output frame of a channel, pulling input from a bound source if needed.
pub(crate) fn pull(state: &mut State, grp: ffi::VPSS_GRP, chn: ffi::VPSS_CHN) -> Pull<Image> {
    if let Err(code) = check_chn(state, grp, chn) {
        return Pull::Err(code);
    }
    let group = &state.vpss_grps[&grp];
    match group.chns.get(&chn) {
        Some(c) if c.enabled && group.started => {}
        _ => return Pull::Err(rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_PERM)),
    }
    if state.vpss_grps[&grp].chns[&chn].queue.is_empty() {
        let src = ChnKey { module: MOD, dev: grp, chn: 0 };
        match pull_bound(state, src) {
            Some(Pull::Ready(image)) => state.vpss_grps.get_mut(&grp).unwrap().process(&image),
            Some(Pull::Err(code)) => return Pull::Err(code),
            Some(Pull::NotYet(wake_at)) => return Pull::NotYet(wake_at),
            None => return Pull::NotYet(None),
        }
    }
    let channel = state.vpss_grps.get_mut(&grp).unwrap().chns.get_mut(&chn).unwrap();
    channel.seq += 1;
    match channel.queue.pop_front() {
        Some(image) => Pull::Ready(image),
        None => Pull::NotYet(None),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VPSS_GetChnFrame(
    VpssGrp: ffi::VPSS_GRP,
    VpssChn: ffi::VPSS_CHN,
    pstVideoFrame: *mut ffi::VIDEO_FRAME_INFO_S,
    s32MilliSec: ffi::RK_S32,
) -> ffi::RK_S32 {
    if let Some(code) = enter(
        "RK_MPI_VPSS_GetChnFrame",
        format_args!("{VpssGrp}, {VpssChn}, {s32MilliSec}"),
    ) {
        return code;
    }
    let Some(out) = (unsafe { pstVideoFrame.as_mut() }) else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    let deadline = Deadline::from_ms(s32MilliSec);
    match pull_blocking(MOD, deadline, |state| pull(state, VpssGrp, VpssChn)) {
        Ok((mut state, image)) => {
            let time_ref = state.vpss_grps[&VpssGrp].chns[&VpssChn].seq * 2;
            *out = state.export_frame(image, time_ref);
            RK_SUCCESS
        }
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_VPSS_ReleaseChnFrame(
    VpssGrp: ffi::VPSS_GRP, VpssChn: ffi::VPSS_CHN, pstVideoFrame: *const ffi::VIDEO_FRAME_INFO_S,
) -> ffi::RK_S32 {
    if let Some(code) =
        enter("RK_MPI_VPSS_ReleaseChnFrame", format_args!("{VpssGrp}, {VpssChn}"))
    {
        return code;
    }
    release_frame_block(MOD, pstVideoFrame)
}
