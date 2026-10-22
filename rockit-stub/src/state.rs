use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::{Condvar, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::ffi;
use crate::image::Image;
use crate::{aiq, venc, vi, vpss};

pub(crate) const RK_SUCCESS: i32 = ffi::RK_SUCCESS as i32;
const RK_ERR_APPID: u32 = 0x8000_0000 + 0x2000_0000;

/// Mirrors the `RK_DEF_ERR` macro with `RK_ERR_LEVEL_ERROR`.
pub(crate) const fn rk_err(module: ffi::rkMOD_ID_E, code: ffi::rkEN_ERR_CODE_E) -> i32 {
    (RK_ERR_APPID | (module << 16) | (ffi::rkERR_LEVEL_E_RK_ERR_LEVEL_ERROR << 13) | code) as i32
}

pub(crate) const DEFAULT_VI_FPS: u32 = 30;

pub(crate) struct Block {
    pub pool: Option<ffi::MB_POOL>,
    pub data: Vec<u8>,
}

pub(crate) struct Pool {
    pub blk_size: u64,
    pub count: u32,
    pub used: u32,
}

/// Normalized bind endpoint: VPSS receives input per group, VENC per channel.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct ChnKey {
    pub module: ffi::rkMOD_ID_E,
    pub dev: i32,
    pub chn: i32,
}

impl ChnKey {
    pub fn dst(chn: &ffi::MPP_CHN_S) -> Self {
        match chn.enModId {
            ffi::rkMOD_ID_E_RK_ID_VPSS => Self { module: chn.enModId, dev: chn.s32DevId, chn: 0 },
            ffi::rkMOD_ID_E_RK_ID_VENC => Self { module: chn.enModId, dev: 0, chn: chn.s32ChnId },
            _ => Self { module: chn.enModId, dev: chn.s32DevId, chn: chn.s32ChnId },
        }
    }
}

pub(crate) struct State {
    pub initialized: bool,
    pub epoch: Instant,
    pub vi_fps: u32,
    pub blocks: HashMap<usize, Box<Block>>,
    pub pools: HashMap<ffi::MB_POOL, Pool>,
    pub next_pool_id: ffi::MB_POOL,
    pub binds: HashMap<ChnKey, ffi::MPP_CHN_S>,
    pub vi_devs: HashMap<i32, vi::ViDev>,
    pub vi_chns: HashMap<(i32, i32), vi::ViChn>,
    pub vpss_grps: HashMap<i32, vpss::VpssGrp>,
    pub venc_chns: HashMap<i32, venc::VencChn>,
    pub aiq_ctxs: HashMap<usize, aiq::AiqCtx>,
}

impl State {
    fn new() -> Self {
        Self {
            initialized: false,
            epoch: Instant::now(),
            vi_fps: crate::control::default_vi_fps(),
            blocks: HashMap::new(),
            pools: HashMap::new(),
            next_pool_id: 0,
            binds: HashMap::new(),
            vi_devs: HashMap::new(),
            vi_chns: HashMap::new(),
            vpss_grps: HashMap::new(),
            venc_chns: HashMap::new(),
            aiq_ctxs: HashMap::new(),
        }
    }

    /// Drops every MPI object except memory blocks that are still referenced by the user.
    pub fn exit(&mut self) {
        let blocks = std::mem::take(&mut self.blocks);
        let vi_fps = self.vi_fps;
        let aiq_ctxs = std::mem::take(&mut self.aiq_ctxs);
        *self = Self::new();
        self.blocks = blocks;
        self.vi_fps = vi_fps;
        self.aiq_ctxs = aiq_ctxs;
        for block in self.blocks.values_mut() {
            block.pool = None;
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn now_us(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64
    }

    pub fn alloc_block(&mut self, pool: Option<ffi::MB_POOL>, size: usize) -> ffi::MB_BLK {
        let block = Box::new(Block { pool, data: vec![0; size] });
        let handle = &*block as *const Block as usize;
        self.blocks.insert(handle, block);
        handle as ffi::MB_BLK
    }

    pub fn block(&self, mb: ffi::MB_BLK) -> Option<&Block> {
        self.blocks.get(&(mb as usize)).map(|b| &**b)
    }

    pub fn block_mut(&mut self, mb: ffi::MB_BLK) -> Option<&mut Block> {
        self.blocks.get_mut(&(mb as usize)).map(|b| &mut **b)
    }

    pub fn release_block(&mut self, mb: ffi::MB_BLK) -> bool {
        let Some(block) = self.blocks.remove(&(mb as usize)) else {
            return false;
        };
        if let Some(pool) = block.pool.and_then(|id| self.pools.get_mut(&id)) {
            pool.used -= 1;
        }
        true
    }

    /// Copies an image into a new block and describes it as a video frame.
    pub fn export_frame(&mut self, image: Image, time_ref: u32) -> ffi::VIDEO_FRAME_INFO_S {
        let size = image.data.len().max(image.width as usize * image.height as usize * 3 / 2);
        let mb = self.alloc_block(None, size);
        let block = self.block_mut(mb).unwrap();
        block.data[..image.data.len()].copy_from_slice(&image.data);

        let mut frame: ffi::VIDEO_FRAME_INFO_S = unsafe { std::mem::zeroed() };
        frame.stVFrame.pMbBlk = mb;
        frame.stVFrame.u32Width = image.width;
        frame.stVFrame.u32Height = image.height;
        frame.stVFrame.u32VirWidth = image.width;
        frame.stVFrame.u32VirHeight = image.height;
        frame.stVFrame.enField = ffi::rkVIDEO_FIELD_E_VIDEO_FIELD_FRAME;
        frame.stVFrame.enPixelFormat = image.format;
        frame.stVFrame.enVideoFormat = ffi::rkVIDEO_FORMAT_E_VIDEO_FORMAT_LINEAR;
        frame.stVFrame.enCompressMode = ffi::rkCOMPRESS_MODE_E_COMPRESS_MODE_NONE;
        frame.stVFrame.enDynamicRange = ffi::rkDYNAMIC_RANGE_E_DYNAMIC_RANGE_SDR8;
        frame.stVFrame.enColorGamut = ffi::rkCOLOR_GAMUT_E_COLOR_GAMUT_BT601;
        frame.stVFrame.u32TimeRef = time_ref;
        frame.stVFrame.u64PTS = image.pts;
        frame
    }

    /// Reads a user supplied video frame into an owned image.
    pub fn import_frame(
        &self, module: ffi::rkMOD_ID_E, frame: *const ffi::VIDEO_FRAME_INFO_S,
    ) -> Result<Image, i32> {
        let Some(frame) = (unsafe { frame.as_ref() }) else {
            return Err(rk_err(module, ffi::rkEN_ERR_CODE_E_RK_ERR_NULL_PTR));
        };
        let vframe = &frame.stVFrame;
        let Some(block) = self.block(vframe.pMbBlk) else {
            return Err(rk_err(module, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM));
        };
        let pts = if vframe.u64PTS != 0 { vframe.u64PTS } else { self.now_us() };
        Image::import(
            vframe.enPixelFormat,
            vframe.u32Width,
            vframe.u32Height,
            vframe.u32VirWidth.max(vframe.u32Width),
            vframe.u32VirHeight.max(vframe.u32Height),
            &block.data,
            pts,
        )
        .ok_or(rk_err(module, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM))
    }
}

pub(crate) static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::new()));
/// Notified whenever new input is queued or resources are released.
pub(crate) static COND: Condvar = Condvar::new();

