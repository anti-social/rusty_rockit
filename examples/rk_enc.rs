use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use argh::{FromArgs, FromArgValue};
use rusty_rockit::RockitSys;
use rusty_rockit::aiq::AiqContext;
use rusty_rockit::venc::{Codec, H26xRateControl, H264Profile, HevcProfile, VencConfig};

/// Test rockchip encoder
#[derive(Debug, FromArgs)]
pub struct Args {
    /// camera id
    #[argh(option, short = 'c', default = "0")]
    camera_id: u8,
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

    let camera_id = args.camera_id;
    let width = 1920;
    let height = 1080;

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
            Encoder::H264 => Path::new("test-stream.h264"),
            Encoder::Hevc => Path::new("test-stream.hevc"),
        });
        
    let aiq_ctx = AiqContext::init(camera_id).expect("AIQ context");
    let _aiq_ctx = aiq_ctx.start().expect("AIQ start");

    let rockit_sys = RockitSys::init().expect("Rockit");

    let cam = rockit_sys.camera(camera_id, 1).expect("Camera device");

    let pipe = cam.get_pipe(0).expect("Rockit pipe");
    let channel = pipe.create_channel(0, width, height).expect("Rockit channel");

    let enc_channel = rockit_sys.encoder(
        0,
        &VencConfig {
            width,
            height,
            codec,
            buf_count: 2,
        }
    ).expect("Encoder channel");
    let enc_channel = enc_channel.start().expect("Encoder start");
    {
        let enc = enc_channel.bind(&channel).expect("Bind encoder");
        let mut frame = enc.alloc_frame();

        let mut file = File::create(output_filename).expect("Create file");

        for i in 0..30 {
            let stream = enc.get_stream(&mut frame).expect("Encoder stream");
            let packet_data = stream.data().expect("Packet data");
            println!("{}: Packet len: {}", i + 1, packet_data.len());

            file.write_all(packet_data).expect("Write file");

            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }
}
