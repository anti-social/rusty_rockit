fn main() {
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let mpi = std::env::var("CARGO_FEATURE_MPI").is_ok();
    let aiq = std::env::var("CARGO_FEATURE_AIQ").is_ok();

    if target_arch == "arm" {
        let pwd = std::env::var("CARGO_MANIFEST_DIR").unwrap();

        if mpi {
            println!("cargo::rustc-link-search={pwd}/vendor/rockit/lib32");
            println!("cargo::rustc-link-search={pwd}/vendor/mpp/lib");
            println!("cargo::rustc-link-search={pwd}/vendor/rga/lib");
            println!("cargo::rustc-link-lib=rockit");
            println!("cargo::rustc-link-lib=rockchip_mpp");
            println!("cargo::rustc-link-lib=rga");
        }

        if aiq {
            println!("cargo::rustc-link-search={pwd}/vendor/isp/lib");
            println!("cargo::rustc-link-lib=rkaiq");
        }
    }
    // On other targets the fake library from the rockit-stub crate is linked
    // statically as a regular dependency, see src/lib.rs.
}
