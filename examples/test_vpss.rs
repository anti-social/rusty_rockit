use core::time::Duration;

use rusty_rockit::{PixelFormat, RockitSys};
use rusty_rockit::vpss::{FrameRateControl, VpssChannelConfig, VpssGroupConfig};

fn main() {
    env_logger::init();
    
    run().expect("Run");
}

fn run() -> Result<(), rusty_rockit::Error>{
    let mpi = RockitSys::init()?;

    let input_pixel_format = PixelFormat::Rgb24;
    let output_pixel_format = PixelFormat::Rgb24;
    let width = 1920;
    let height = 1080;
    
    let vpss_config = VpssGroupConfig {
        pixel_format: input_pixel_format,
        max_width: width,
        max_height: height,
        frame_rate: FrameRateControl {
            src: 30,
            dst: 30,
        },
    };
    let vpss_group = mpi.vpss_group(&vpss_config)?;
    let vpss_channel_config = VpssChannelConfig {
        pixel_format: output_pixel_format,
        width: width,
        height: height,
        frame_rate: FrameRateControl {
            src: 30,
            dst: 30,
        },
        mirror: false,
        flip: false,
        queue_size: 1,
        frame_buffer_count: 2, 
    };
    let vpss_group = vpss_group.start()?;
    let vpss_channel = vpss_group.channel(&vpss_channel_config)?;
    let vpss_channel = vpss_channel.enable()?;

    let bytes_per_pixel = vpss_config.pixel_format.bytes_per_pixel();
    let input_buffer_size = width as u32 * height as u32 *
        bytes_per_pixel.0 as u32 / bytes_per_pixel.1 as u32;
    println!(">>> Input buffer size: {input_buffer_size}");
    let pool = mpi.pool(input_buffer_size)?;
    let mut buffer = pool.get_buffer(input_buffer_size)?;
    let buf_data = buffer.data_mut()?;
    buf_data.fill(128);

    let mut input_frame = buffer.new_frame(input_pixel_format, width, height);
    println!(">>> Sending frame");
    vpss_group.send_frame(0, &mut input_frame, Duration::from_millis(100))?;

    println!(">>> Getting frame");
    let output_frame = vpss_channel.get_frame(Duration::from_millis(100))?;
    let frame_data = output_frame.data()?;
    println!(">>> Frame data len: {}", frame_data.len());

    Ok(())
}
