mod common;

use std::time::Duration;

use common::*;
use rusty_rockit::venc::StreamFrame;
use rusty_rockit::vi::CameraId;
use rusty_rockit::vpss::{FrameRateControl, VpssChannelConfig, VpssGroupConfig};
use rusty_rockit::{CameraEncoder, PixelFormat, SimpleEncoder, ffi};

const FRAME_RATE: FrameRateControl = FrameRateControl { src: 30, dst: 30 };

#[test]
fn camera_encoder() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, 64, 32, h264(2));
    let mut encoder = CameraEncoder::new(&mpi, CameraId::Zero, &cfg).unwrap();
    let mut types = Vec::new();
    for _ in 0..3 {
        let stream = encoder.get_frame(TIMEOUT).unwrap();
        types.push(h264_nal_types(stream.data().unwrap()));
    }
    assert_eq!(types, [vec![7, 8, 5], vec![1], vec![7, 8, 5]]);
    assert_eq!(live(Live::Binds), 1);

    clear_call_log();
    drop(encoder);
    let log = call_log();
    assert!(
        call_index(&log, "RK_MPI_SYS_UnBind") < call_index(&log, "RK_MPI_VENC_DestroyChn"),
        "{log:#?}"
    );
    assert!(
        call_index(&log, "RK_MPI_SYS_UnBind") < call_index(&log, "RK_MPI_VI_DisableChn"),
        "{log:#?}"
    );
    for kind in [Live::Binds, Live::VencChns, Live::ViChns, Live::ViDevs, Live::Mb] {
        assert_eq!(live(kind), 0, "{kind:?}");
    }
}

#[test]
fn vi_venc_bind() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero).unwrap();
    let pipe = cam.pipe();
    let vi_chn = pipe.create_channel(0, 64, 32).unwrap();
    let cfg = venc_config(PixelFormat::Nv12, 64, 32, hevc(30));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let bind = venc.bind_vi(&vi_chn).unwrap();
    let mut stream_frame = StreamFrame::new();
    let stream = bind.get_stream(&mut stream_frame, TIMEOUT).unwrap();
    assert_eq!(hevc_nal_types(stream.data().unwrap()), [32, 33, 34, 19]);
}

#[test]
fn vi_vpss_bind() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero).unwrap();
    let pipe = cam.pipe();
    let vi_chn = pipe.create_channel(0, 64, 32).unwrap();
    let group_cfg = VpssGroupConfig {
        pixel_format: PixelFormat::Nv12,
        max_width: 64,
        max_height: 32,
        frame_rate: FRAME_RATE,
    };
    let group = mpi.vpss_group(&group_cfg).unwrap().start().unwrap();
    let chn_cfg = VpssChannelConfig {
        pixel_format: PixelFormat::Rgb24,
        width: 16,
        height: 8,
        frame_rate: FRAME_RATE,
        mirror: true,
        flip: true,
        queue_size: 1,
        frame_buffer_count: 2,
    };
    let chn = group.channel(&chn_cfg).unwrap().enable().unwrap();
    let _bind = group.bind_vi(&vi_chn).unwrap();
    let frame = chn.get_frame(TIMEOUT).unwrap();
    assert_eq!((frame.width(), frame.height()), (16, 8));
}

fn group_cfg(pixel_format: PixelFormat) -> VpssGroupConfig {
    VpssGroupConfig { pixel_format, max_width: 64, max_height: 32, frame_rate: FRAME_RATE }
}

fn chn_cfg(pixel_format: PixelFormat) -> VpssChannelConfig {
    VpssChannelConfig {
        pixel_format,
        width: 64,
        height: 32,
        frame_rate: FRAME_RATE,
        mirror: false,
        flip: false,
        queue_size: 1,
        frame_buffer_count: 2,
    }
}

/// `MPP_CHN_S` as logged by the fake: `mod:dev:chn`.
fn mpp_chn(module: ffi::rkMOD_ID_E, dev: i32, chn: i32) -> String {
    format!("{module}:{dev}:{chn}")
}

#[test]
fn vi_binds_to_vpss_group() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::One).unwrap();
    let pipe = cam.pipe();
    let vi_chn = pipe.create_channel(2, 64, 32).unwrap();
    let group_0 = mpi.vpss_group(&group_cfg(PixelFormat::Nv12)).unwrap().start().unwrap();
    let chn_0 = group_0.channel(&chn_cfg(PixelFormat::Nv12)).unwrap().enable().unwrap();
    let group_1 = mpi.vpss_group(&group_cfg(PixelFormat::Nv12)).unwrap().start().unwrap();
    assert_eq!(group_1.id(), 1);
    let _chn_1_0 = group_1.channel(&chn_cfg(PixelFormat::Nv12)).unwrap().enable().unwrap();
    let chn_1_1 = group_1.channel(&chn_cfg(PixelFormat::Nv12)).unwrap().enable().unwrap();
    assert_eq!(chn_1_1.id(), 1);

    clear_call_log();
    let bind = group_1.bind_vi(&vi_chn).unwrap();
    let expected = format!(
        "RK_MPI_SYS_Bind({} -> {})",
        mpp_chn(ffi::rkMOD_ID_E_RK_ID_VI, 1, 2),
        mpp_chn(ffi::rkMOD_ID_E_RK_ID_VPSS, 1, 0),
    );
    assert_eq!(call_log(), [expected]);

    assert!(chn_1_1.get_frame(TIMEOUT).is_ok());
    assert!(chn_0.get_frame(Duration::from_millis(10)).is_err(), "group 0 is not bound");

    drop(bind);
    assert_eq!(live(Live::Binds), 0);
}

