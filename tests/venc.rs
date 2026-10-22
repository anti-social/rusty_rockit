mod common;

use common::*;
use rusty_rockit::venc::{Codec, StreamFrame};
use rusty_rockit::{Error, PixelFormat, ffi};

const W: u16 = 64;
const H: u16 = 32;

#[test]
fn encodes_h264_with_idr_every_gop() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(3));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let pool = mpi.pool(cfg.calc_frame_buffer_size()).unwrap();
    let mut buf = pool.get_buffer(cfg.calc_frame_buffer_size()).unwrap();
    buf.data_mut().unwrap().fill(128);
    let mut frame = buf.new_frame(PixelFormat::Nv12, W, H);
    let mut stream_frame = StreamFrame::new();

    let mut types = Vec::new();
    for _ in 0..4 {
        venc.send_frame(&mut frame, TIMEOUT).unwrap();
        let stream = venc.get_stream(&mut stream_frame, TIMEOUT).unwrap();
        let data = stream.data().unwrap();
        assert_eq!(&data[..4], &[0, 0, 0, 1]);
        types.push(h264_nal_types(data));
    }
    // SPS, PPS, IDR then P slices
    assert_eq!(types, [vec![7, 8, 5], vec![1], vec![1], vec![7, 8, 5]]);
}

#[test]
fn encodes_hevc() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, hevc(30));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let pool = mpi.pool(cfg.calc_frame_buffer_size()).unwrap();
    let buf = pool.get_buffer(cfg.calc_frame_buffer_size()).unwrap();
    let mut frame = buf.new_frame(PixelFormat::Nv12, W, H);
    let mut stream_frame = StreamFrame::new();

    venc.send_frame(&mut frame, TIMEOUT).unwrap();
    let stream = venc.get_stream(&mut stream_frame, TIMEOUT).unwrap();
    // VPS, SPS, PPS, IDR_W_RADL
    assert_eq!(hevc_nal_types(stream.data().unwrap()), [32, 33, 34, 19]);
}

#[test]
fn stream_is_released() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(30));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let pool = mpi.pool(cfg.calc_frame_buffer_size()).unwrap();
    let buf = pool.get_buffer(cfg.calc_frame_buffer_size()).unwrap();
    let mut frame = buf.new_frame(PixelFormat::Nv12, W, H);
    let mut stream_frame = StreamFrame::new();

    venc.send_frame(&mut frame, TIMEOUT).unwrap();
    let stream = venc.get_stream(&mut stream_frame, TIMEOUT).unwrap();
    assert_eq!(live(Live::Mb), 2);
    drop(stream);
    assert_eq!(live(Live::Mb), 1);
}

#[test]
fn get_stream_times_out_without_input() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(30));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let mut stream_frame = StreamFrame::new();
    assert_rockit_err(
        venc.get_stream(&mut stream_frame, TIMEOUT).map(|_| ()),
        ffi::rkMOD_ID_E_RK_ID_VENC,
        ffi::rkEN_ERR_CODE_E_RK_ERR_BUF_EMPTY,
    );
}

#[test]
fn channels_get_distinct_ids() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(30));
    let c0 = mpi.venc_channel(&cfg).unwrap();
    let c1 = mpi.venc_channel(&cfg).unwrap();
    assert_ne!(c0.id(), c1.id());
    assert_eq!(live(Live::VencChns), 2);
    drop(c0);
    // The freed id is reused
    let c2 = mpi.venc_channel(&cfg).unwrap();
    assert_ne!(c2.id(), c1.id());
}

#[test]
fn channel_ids_are_exhausted() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(30));
    let channels: Vec<_> = (0..ffi::VENC_MAX_CHN_NUM)
        .map(|_| mpi.venc_channel(&cfg).unwrap())
        .collect();
    assert_eq!(live(Live::VencChns), channels.len() as i32);
    assert!(matches!(
        mpi.venc_channel(&cfg),
        Err(Error::ResourceUnavailable { .. })
    ));
}

#[test]
fn create_failure_releases_channel_id() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(30));
    let code = rk_err(ffi::rkMOD_ID_E_RK_ID_VENC, ffi::rkEN_ERR_CODE_E_RK_ERR_NOMEM);
    fail_next("RK_MPI_VENC_CreateChn", code);
    assert_rockit_err(
        mpi.venc_channel(&cfg).map(|_| ()),
        ffi::rkMOD_ID_E_RK_ID_VENC,
        ffi::rkEN_ERR_CODE_E_RK_ERR_NOMEM,
    );
    assert_eq!(mpi.venc_channel(&cfg).unwrap().id(), 0);
}

