mod common;

use std::time::{Duration, Instant};

use common::*;
use rusty_rockit::vi::CameraId;
use rusty_rockit::{Error, ffi};

#[test]
fn camera_produces_frames() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero).unwrap();
    let pipe = cam.pipe();
    let chn = pipe.create_channel(0, 64, 32).unwrap();
    for _ in 0..3 {
        let frame = chn.get_frame(TIMEOUT).unwrap();
        assert_eq!((frame.width(), frame.height()), (64, 32));
        assert_eq!(frame.data().unwrap().len(), 64 * 32 * 3 / 2);
    }
    assert_eq!(live(Live::Mb), 0);
}

#[test]
fn frames_are_paced() {
    let mpi = setup();
    set_vi_fps(50);
    let cam = mpi.camera(CameraId::Zero).unwrap();
    let pipe = cam.pipe();
    let chn = pipe.create_channel(0, 16, 16).unwrap();
    let start = Instant::now();
    for _ in 0..4 {
        drop(chn.get_frame(TIMEOUT).unwrap());
    }
    // First frame is immediate, then 20 ms per frame
    assert!(start.elapsed() >= Duration::from_millis(55), "{:?}", start.elapsed());
}

#[test]
fn camera_is_exclusive() {
    let mpi = setup();
    let _cam = mpi.camera(CameraId::One).unwrap();
    assert!(matches!(
        mpi.camera(CameraId::One),
        Err(Error::ResourceUnavailable { id: Some(1), .. })
    ));
    mpi.camera(CameraId::Two).unwrap();
}

#[test]
fn camera_reopened_after_drop() {
    let mpi = setup();
    drop(mpi.camera(CameraId::Zero).unwrap());
    assert_eq!(live(Live::ViDevs), 0);
    let cam = mpi.camera(CameraId::Zero).unwrap();
    assert_eq!(live(Live::ViDevs), 1);
    // The device attributes stay configured, so they are not set again
    assert_eq!(
        call_log().iter().filter(|c| c.starts_with("RK_MPI_VI_SetDevAttr")).count(),
        1
    );
    drop(cam);
}

#[test]
fn camera_binds_pipe_with_its_id() {
    for camera_id in [CameraId::Zero, CameraId::One, CameraId::Two] {
        let mpi = setup();
        let cam = mpi.camera(camera_id).unwrap();
        let id = camera_id as i32;
        assert_eq!((cam.id(), cam.pipe().id()), (id, id));
        let log = call_log();
        assert!(
            call_index(&log, "RK_MPI_VI_EnableDev")
                < call_index(&log, &format!("RK_MPI_VI_SetDevBindPipe({id}, pipes=[{id}])")),
            "{log:#?}"
        );
    }
}

#[test]
fn camera_disabled_when_pipe_bind_fails() {
    let mpi = setup();
    let code = rk_err(ffi::rkMOD_ID_E_RK_ID_VI, ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM);
    fail_next("RK_MPI_VI_SetDevBindPipe", code);
    assert_rockit_err(
        mpi.camera(CameraId::Zero),
        ffi::rkMOD_ID_E_RK_ID_VI,
        ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM,
    );
    assert_eq!(live(Live::ViDevs), 0);
    mpi.camera(CameraId::Zero).unwrap();
}

#[test]
fn channel_id_is_limited() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero).unwrap();
    let pipe = cam.pipe();
    let max = ffi::VI_MAX_CHN_NUM as u8;
    assert!(matches!(
        pipe.create_channel(max, 32, 16),
        Err(Error::InvalidChannelId { id }) if id == max
    ));
    pipe.create_channel(max - 1, 32, 16).unwrap();
}

#[test]
fn channel_disabled_on_drop() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero).unwrap();
    let pipe = cam.pipe();
    let chn = pipe.create_channel(1, 32, 16).unwrap();
    assert_eq!(live(Live::ViChns), 1);
    drop(chn);
    assert_eq!(live(Live::ViChns), 0);
}

#[test]
fn owned_channel_frames() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero).unwrap().into_owned();
    let chn = cam.pipe().create_channel(0, 32, 16).unwrap();
    drop(cam);
    let frame = chn.get_frame(TIMEOUT).unwrap();
    assert_eq!((frame.width(), frame.height()), (32, 16));
}

#[test]
fn channel_on_camera_one() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::One).unwrap();
    let pipe = cam.pipe();
    let chn = pipe.create_channel(0, 32, 16).unwrap();
    clear_call_log();
    let frame = chn.get_frame(TIMEOUT).unwrap();
    assert_eq!((frame.width(), frame.height()), (32, 16));
    drop(frame);
    drop(chn);
    let log = call_log();
    call_index(&log, "RK_MPI_VI_GetChnFrame(1, 0");
    call_index(&log, "RK_MPI_VI_ReleaseChnFrame(1, 0");
    call_index(&log, "RK_MPI_VI_DisableChn(1, 0)");
    assert_eq!(live(Live::ViChns), 0);
}
