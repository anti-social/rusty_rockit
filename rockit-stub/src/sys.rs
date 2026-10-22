use crate::ffi;
use crate::state::{self, COND, ChnKey, RK_SUCCESS, enter, lock, rk_err};

const MOD: ffi::rkMOD_ID_E = ffi::rkMOD_ID_E_RK_ID_SYS;

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_SYS_Init() -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_SYS_Init", format_args!("")) {
        return code;
    }
    lock().initialized = true;
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_SYS_Exit() -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_SYS_Exit", format_args!("")) {
        return code;
    }
    lock().exit();
    COND.notify_all();
    RK_SUCCESS
}

fn fmt_chn(chn: &ffi::MPP_CHN_S) -> String {
    format!("{}:{}:{}", chn.enModId, chn.s32DevId, chn.s32ChnId)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_SYS_Bind(
    pstSrcChn: *const ffi::MPP_CHN_S, pstDestChn: *const ffi::MPP_CHN_S,
) -> ffi::RK_S32 {
    let (Some(src), Some(dst)) = (unsafe { pstSrcChn.as_ref() }, unsafe { pstDestChn.as_ref() })
    else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    if let Some(code) =
        enter("RK_MPI_SYS_Bind", format_args!("{} -> {}", fmt_chn(src), fmt_chn(dst)))
    {
        return code;
    }
    let mut state = lock();
    if !state.initialized {
        return state::sys_not_ready(MOD);
    }
    let key = ChnKey::dst(dst);
    if state.binds.contains_key(&key) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_EXIST);
    }
    state.binds.insert(key, *src);
    COND.notify_all();
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_SYS_UnBind(
    pstSrcChn: *const ffi::MPP_CHN_S, pstDestChn: *const ffi::MPP_CHN_S,
) -> ffi::RK_S32 {
    let (Some(src), Some(dst)) = (unsafe { pstSrcChn.as_ref() }, unsafe { pstDestChn.as_ref() })
    else {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    if let Some(code) =
        enter("RK_MPI_SYS_UnBind", format_args!("{} -> {}", fmt_chn(src), fmt_chn(dst)))
    {
        return code;
    }
    let mut state = lock();
    let key = ChnKey::dst(dst);
    match state.binds.get(&key) {
        Some(bound) if same_chn(bound, src) => {
            state.binds.remove(&key);
            RK_SUCCESS
        }
        _ => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_UNEXIST),
    }
}

fn same_chn(a: &ffi::MPP_CHN_S, b: &ffi::MPP_CHN_S) -> bool {
    a.enModId == b.enModId && a.s32DevId == b.s32DevId && a.s32ChnId == b.s32ChnId
}
