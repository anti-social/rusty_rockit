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
