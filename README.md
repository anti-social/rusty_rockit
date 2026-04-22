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
scp target/armv7-unknown-linux-gnueabihf/release/examples/rk_enc root@<ip-addr>:/root/
```

Ssh into it and run the example:

``` sh
./rk_vi_enc
```

Now you can download saved stream and play it:

``` sh
scp root@<ip-addr>:/root/test-stream.h264 ./test-stream.h264

ffplay -vf "setpts=1.0*N/FRAME_RATE/TB" test-stream.h264
```
