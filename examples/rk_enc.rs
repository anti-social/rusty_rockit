use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use argh::{FromArgs, FromArgValue};
use rusty_rockit::RockitSys;
use rusty_rockit::venc::{Codec, H26xRateControl, H264Profile, HevcProfile, StreamFrame, VencConfig};

/// Test rockchip encoder
#[derive(Debug, FromArgs)]
pub struct Args {
    // /// input raw file
    // #[argh(option, short = 'i')]
    // input_file: PathBuf,
    /// width
    #[argh(option, short = 'w')]
    width: u16,
    /// height
    #[argh(option, short = 'h')]
    height: u16,

    /// logging system
    #[argh(option, short = 'e', default = "Encoder::H264")]
    encoder: Encoder,
    /// output file
    #[argh(option, short = 'o')]
    output_file: Option<PathBuf>,
}

#[derive(Debug, FromArgValue)]
enum Encoder {
    H264,
    Hevc,
}

fn main() {
    let args: Args = argh::from_env();

    let codec = match args.encoder {
        Encoder::H264 => Codec::H264 {
            rate_control: H26xRateControl::Cbr {
                gop: 30,
                framerate: 30,
                bitrate_kbps: 4 * 1024,
            },
            profile: H264Profile::High,
        },
        Encoder::Hevc => Codec::Hevc {
            rate_control: H26xRateControl::Cbr {
                gop: 30,
                framerate: 30,
                bitrate_kbps: 4 * 1024,
            },
            profile: HevcProfile::Main,
        },
    };

    let output_filename = args.output_file.as_deref()
        .unwrap_or_else(|| match args.encoder {
            Encoder::H264 => Path::new("test-enc.h264"),
            Encoder::Hevc => Path::new("test-enc.hevc"),
        });
    let mut out_file = File::create(output_filename).expect("Create file");
        
    let rockit_sys = RockitSys::init().expect("Rockit");

    let enc_channel = rockit_sys.encoder(
        0,
        &VencConfig {
            width: args.width,
            height: args.height,
            codec,
            buf_count: 2,
        }
    ).expect("Encoder channel");
    let enc_channel = enc_channel.start().expect("Encoder start");
    let buffer_pool = rockit_sys.pool().expect("Buffer pool");
    let buf_size = args.width as u32 * args.height as u32 * 3 / 2;
    let mut mem_buffer = buffer_pool.get_buffer(buf_size).expect("Mem buffer");
    {
        let data = mem_buffer.data_mut().expect("Buffer data");
        data.fill(128);
    }
    let mut frame = mem_buffer.new_frame(args.width, args.height);
    let mut enc_frame = StreamFrame::new();
    for i in 0..30 {
        enc_channel.send_frame(&mut frame, Duration::from_millis(100)).expect("Send frame");

            let stream = enc_channel.get_stream(&mut enc_frame, Duration::from_millis(100))
                .expect("Encoder stream");
            let packet_data = stream.data().expect("Packet data");
            println!("{}: Packet len: {}", i + 1, packet_data.len());

            out_file.write_all(packet_data).expect("Write file");
    }
}
