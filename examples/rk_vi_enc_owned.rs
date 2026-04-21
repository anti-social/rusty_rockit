use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use argh::{FromArgs, FromArgValue};
use rusty_rockit::RockitSys;
use rusty_rockit::aiq::AiqContext;
use rusty_rockit::venc::{
    self, Codec, H26xRateControl, H264Profile, HevcProfile, StreamFrame, VencChannelBindOwned, VencChannelOwned, VencConfig
};

/// Test rockchip encoder
#[derive(Debug, FromArgs)]
pub struct Args {
    /// camera id
    #[argh(option, short = 'c', default = "0")]
    camera_id: u8,
    /// logging system
    #[argh(option, short = 'e', default = "CodecKind::H264")]
    encoder: CodecKind,
    /// output file
    #[argh(option, short = 'o')]
    output_file: Option<PathBuf>,
}

#[derive(Debug, FromArgValue)]
enum CodecKind {
    H264,
    Hevc,
}

struct CameraEncoder {
    enc_channel: VencChannelOwned<venc::state::Started>,
    _bind: VencChannelBindOwned,
    frame: StreamFrame,
}

impl CameraEncoder {
    fn new(
        mpi: &RockitSys,
        camera_id: u8,
        codec: CodecKind,
        width: u16,
        height: u16,
    ) -> Result<Self, rusty_rockit::Error> {
        let codec = match codec {
            CodecKind::H264 => Codec::H264 {
                rate_control: H26xRateControl::Cbr {
                    gop: 30,
                    framerate: 30,
                    bitrate_kbps: 4 * 1024,
                },
                profile: H264Profile::High,
            },
            CodecKind::Hevc => Codec::Hevc {
                rate_control: H26xRateControl::Cbr {
                    gop: 30,
                    framerate: 30,
                    bitrate_kbps: 4 * 1024,
                },
                profile: HevcProfile::Main,
            },
        };

        let cam = mpi.camera(camera_id, 1).expect("Camera device").into_owned();

        let pipe = cam.get_pipe(0).expect("Rockit pipe");
        let channel = pipe.create_channel(0, width, height).expect("Rockit channel");

        let enc_channel = mpi.encoder(
            0,
            &VencConfig {
                width,
                height,
                codec,
                buf_count: 2,
            }
        )
            .expect("Encoder channel")
            .into_owned();
        let enc_channel = enc_channel.start().expect("Encoder start");
        let bind = enc_channel.bind(&channel).expect("Bind encoder");
        let frame = StreamFrame::new();
        Ok(Self { enc_channel, _bind: bind, frame })
    }

    fn get_frame(&mut self) -> Result<&[u8], rusty_rockit::Error> {
        let stream = self.enc_channel.get_stream(&mut self.frame, Duration::from_millis(100))?;
        stream.data()
    }
}

fn main() {
    env_logger::init();

    let args: Args = argh::from_env();

    let camera_id = args.camera_id;
    let width = 1920;
    let height = 1080;


    let output_filename = args.output_file.as_deref()
        .unwrap_or_else(|| match args.encoder {
            CodecKind::H264 => Path::new("test-stream.h264"),
            CodecKind::Hevc => Path::new("test-stream.hevc"),
        });

    log::info!("Starting AIQ...");
    let aiq_ctx = AiqContext::init(camera_id, None).expect("AIQ context");
    let _aiq_ctx = aiq_ctx.start().expect("AIQ start");

    log::info!("Creating MPI context...");
    let rockit_sys = RockitSys::init().expect("Rockit");
    let mut encoder = CameraEncoder::new(&rockit_sys, camera_id, args.encoder, width, height)
        .expect("Encoder");
    let mut file = File::create(output_filename).expect("Create file");
    for i in 0..30 {
        let packet_data = encoder.get_frame().expect("Get frame");
        println!("{}: Packet len: {}", i + 1, packet_data.len());

        file.write_all(packet_data).expect("Write file");
    }
}
