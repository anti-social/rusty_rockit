use std::io::Write;

use rusty_rockit::RockitSys;

fn main() {
    println!("Hello rockit!");
    let rockit_sys = RockitSys::init().expect("Rockit");

    let dev = rockit_sys.dev(0, 1).expect("Rockit dev");

    let pipe = dev.get_pipe(0).expect("Rockit pipe");
    let channel = pipe.create_channel(0, 1920, 1080).expect("Rockit channel");

    let frame = channel.get_frame().expect("Get frame");
    println!("Frame size: {}x{}", frame.width(), frame.height());

    let frame_buf = frame.data().expect("Frame data");
    println!("Frame buffer len: {}", frame_buf.len());

    let mut file = std::fs::File::create("test_frame.yuv").expect("Create file");
    file.write_all(frame_buf).expect("Write file");
}
