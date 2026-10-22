//! Helpers for integration tests running against the fake `librockit.so` (rockit-stub).
#![allow(dead_code)]

use std::ffi::{CString, c_char, c_int};
use std::ops::Deref;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusty_rockit::venc::{Codec, H26xRateControl, H264Profile, HevcProfile, VencConfig};
use rusty_rockit::{Error, PixelFormat, RockitErr, RockitMpi, ffi};

unsafe extern "C" {
    fn rockit_stub_reset();
    fn rockit_stub_fail_next(name: *const c_char, code: i32);
    fn rockit_stub_call_log(buf: *mut c_char, len: usize) -> usize;
    fn rockit_stub_clear_call_log();
    fn rockit_stub_live_count(kind: c_int) -> i32;
    fn rockit_stub_set_vi_fps(fps: u32);
}

#[derive(Clone, Copy, Debug)]
pub enum Live {
    Mb = 0,
    Pools = 1,
    VencChns = 2,
    VpssGrps = 3,
    Binds = 4,
    ViChns = 5,
    ViDevs = 6,
    AiqCtxs = 7,
}

pub const TIMEOUT: Duration = Duration::from_millis(100);

/// MPI and the fake library are process-global: tests of one binary run one at a time.
static LOCK: Mutex<()> = Mutex::new(());

/// Serializes the test and resets the fake library without initializing MPI.
pub fn lock() -> MutexGuard<'static, ()> {
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        rockit_stub_reset();
        rockit_stub_set_vi_fps(0);
    }
    guard
}

pub struct Fixture {
    // Dropped before the guard
    pub mpi: RockitMpi,
    _guard: MutexGuard<'static, ()>,
}

impl Deref for Fixture {
    type Target = RockitMpi;

    fn deref(&self) -> &RockitMpi {
        &self.mpi
    }
}

pub fn setup() -> Fixture {
    let guard = lock();
    let mpi = RockitMpi::init().expect("MPI init");
    Fixture { mpi, _guard: guard }
}

pub fn live(kind: Live) -> i32 {
    unsafe { rockit_stub_live_count(kind as c_int) }
}

pub fn call_log() -> Vec<String> {
    unsafe {
        let len = rockit_stub_call_log(std::ptr::null_mut(), 0);
        let mut buf = vec![0u8; len];
        rockit_stub_call_log(buf.as_mut_ptr() as *mut c_char, len);
        String::from_utf8(buf).unwrap().lines().map(str::to_string).collect()
    }
}

pub fn clear_call_log() {
    unsafe { rockit_stub_clear_call_log() }
}

/// Index of the first logged call that starts with `prefix`.
pub fn call_index(log: &[String], prefix: &str) -> usize {
    log.iter()
        .position(|c| c.starts_with(prefix))
        .unwrap_or_else(|| panic!("{prefix} not called, log: {log:#?}"))
}

pub fn fail_next(name: &str, code: i32) {
    let name = CString::new(name).unwrap();
    unsafe { rockit_stub_fail_next(name.as_ptr(), code) }
}

pub fn set_vi_fps(fps: u32) {
    unsafe { rockit_stub_set_vi_fps(fps) }
}

/// `RK_DEF_ERR` with `RK_ERR_LEVEL_ERROR`.
pub fn rk_err(module: ffi::rkMOD_ID_E, code: ffi::rkEN_ERR_CODE_E) -> i32 {
    (0xA000_0000u32 | (module << 16) | (ffi::rkERR_LEVEL_E_RK_ERR_LEVEL_ERROR << 13) | code) as i32
}

#[track_caller]
pub fn assert_rockit_err<T>(
    result: Result<T, Error>, module: ffi::rkMOD_ID_E, code: ffi::rkEN_ERR_CODE_E,
) {
    match result {
        Err(Error::Rockit { err: RockitErr { id, level, module: m } }) => {
            assert_eq!((m, level, id), (module, ffi::rkERR_LEVEL_E_RK_ERR_LEVEL_ERROR, code));
        }
        Err(e) => panic!("expected rockit error, got {e:?}"),
        Ok(_) => panic!("expected rockit error, got Ok"),
    }
}

pub fn h264(gop: u16) -> Codec {
    Codec::H264 {
        rate_control: H26xRateControl::Cbr { gop, framerate: 30, bitrate_kbps: 1024 },
        profile: H264Profile::Main,
    }
}

pub fn hevc(gop: u16) -> Codec {
    Codec::Hevc {
        rate_control: H26xRateControl::Vbr {
            gop,
            framerate: 30,
            bitrate_kbps: 1024,
            max_bitrate_kbps: 2048,
            min_bitrate_kbps: 512,
        },
        profile: HevcProfile::Main,
    }
}

pub fn venc_config(pixel_format: PixelFormat, width: u16, height: u16, codec: Codec) -> VencConfig {
    VencConfig { pixel_format, width, height, codec, buf_count: 2 }
}

/// Splits an Annex-B byte stream into NAL units (without start codes).
pub fn nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &start)| {
            let mut end = starts.get(n + 1).map_or(data.len(), |&next| next - 3);
            // 4-byte start code of the next unit
            if end > start && starts.get(n + 1).is_some() && data[end - 1] == 0 {
                end -= 1;
            }
            &data[start..end]
        })
        .collect()
}

pub fn h264_nal_types(data: &[u8]) -> Vec<u8> {
    nal_units(data).iter().map(|nal| nal[0] & 0x1f).collect()
}

pub fn hevc_nal_types(data: &[u8]) -> Vec<u8> {
    nal_units(data).iter().map(|nal| (nal[0] >> 1) & 0x3f).collect()
}
