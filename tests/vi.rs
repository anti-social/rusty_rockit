mod common;

use std::time::{Duration, Instant};

use common::*;
use rusty_rockit::vi::CameraId;
use rusty_rockit::{Error, ffi};

#[test]
fn camera_produces_frames() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero, 1).unwrap();
    let pipe = cam.get_pipe(0).unwrap();
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
    let cam = mpi.camera(CameraId::Zero, 1).unwrap();
    let pipe = cam.get_pipe(0).unwrap();
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
    let _cam = mpi.camera(CameraId::One, 1).unwrap();
    assert!(matches!(
        mpi.camera(CameraId::One, 1),
        Err(Error::ResourceUnavailable { id: Some(1), .. })
    ));
    mpi.camera(CameraId::Two, 1).unwrap();
}

#[test]
fn camera_reopened_after_drop() {
    let mpi = setup();
    drop(mpi.camera(CameraId::Zero, 1).unwrap());
    assert_eq!(live(Live::ViDevs), 0);
    let cam = mpi.camera(CameraId::Zero, 1).unwrap();
    assert_eq!(live(Live::ViDevs), 1);
    // The device attributes stay configured, so they are not set again
    assert_eq!(
        call_log().iter().filter(|c| c.starts_with("RK_MPI_VI_SetDevAttr")).count(),
        1
    );
    drop(cam);
}

#[test]
fn pipes_are_limited() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero, 2).unwrap();
    assert!(cam.get_pipe(1).is_some());
    assert!(cam.get_pipe(2).is_none());
    assert!(matches!(
        mpi.camera(CameraId::One, ffi::VI_MAX_PIPE_NUM as u8 + 1),
        Err(Error::RequestedTooManyPipes { .. })
    ));
}

#[test]
fn channel_disabled_on_drop() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero, 1).unwrap();
    let pipe = cam.get_pipe(0).unwrap();
    let chn = pipe.create_channel(1, 32, 16).unwrap();
    assert_eq!(live(Live::ViChns), 1);
    drop(chn);
    assert_eq!(live(Live::ViChns), 0);
}

#[test]
fn owned_channel_frames() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero, 1).unwrap().into_owned();
    let chn = cam.get_pipe(0).unwrap().create_channel(0, 32, 16).unwrap();
    drop(cam);
    let frame = chn.get_frame(TIMEOUT).unwrap();
    assert_eq!((frame.width(), frame.height()), (32, 16));
}
