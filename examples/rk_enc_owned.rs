use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use argh::{FromArgs, FromArgValue};
use rusty_rockit::{RockitSys, PixelFormat, SimpleEncoder};
use rusty_rockit::venc::{Codec, H26xRateControl, H264Profile, HevcProfile, VencConfig};

const DEFAULT_BITRATE: u32 = 4 * 1024;
const ENCODE_FRAME_TIMEOUT: Duration = Duration::from_millis(200);

/// Test rockchip encoder
#[derive(Debug, FromArgs)]
pub struct Args {
    /// input raw file
    #[argh(option, short = 'i')]
    input_file: Option<PathBuf>,
    /// width
    #[argh(option, short = 'w')]
    width: u16,
    /// height
    #[argh(option, short = 'h')]
    height: u16,

    /// encoder
    #[argh(option, short = 'e', default = "CodecKind::H264")]
    encoder: CodecKind,
    /// bitrate (kbps)
    #[argh(option, short = 'b', default = "DEFAULT_BITRATE")]
    bitrate_kbps: u32,
    /// framerate
    #[argh(option, short = 'r', default = "30")]
    framerate: u8,
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
                framerate: args.framerate,
                bitrate_kbps: args.bitrate_kbps,
            },
            profile: H264Profile::High,
        },
        CodecKind::Hevc => Codec::Hevc {
            rate_control: H26xRateControl::Cbr {
                gop: 30,
                framerate: args.framerate,
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

    let output_filename = args.output_file.as_deref()
        .unwrap_or_else(|| match args.encoder {
            CodecKind::H264 => Path::new("test-enc.h264"),
            CodecKind::Hevc => Path::new("test-enc.hevc"),
        });
    let mut out_file = File::create(output_filename).expect("Create file");
        
    let rockit_sys = RockitSys::init().expect("Rockit");

    let encoder_id = 0;
    let mut encoder = SimpleEncoder::new(
        &rockit_sys, encoder_id, &prepare_encoder_config(&args)
    )
        .expect("Simple encoder");
    let mut frame_buf = vec![0; encoder.frame_buf_size()];

    if let Some(input_file_path) = args.input_file {
        let mut in_file = File::open(input_file_path).expect("Input file open");
        loop {
            match in_file.read_exact(&mut frame_buf) {
                Ok(()) => {
                    let stream = encoder.encode_frame(&frame_buf, ENCODE_FRAME_TIMEOUT)
                        .expect("Encode frame");
                    let packet_data = stream.data().expect("Packet data");
                    out_file.write_all(packet_data).expect("Write file");
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(e) => {
                    panic!("Input file read: {e}");
                }
            }
        }
    } else {
        // Just generate 30 frames
        for i in 0..30 {
            frame_buf.fill(i * 8);
            let stream = encoder.encode_frame(&frame_buf, ENCODE_FRAME_TIMEOUT)
                .expect("Encode frame");
            let packet_data = stream.data().expect("Packet data");
            println!("{}: Packet len: {}", i + 1, packet_data.len());

            out_file.write_all(packet_data).expect("Write file");
        }
    }
}
