use std::fs::File;
use std::io::Write;

use rusty_rockit::RockitSys;
use rusty_rockit::aiq::AiqContext;

fn main() {
    println!("Hello rockit!");

    let camera_id = 0;
    let width = 1920;
    let height = 1080;

    let aiq_ctx = AiqContext::init(camera_id);
    let _aiq_ctx = aiq_ctx.start();

    let rockit_sys = RockitSys::init().expect("Rockit");

    let cam = rockit_sys.camera(camera_id, 1).expect("Rockit dev");

    let pipe = cam.get_pipe(0).expect("Rockit pipe");
    let channel = pipe.create_channel(0, width, height).expect("Rockit channel");

    let enc_channel = rockit_sys.encoder(0, width, height).expect("Encoder channel");
    let enc_channel = enc_channel.start().expect("Encoder start");
    {
        let enc = enc_channel.bind(&channel).expect("Bind encoder");
        let mut frame = enc.alloc_frame();

        let mut file = File::create("test_frame.h264").expect("Create file");

        for i in 0..30 {
            let stream = enc.get_stream(&mut frame).expect("Encoder stream");
            let packet_data = stream.data().expect("Packet data");
            println!("{i}: Packet len: {}", packet_data.len());

            file.write_all(packet_data).expect("Write file");

            std::thread::sleep(std::time::Duration::from_millis(30));
        }
    }
}