#[test]
fn drop_stops_before_destroying() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(30));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let id = venc.id();
    clear_call_log();
    drop(venc);
    let log = call_log();
    assert_eq!(
        log,
        [format!("RK_MPI_VENC_StopRecvFrame({id})"), format!("RK_MPI_VENC_DestroyChn({id})")],
    );
    assert_eq!(live(Live::VencChns), 0);
}

#[test]
fn stop_and_restart() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(30));
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let venc = venc.stop().unwrap();
    clear_call_log();
    drop(venc.start().unwrap());
    let log = call_log();
    assert!(call_index(&log, "RK_MPI_VENC_StartRecvFrame") < call_index(&log, "RK_MPI_VENC_StopRecvFrame"));
}

#[test]
fn rejects_unsupported_size() {
    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, 0, H, h264(30));
    assert_rockit_err(
        mpi.venc_channel(&cfg).map(|_| ()),
        ffi::rkMOD_ID_E_RK_ID_VENC,
        ffi::rkEN_ERR_CODE_E_RK_ERR_ILLEGAL_PARAM,
    );
}

#[test]
fn explicit_channel_id() {
    use rusty_rockit::venc::VencChannel;

    let mpi = setup();
    let cfg = venc_config(PixelFormat::Nv12, W, H, h264(30));
    let c5 = VencChannel::new(&mpi, 5, &cfg).unwrap();
    assert_eq!(c5.id(), 5);
    assert!(matches!(
        VencChannel::new(&mpi, 5, &cfg),
        Err(Error::ResourceUnavailable { id: Some(5), .. })
    ));
    // Automatically allocated ids skip the reserved one
    let ids: Vec<_> = (0..6).map(|_| mpi.venc_channel(&cfg).unwrap()).collect();
    assert!(ids.iter().all(|c| c.id() != 5));
}

/// Encodes `count` frames of a `width`x`height` channel in clip mode.
fn clip_stream(codec: Codec, width: u16, height: u16, count: usize) -> Vec<Vec<u8>> {
    let mpi = setup();
    set_venc_source(VencSource::Clip);
    let cfg = venc_config(PixelFormat::Nv12, width, height, codec);
    let venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
    let pool = mpi.pool(cfg.calc_frame_buffer_size()).unwrap();
    let buf = pool.get_buffer(cfg.calc_frame_buffer_size()).unwrap();
    let mut frame = buf.new_frame(PixelFormat::Nv12, width, height);
    let mut stream_frame = StreamFrame::new();
    (0..count)
        .map(|_| {
            venc.send_frame(&mut frame, TIMEOUT).unwrap();
            let stream = venc.get_stream(&mut stream_frame, TIMEOUT).unwrap();
            stream.data().unwrap().to_vec()
        })
        .collect()
}

#[test]
fn replays_h264_clip() {
    let clip = include_bytes!("../rockit-stub/clips/720p.h264");
    // The configured GOP is ignored: the clip has an IDR every 30 frames
    let packets = clip_stream(h264(3), 1280, 720, 61);
    assert_eq!(nal_units(&packets[..60].concat()), nal_units(clip));
    for (i, packet) in packets.iter().enumerate() {
        let types = h264_nal_types(packet);
        let expected: &[u8] = if i % 30 == 0 { &[7, 8, 5] } else { &[1] };
        assert_eq!(types, expected, "packet {i}");
    }
    assert_eq!(packets[60], packets[0], "the clip loops");
}

#[test]
fn replays_hevc_clip() {
    let clip = include_bytes!("../rockit-stub/clips/1080p.hevc");
    let packets = clip_stream(hevc(30), 1920, 1080, 31);
    assert_eq!(nal_units(&packets[..30].concat()), nal_units(&clip[..]).split_at(33).0);
    assert_eq!(hevc_nal_types(&packets[0]), [32, 33, 34, 20]);
    assert_eq!(hevc_nal_types(&packets[1]), [1]);
    assert_eq!(hevc_nal_types(&packets[30]), [32, 33, 34, 20]);
}

#[test]
fn clip_for_other_sizes_is_closest_one() {
    let clip = include_bytes!("../rockit-stub/clips/720p.h264");
    let packets = clip_stream(h264(30), W, H, 2);
    assert_eq!(nal_units(&packets.concat()), nal_units(clip)[..4]);
}