pub(crate) fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Deadline {
    NonBlock,
    At(Instant),
    Never,
}

impl Deadline {
    pub fn from_ms(ms: i32) -> Self {
        match ms {
            ..0 => Self::Never,
            0 => Self::NonBlock,
            ms => Self::At(Instant::now() + Duration::from_millis(ms as u64)),
        }
    }

    pub fn expired(&self) -> bool {
        match self {
            Self::NonBlock => true,
            Self::At(at) => Instant::now() >= *at,
            Self::Never => false,
        }
    }
}

/// Outcome of a non-blocking attempt to get a frame.
pub(crate) enum Pull<T> {
    Ready(T),
    /// Nothing available yet. The next frame is expected at the given instant if known.
    NotYet(Option<Instant>),
    Err(i32),
}

/// Waits until notified, `wake_at` is reached or the deadline expires.
pub(crate) fn wait(
    guard: MutexGuard<'static, State>, deadline: Deadline, wake_at: Option<Instant>,
) -> MutexGuard<'static, State> {
    let target = match (deadline, wake_at) {
        (Deadline::At(at), Some(wake)) => Some(at.min(wake)),
        (Deadline::At(at), None) => Some(at),
        (_, wake) => wake,
    };
    match target {
        Some(target) => {
            let timeout = target.saturating_duration_since(Instant::now());
            COND.wait_timeout(guard, timeout).unwrap_or_else(|e| e.into_inner()).0
        }
        None => COND.wait(guard).unwrap_or_else(|e| e.into_inner()),
    }
}

/// Repeats `attempt` until it produces a value, fails or the deadline expires.
pub(crate) fn pull_blocking<T>(
    module: ffi::rkMOD_ID_E,
    deadline: Deadline,
    mut attempt: impl FnMut(&mut State) -> Pull<T>,
) -> Result<(MutexGuard<'static, State>, T), i32> {
    let mut state = lock();
    loop {
        match attempt(&mut state) {
            Pull::Ready(value) => return Ok((state, value)),
            Pull::Err(code) => return Err(code),
            Pull::NotYet(wake_at) => {
                if deadline.expired() {
                    return Err(rk_err(module, ffi::rkEN_ERR_CODE_E_RK_ERR_BUF_EMPTY));
                }
                state = wait(state, deadline, wake_at);
            }
        }
    }
}

/// Pulls a frame from the source bound to `dst`.
pub(crate) fn pull_bound(state: &mut State, dst: ChnKey) -> Option<Pull<Image>> {
    let src = *state.binds.get(&dst)?;
    Some(match src.enModId {
        ffi::rkMOD_ID_E_RK_ID_VI => vi::pull(state, src.s32DevId, src.s32ChnId),
        ffi::rkMOD_ID_E_RK_ID_VPSS => vpss::pull(state, src.s32DevId, src.s32ChnId),
        _ => Pull::Err(rk_err(ffi::rkMOD_ID_E_RK_ID_SYS, ffi::rkEN_ERR_CODE_E_RK_ERR_NOT_SUPPORT)),
    })
}

pub(crate) fn sys_not_ready(module: ffi::rkMOD_ID_E) -> i32 {
    rk_err(module, ffi::rkEN_ERR_CODE_E_RK_ERR_NOTREADY)
}

#[derive(Default)]
pub(crate) struct Control {
    pub log: Vec<String>,
    pub fail: HashMap<String, VecDeque<i32>>,
}

pub(crate) static CONTROL: LazyLock<Mutex<Control>> = LazyLock::new(Default::default);

pub(crate) fn control() -> MutexGuard<'static, Control> {
    CONTROL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Records an API call and returns an injected failure code if one is pending.
pub(crate) fn enter(name: &str, args: fmt::Arguments) -> Option<i32> {
    let mut control = control();
    control.log.push(format!("{name}({args})"));
    control.fail.get_mut(name).and_then(|codes| codes.pop_front())
}
