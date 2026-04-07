fn main() {
    let pwd = std::env::current_dir().unwrap();
    let pwd = pwd.as_os_str().to_string_lossy();
    println!("cargo::rustc-link-search={pwd}/vendor/rockit/lib32");
    println!("cargo::rustc-link-search={pwd}/vendor/mpp/lib");
    println!("cargo::rustc-link-search={pwd}/vendor/rga/lib");
    println!("cargo::rustc-link-lib=rockit");
    println!("cargo::rustc-link-lib=rockchip_mpp");
    println!("cargo::rustc-link-lib=rga");

    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .clang_arg("-Ivendor/rockit/include")
        .clang_arg("-Ivendor/isp/include/rkaiq")
        .clang_arg("-Ivendor/isp/include/rkaiq/algos")
        .clang_arg("-Ivendor/isp/include/rkaiq/common")
        .clang_arg("-Ivendor/isp/include/rkaiq/iq_parser")
        .clang_arg("-Ivendor/isp/include/rkaiq/iq_parser_v2")
        .clang_arg("-Ivendor/isp/include/rkaiq/uAPI2")
        .clang_arg("-Ivendor/isp/include/rkaiq/xcore")
        .generate()
        .expect("Unable to generate bindings");

    bindings
        .write_to_file(out_path.join("bindings_mpp.rs"))
        .expect("Couldn't write bindings!");

    println!("cargo::rustc-link-search={pwd}/vendor/isp/lib");
    println!("cargo::rustc-link-lib=rkaiq");

    let aiq_bindings = bindgen::Builder::default()
        .header("aiq.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .blocklist_item("FP_NAN")
        .blocklist_item("FP_INFINITE")
        .blocklist_item("FP_ZERO")
        .blocklist_item("FP_SUBNORMAL")
        .blocklist_item("FP_NORMAL")
        .opaque_type("__pthread_unwind_buf_t__bindgen_ty_1")
        .clang_arg("-Ivendor/rockit/include")
        .clang_arg("-Ivendor/isp/include/rkaiq")
        .clang_arg("-Ivendor/isp/include/rkaiq/algos")
        .clang_arg("-Ivendor/isp/include/rkaiq/common")
        .clang_arg("-Ivendor/isp/include/rkaiq/iq_parser")
        .clang_arg("-Ivendor/isp/include/rkaiq/iq_parser_v2")
        .clang_arg("-Ivendor/isp/include/rkaiq/uAPI2")
        .clang_arg("-Ivendor/isp/include/rkaiq/xcore")
        .generate()
        .expect("Unable to generate bindings");

    aiq_bindings
        .write_to_file(out_path.join("bindings_aiq.rs"))
        .expect("Couldn't write bindings!");
}
