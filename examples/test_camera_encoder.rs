use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use argh::{FromArgs, FromArgValue};
use rusty_rockit::{CameraEncoder, PixelFormat, RockitMpi};
use rusty_rockit::aiq::AiqContext;
use rusty_rockit::venc::{
    Codec, H26xRateControl, H264Profile, HevcProfile, VencConfig
};

const DEFAULT_BITRATE: u32 = 4 * 1024;
const GET_FRAME_TIMEOUT: Duration = Duration::from_millis(200);

/// Test rockchip encoder
#[derive(Debug, FromArgs)]
pub struct Args {
    /// camera id
    #[argh(option, short = 'c', default = "0")]
    camera_id: u8,
    /// picture width
    #[argh(option, short = 'w', default = "1920")]
    width: u16,
    /// picture height
    #[argh(option, short = 'h', default = "1080")]
    height: u16,
    /// logging system
    #[argh(option, short = 'e', default = "CodecKind::H264")]
    encoder: CodecKind,
    /// bitrate (kbps)
    #[argh(option, short = 'b', default = "DEFAULT_BITRATE")]
    bitrate_kbps: u32,
    /// output file
    #[argh(option, short = 'o')]
    output_file: Option<PathBuf>,
}

#[derive(Debug, FromArgValue)]
enum CodecKind {
    H264,
    Hevc,
}

fn prepare_encoder_config(args: &Args) -> VencConfig {
    let codec = match args.encoder {
        CodecKind::H264 => Codec::H264 {
            rate_control: H26xRateControl::Cbr {
                gop: 30,
                framerate: 30,
                bitrate_kbps: args.bitrate_kbps,
            },
            profile: H264Profile::High,
        },
        CodecKind::Hevc => Codec::Hevc {
            rate_control: H26xRateControl::Cbr {
                gop: 30,
                framerate: 30,
                bitrate_kbps: args.bitrate_kbps,
            },
            profile: HevcProfile::Main,
        },
    };
    VencConfig {
        pixel_format: PixelFormat::Nv12,
        width: args.width,
        height: args.height,
        codec,
        buf_count: 2,
    }
}

fn main() {
    env_logger::init();

    let args: Args = argh::from_env();

    let camera_id = args.camera_id.try_into().expect("Camera id");

    let output_filename = args.output_file.as_deref()
        .unwrap_or_else(|| match args.encoder {
            CodecKind::H264 => Path::new("test-stream.h264"),
            CodecKind::Hevc => Path::new("test-stream.hevc"),
        });

    log::info!("Starting AIQ...");
    let aiq_ctx = AiqContext::init(camera_id, None).expect("AIQ context");
    let _aiq_ctx = aiq_ctx.start().expect("AIQ start");

    log::info!("Creating MPI context...");
    let rockit_sys = RockitMpi::init().expect("Rockit");
    let mut encoder = CameraEncoder::new(
        &rockit_sys, camera_id, &prepare_encoder_config(&args)
    )
        .expect("Camera encoder");
    let mut file = File::create(output_filename).expect("Create file");
    for i in 0..30 {
        let stream = encoder.get_frame(GET_FRAME_TIMEOUT).expect("Get frame");
        let packet_data = stream.data().expect("Packet data");
        println!("{}: Packet len: {}", i + 1, packet_data.len());

        file.write_all(packet_data).expect("Write file");
    }
}
