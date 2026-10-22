fn main() {
    let target = std::env::var("TARGET").expect("TARGET var");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR var");
    let stub_name = "getauxval_stub";
    if target == "armv7-unknown-linux-gnueabihf" {
        let stub_src_filename = format!("{stub_name}.c");
        println!("cargo::rerun-if-changed={stub_src_filename}");
        cc::Build::new()
            .file(stub_src_filename)
            .compile(stub_name);

        // Althought cc prints link directives, it's a library crate so cargo
        // does not propagate them when building examples.
        // Pass link arg explicitly with the path to the lib
        println!("cargo:rustc-link-arg-examples={out_dir}/lib{stub_name}.a");
    }
}
