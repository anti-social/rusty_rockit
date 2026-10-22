use std::ffi::c_void;
use std::ptr;

use crate::ffi;
use crate::state::{self, COND, Pool, RK_SUCCESS, enter, lock, rk_err};

const MOD: ffi::rkMOD_ID_E = ffi::rkMOD_ID_E_RK_ID_MB;
const INVALID_POOL: ffi::MB_POOL = ffi::MB_INVALID_POOLID as ffi::MB_POOL;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn RK_MPI_MB_CreatePool(
    pstMbPoolCfg: *mut ffi::MB_POOL_CONFIG_S,
) -> ffi::MB_POOL {
    let Some(cfg) = (unsafe { pstMbPoolCfg.as_ref() }) else {
        return INVALID_POOL;
    };
    if enter(
        "RK_MPI_MB_CreatePool",
        format_args!("size={}, count={}", cfg.u64MBSize, cfg.u32MBCnt),
    )
    .is_some()
    {
        return INVALID_POOL;
    }
    if cfg.u64MBSize == 0 || cfg.u32MBCnt == 0 {
        return INVALID_POOL;
    }
    let mut state = lock();
    if !state.initialized {
        return INVALID_POOL;
    }
    let id = state.next_pool_id;
    state.next_pool_id += 1;
    state.pools.insert(id, Pool { blk_size: cfg.u64MBSize, count: cfg.u32MBCnt, used: 0 });
    id
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_MB_DestroyPool(pool: ffi::MB_POOL) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_MB_DestroyPool", format_args!("{pool}")) {
        return code;
    }
    let mut state = lock();
    match state.pools.get(&pool) {
        None => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_UNEXIST),
        Some(p) if p.used > 0 => rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_BUSY),
        Some(_) => {
            state.pools.remove(&pool);
            RK_SUCCESS
        }
    }
}

/// Exhausted pools return null immediately, even if `block` is requested,
/// so that a leaked buffer fails a test instead of hanging it.
#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_MB_GetMB(
    pool: ffi::MB_POOL, u64Size: ffi::RK_U64, block: ffi::RK_BOOL,
) -> ffi::MB_BLK {
    if enter("RK_MPI_MB_GetMB", format_args!("{pool}, size={u64Size}, block={block}")).is_some() {
        return ptr::null_mut();
    }
    let mut state = lock();
    let Some(p) = state.pools.get_mut(&pool) else {
        return ptr::null_mut();
    };
    if u64Size > p.blk_size || p.used >= p.count {
        return ptr::null_mut();
    }
    p.used += 1;
    let size = p.blk_size as usize;
    state.alloc_block(Some(pool), size)
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_MB_ReleaseMB(mb: ffi::MB_BLK) -> ffi::RK_S32 {
    if let Some(code) = enter("RK_MPI_MB_ReleaseMB", format_args!("{mb:p}")) {
        return code;
    }
    if !lock().release_block(mb) {
        return rk_err(MOD, ffi::rkEN_ERR_CODE_E_RK_ERR_UNEXIST);
    }
    COND.notify_all();
    RK_SUCCESS
}

#[unsafe(no_mangle)]
pub extern "C" fn RK_MPI_MB_Handle2VirAddr(mb: ffi::MB_BLK) -> *mut c_void {
    let mut state = lock();
    match state.block_mut(mb) {
        Some(block) => block.data.as_mut_ptr() as *mut c_void,
        None => ptr::null_mut(),
    }
}

pub(crate) fn release_frame_block(
    module: ffi::rkMOD_ID_E, frame: *const ffi::VIDEO_FRAME_INFO_S,
) -> ffi::RK_S32 {
    let Some(frame) = (unsafe { frame.as_ref() }) else {
        return rk_err(module, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR);
    };
    if !lock().release_block(frame.stVFrame.pMbBlk) {
        return rk_err(module, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM);
    }
    COND.notify_all();
    state::RK_SUCCESS
}
