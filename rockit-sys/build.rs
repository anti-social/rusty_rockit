fn main() {
    let pwd = std::env::current_dir().unwrap();
    let pwd = pwd.as_os_str().to_string_lossy();

    let out_path = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

    if std::env::var("CARGO_FEATURE_MPI").is_ok() {
        println!("cargo::rustc-link-search={pwd}/vendor/rockit/lib32");
        println!("cargo::rustc-link-search={pwd}/vendor/mpp/lib");
        println!("cargo::rustc-link-search={pwd}/vendor/rga/lib");
        println!("cargo::rustc-link-lib=rockit");
        println!("cargo::rustc-link-lib=rockchip_mpp");
        println!("cargo::rustc-link-lib=rga");

        let mpi_headers = &[
            "rk_debug.h",
            "rk_defines.h",
            "rk_mpi_adec.h",
            "rk_mpi_aenc.h",
            "rk_mpi_ai.h",
            "rk_mpi_ao.h",
            "rk_mpi_avs.h",
            "rk_mpi_cal.h",
            "rk_mpi_ivs.h",
            "rk_mpi_mb.h",
            "rk_mpi_rgn.h",
            "rk_mpi_sys.h",
            "rk_mpi_tde.h",
            "rk_mpi_vdec.h",
            "rk_mpi_venc.h",
            "rk_mpi_vi.h",
            "rk_mpi_vo.h",
            "rk_mpi_vpss.h",
        ];
        let mpi_bindings = bindgen::Builder::default()
            .headers(
                mpi_headers.map(|h| format!("vendor/rockit/include/{h}"))
            )
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .clang_arg("-Ivendor/rockit/include")
            .generate()
            .expect("Unable to generate bindings");

        mpi_bindings
            .write_to_file(out_path.join("bindings_mpi.rs"))
            .expect("Couldn't write bindings!");
    }

    if std::env::var("CARGO_FEATURE_AIQ").is_ok() {
        println!("cargo::rustc-link-search={pwd}/vendor/isp/lib");
        println!("cargo::rustc-link-lib=rkaiq");

        let aiq_headers = &[
            "rk_aiq_user_api2_sysctl.h",
        ];
        let aiq_bindings = bindgen::Builder::default()
            .headers(
                aiq_headers.map(|h| format!("vendor/isp/include/rkaiq/uAPI2/{h}"))
            )
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
}
