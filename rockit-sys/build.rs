fn main() {
    let pwd = std::env::current_dir().unwrap();
    let pwd = pwd.as_os_str().to_string_lossy();
    println!("cargo::rustc-link-search={pwd}/vendor/rockit/lib32");
    println!("cargo::rustc-link-search={pwd}/vendor/mpp/lib");
    println!("cargo::rustc-link-search={pwd}/vendor/rga/lib");
    println!("cargo::rustc-link-lib=rockit");
    println!("cargo::rustc-link-lib=rockchip_mpp");
    println!("cargo::rustc-link-lib=rga");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .clang_arg("-Ivendor/rockit/include")
        .generate()
        .expect("Unable to generate bindings");

    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
