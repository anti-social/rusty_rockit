Install cross tool:

``` sh
cargo install cross
```

Build examples:

``` sh
cross build --release --examples --target armv7-unknown-linux-gnueabihf
```

Copy an example to your rv1103/rv1106 mini computer:

``` sh
scp target/armv7-unknown-linux-gnueabihf/release/examples/test_camera_encoder root@<ip-addr>:/root/
```

Ssh into it and run the example:

``` sh
./test_camera_encoder
```

Now you can download saved stream and play it:

``` sh
scp root@<ip-addr>:/root/test-stream.h264 ./test-stream.h264

ffplay -vf "setpts=1.0*N/FRAME_RATE/TB" test-stream.h264
```

## Camera usage

A camera is a VI device. Rockit binds a single pipe to it, with the same id as the
device, so `Camera::pipe()` takes no arguments. Channels are created on that pipe:

``` rust
let mpi = RockitMpi::init()?;
let cam = mpi.camera(CameraId::Zero)?;
let pipe = cam.pipe();                                // pipe id == camera id
let vi_channel = pipe.create_channel(0, 1920, 1080)?; // channel id < VI_MAX_CHN_NUM

// Read raw NV12 frames directly...
let frame = vi_channel.get_frame(timeout)?;

// ...or bind the channel to an encoder (started VENC channel)
let bind = venc.bind_vi(&vi_channel)?;
let stream = bind.get_stream(&mut stream_frame, timeout)?;
```

VI feeds a whole VPSS group, so the group is bound and frames are read from its
channels:

``` rust
let group = mpi.vpss_group(&group_cfg)?.start()?;
let small = group.channel(&channel_cfg)?.enable()?;
let _vi_vpss = group.bind_vi(&vi_channel)?;
let frame = small.get_frame(timeout)?;
```

A VPSS channel can in turn feed an encoder with `venc.bind_vpss(&small)`. Binds are
undone when dropped. `mpi.camera(id)?.into_owned()` and the other `into_owned()`
variants return `Rc`-based handles without lifetimes, as used by `CameraEncoder`.

## Building on x86_64

On non-ARM hosts `rockit-sys` links against a fake `librockit.so` built from the
`rockit-stub` crate instead of the vendored ARM libraries. The fake implements the
subset of the Rockit MPI and rkaiq API used by this crate: memory buffers live on
the heap, VI channels produce a test pattern, VPSS converts/scales frames and VENC
emits fake (non-decodable) H.264/HEVC packets.

Build the stub first, then everything else:

``` sh
cargo build -p rockit-stub
cargo build --examples
cargo run --example test_simple_encoder -- -o /tmp/out.h264
```

or `make host` / `make host-test`. Set `ROCKIT_STUB_LIB_DIR` to link a stub from
another directory and `ROCKIT_STUB_VI_FPS` to change the fake camera frame rate
(`0` disables pacing).
