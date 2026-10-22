mod common;

use common::*;
use rusty_rockit::venc::StreamFrame;
use rusty_rockit::vi::CameraId;
use rusty_rockit::vpss::{FrameRateControl, VpssChannelConfig, VpssGroupConfig};
use rusty_rockit::{CameraEncoder, PixelFormat, SimpleEncoder};

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
    let cam = mpi.camera(CameraId::Zero, 1).unwrap();
    let pipe = cam.get_pipe(0).unwrap();
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
    let cam = mpi.camera(CameraId::Zero, 1).unwrap();
    let pipe = cam.get_pipe(0).unwrap();
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
    let bind = chn.bind_vi(&vi_chn).unwrap();
    let frame = bind.get_frame(TIMEOUT).unwrap();
    assert_eq!((frame.width(), frame.height()), (16, 8));
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
