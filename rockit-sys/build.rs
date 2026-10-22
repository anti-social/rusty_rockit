use std::path::PathBuf;

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
    } else if mpi || aiq {
        // On non-ARM hosts link against the fake librockit.so built from the rockit-stub crate.
        // It exports both MPI and AIQ symbols.
        println!("cargo::rerun-if-env-changed=ROCKIT_STUB_LIB_DIR");
        let stub_dir = match std::env::var("ROCKIT_STUB_LIB_DIR") {
            Ok(dir) => PathBuf::from(dir),
            Err(_) => {
                // OUT_DIR = target/[triple/]<profile>/build/rockit-sys-<hash>/out
                let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
                out_dir.ancestors().nth(3).expect("target profile dir").to_path_buf()
            }
        };
        let stub_lib = stub_dir.join("librockit.so");
        println!("cargo::rerun-if-changed={}", stub_lib.display());
        if !stub_lib.exists() {
            panic!(
                "{} not found. Build the stub first: `cargo build -p rockit-stub` \
                 (with the same profile/target) or set ROCKIT_STUB_LIB_DIR",
                stub_lib.display(),
            );
        }
        println!("cargo::rustc-link-search=native={}", stub_dir.display());
        println!("cargo::rustc-link-lib=dylib=rockit");
        // Exposed to dependents' build scripts as DEP_ROCKIT_STUB_DIR
        println!("cargo::metadata=stub_dir={}", stub_dir.display());
    }
}
