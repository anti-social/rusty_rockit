mod common;

use common::*;
use rusty_rockit::vpss::{FrameRateControl, VpssChannelConfig, VpssGroupConfig};
use rusty_rockit::{PixelFormat, ffi};

const FRAME_RATE: FrameRateControl = FrameRateControl { src: 30, dst: 30 };

fn group_config(pixel_format: PixelFormat) -> VpssGroupConfig {
    VpssGroupConfig { pixel_format, max_width: 1920, max_height: 1080, frame_rate: FRAME_RATE }
}

fn channel_config(pixel_format: PixelFormat, width: u16, height: u16) -> VpssChannelConfig {
    VpssChannelConfig {
        pixel_format,
        width,
        height,
        frame_rate: FRAME_RATE,
        mirror: false,
        flip: false,
        queue_size: 1,
        frame_buffer_count: 2,
    }
}

#[test]
fn converts_and_scales_frames() {
    let mpi = setup();
    let group = mpi.vpss_group(&group_config(PixelFormat::Rgb24)).unwrap().start().unwrap();
    let chn = group.channel(&channel_config(PixelFormat::Nv12, 32, 16)).unwrap();
    let chn = chn.enable().unwrap();

    let size = (PixelFormat::Rgb24.bytes_per_pixel() * 64 * 32).ceil();
    let pool = mpi.pool(size).unwrap();
    let mut buf = pool.get_buffer(size).unwrap();
    // White RGB frame
    buf.data_mut().unwrap().fill(255);
    let mut frame = buf.new_frame(PixelFormat::Rgb24, 64, 32);
    group.send_frame(0, &mut frame, TIMEOUT).unwrap();

    let out = chn.get_frame(TIMEOUT).unwrap();
    assert_eq!((out.width(), out.height()), (32, 16));
    let data = out.data().unwrap();
    assert_eq!(data.len(), 32 * 16 * 3 / 2);
    // BT.601 white: Y = 235, U = V = 128
    assert!(data[..32 * 16].iter().all(|y| *y == 235));
    assert!(data[32 * 16..].iter().all(|c| *c == 128));
}

#[test]
fn every_enabled_channel_gets_the_frame() {
    let mpi = setup();
    let group = mpi.vpss_group(&group_config(PixelFormat::Nv12)).unwrap().start().unwrap();
    let c0 = group.channel(&channel_config(PixelFormat::Nv12, 32, 16)).unwrap().enable().unwrap();
    let c1 = group.channel(&channel_config(PixelFormat::Yuyv, 16, 8)).unwrap().enable().unwrap();
    assert_ne!(c0.id(), c1.id());

    let pool = mpi.pool(64 * 32 * 3 / 2).unwrap();
    let buf = pool.get_buffer(64 * 32 * 3 / 2).unwrap();
    let mut frame = buf.new_frame(PixelFormat::Nv12, 64, 32);
    group.send_frame(0, &mut frame, TIMEOUT).unwrap();

    assert_eq!(c0.get_frame(TIMEOUT).unwrap().width(), 32);
    assert_eq!(c1.get_frame(TIMEOUT).unwrap().width(), 16);
    assert_rockit_err(
        c0.get_frame(TIMEOUT).map(|_| ()),
        ffi::rkMOD_ID_E_RK_ID_VPSS,
        ffi::rkEN_ERR_CODE_E_RK_ERR_BUF_EMPTY,
    );
}

#[test]
fn group_channels_are_limited() {
    let mpi = setup();
    let group = mpi.vpss_group(&group_config(PixelFormat::Nv12)).unwrap();
    let cfg = channel_config(PixelFormat::Nv12, 32, 16);
    let channels: Vec<_> = (0..ffi::VPSS_MAX_CHN_NUM).map(|_| group.channel(&cfg).unwrap()).collect();
    assert_eq!(channels.len(), ffi::VPSS_MAX_CHN_NUM as usize);
    assert!(group.channel(&cfg).is_err());
}

#[test]
fn groups_get_distinct_ids() {
    let mpi = setup();
    let cfg = group_config(PixelFormat::Nv12);
    let g0 = mpi.vpss_group(&cfg).unwrap();
    let g1 = mpi.vpss_group(&cfg).unwrap();
    assert_ne!(g0.id(), g1.id());
    assert_eq!(live(Live::VpssGrps), 2);
}

#[test]
fn group_ids_are_independent_of_venc_channels() {
    let mpi = setup();
    let venc_cfg = venc_config(PixelFormat::Nv12, 64, 32, h264(30));
    let _venc: Vec<_> = (0..ffi::VENC_MAX_CHN_NUM)
        .map(|_| mpi.venc_channel(&venc_cfg).unwrap())
        .collect();
    mpi.vpss_group(&group_config(PixelFormat::Nv12)).unwrap();
}

#[test]
fn drop_disables_stops_and_destroys() {
    let mpi = setup();
    let group = mpi.vpss_group(&group_config(PixelFormat::Nv12)).unwrap().start().unwrap();
    let chn = group.channel(&channel_config(PixelFormat::Nv12, 32, 16)).unwrap().enable().unwrap();
    clear_call_log();
    drop(chn);
    drop(group);
    let log = call_log();
    let disable = call_index(&log, "RK_MPI_VPSS_DisableChn");
    let stop = call_index(&log, "RK_MPI_VPSS_StopGrp");
    let destroy = call_index(&log, "RK_MPI_VPSS_DestroyGrp");
    assert!(disable < stop && stop < destroy, "{log:#?}");
    assert_eq!(live(Live::VpssGrps), 0);
}