#[test]
fn owned_vi_binds_to_vpss_group() {
    let mpi = setup();
    let cam = mpi.camera(CameraId::Zero).unwrap().into_owned();
    let vi_chn = cam.pipe().create_channel(0, 64, 32).unwrap();
    let _group_0 = mpi.vpss_group(&group_cfg(PixelFormat::Nv12)).unwrap();
    let group = mpi.vpss_group(&group_cfg(PixelFormat::Nv12)).unwrap().into_owned();
    let group = group.start().unwrap();
    let chn = group.channel(&chn_cfg(PixelFormat::Rgb24)).unwrap().enable().unwrap();
    let bind = group.bind_vi(&vi_chn).unwrap();
    drop((cam, vi_chn, group));
    let frame = chn.get_frame(TIMEOUT).unwrap();
    assert_eq!((frame.width(), frame.height()), (64, 32));
    drop(frame);

    clear_call_log();
    drop(chn);
    drop(bind);
    let log = call_log();
    assert!(
        call_index(&log, "RK_MPI_SYS_UnBind") < call_index(&log, "RK_MPI_VPSS_DestroyGrp(1"),
        "{log:#?}"
    );
    assert!(
        call_index(&log, "RK_MPI_SYS_UnBind") < call_index(&log, "RK_MPI_VI_DisableChn"),
        "{log:#?}"
    );
}

#[test]
fn venc_binds_to_vpss_group_channel() {
    let mpi = setup();
    let _group_0 = mpi.vpss_group(&group_cfg(PixelFormat::Yuyv)).unwrap();
    let group = mpi.vpss_group(&group_cfg(PixelFormat::Yuyv)).unwrap().start().unwrap();
    let _chn_0 = group.channel(&chn_cfg(PixelFormat::Nv12)).unwrap().enable().unwrap();
    let chn = group.channel(&chn_cfg(PixelFormat::Nv12)).unwrap().enable().unwrap();
    assert_eq!((chn.group_id(), chn.id()), (1, 1));
    let cfg = venc_config(PixelFormat::Nv12, 64, 32, h264(30));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();

    clear_call_log();
    let bind = venc.bind_vpss(&chn).unwrap();
    let expected = format!(
        "RK_MPI_SYS_Bind({} -> {})",
        mpp_chn(ffi::rkMOD_ID_E_RK_ID_VPSS, 1, 1),
        mpp_chn(ffi::rkMOD_ID_E_RK_ID_VENC, 0, venc.id()),
    );
    assert_eq!(call_log(), [expected]);

    let size = (PixelFormat::Yuyv.bytes_per_pixel() * 64 * 32).ceil();
    let pool = mpi.pool(size).unwrap();
    let buf = pool.get_buffer(size).unwrap();
    let mut frame = buf.new_frame(PixelFormat::Yuyv, 64, 32);
    group.send_frame(0, &mut frame, TIMEOUT).unwrap();
    let mut stream_frame = StreamFrame::new();
    let stream = bind.get_stream(&mut stream_frame, TIMEOUT).unwrap();
    assert_eq!(h264_nal_types(stream.data().unwrap()), [7, 8, 5]);
}

#[test]
fn vpss_venc_bind() {
    let mpi = setup();
    let group_cfg = VpssGroupConfig {
        pixel_format: PixelFormat::Yuyv,
        max_width: 64,
        max_height: 32,
        frame_rate: FRAME_RATE,
    };
    let group = mpi.vpss_group(&group_cfg).unwrap().start().unwrap();
    let chn_cfg = VpssChannelConfig {
        pixel_format: PixelFormat::Nv12,
        width: 64,
        height: 32,
        frame_rate: FRAME_RATE,
        mirror: false,
        flip: false,
        queue_size: 0,
        frame_buffer_count: 2,
    };
    let chn = group.channel(&chn_cfg).unwrap().enable().unwrap();
    let cfg = venc_config(PixelFormat::Nv12, 64, 32, h264(30));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let bind = venc.bind_vpss(&chn).unwrap();

    let size = (PixelFormat::Yuyv.bytes_per_pixel() * 64 * 32).ceil();
    let pool = mpi.pool(size).unwrap();
    let buf = pool.get_buffer(size).unwrap();
    let mut frame = buf.new_frame(PixelFormat::Yuyv, 64, 32);
    group.send_frame(0, &mut frame, TIMEOUT).unwrap();
    let mut stream_frame = StreamFrame::new();
    let stream = bind.get_stream(&mut stream_frame, TIMEOUT).unwrap();
    assert_eq!(h264_nal_types(stream.data().unwrap()), [7, 8, 5]);
}

#[test]
fn simple_encoder_converts_pixel_formats() {
    for pixel_format in [PixelFormat::Nv12, PixelFormat::Yuyv, PixelFormat::Rgb24] {
        let mpi = setup();
        let cfg = venc_config(pixel_format, 64, 32, h264(30));
        let mut encoder = SimpleEncoder::new(&mpi, &cfg).unwrap();
        let frame = vec![100u8; encoder.frame_buf_size()];
        for i in 0..3 {
            let stream = encoder.encode_frame(&frame, TIMEOUT).unwrap();
            let expected: &[u8] = if i == 0 { &[7, 8, 5] } else { &[1] };
            assert_eq!(h264_nal_types(stream.data().unwrap()), expected, "{pixel_format:?}");
        }
        drop(encoder);
        for kind in [Live::Binds, Live::VencChns, Live::VpssGrps, Live::Mb, Live::Pools] {
            assert_eq!(live(kind), 0, "{pixel_format:?} {kind:?}");
        }
    }
}
