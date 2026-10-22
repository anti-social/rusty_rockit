mod common;

use common::*;
use rusty_rockit::{Error, RockitMpi, ffi};

#[test]
fn init_twice_fails() {
    let _mpi = setup();
    assert!(matches!(RockitMpi::init(), Err(Error::MpiAlreadyInitialized)));
}

#[test]
fn reinit_after_last_clone_dropped() {
    let _guard = lock();
    let mpi = RockitMpi::init().unwrap();
    let clone = mpi.clone();
    drop(mpi);
    assert!(matches!(RockitMpi::init(), Err(Error::MpiAlreadyInitialized)));
    drop(clone);
    assert!(call_log().iter().any(|c| c.starts_with("RK_MPI_SYS_Exit")));
    RockitMpi::init().unwrap();
}

#[test]
fn init_error_is_decoded() {
    let _guard = lock();
    fail_next(
        "RK_MPI_SYS_Init",
        rk_err(ffi::rkMOD_ID_E_RK_ID_SYS, ffi::rkEN_ERR_CODE_E_RK_ERR_NOTREADY),
    );
    assert_rockit_err(
        RockitMpi::init(),
        ffi::rkMOD_ID_E_RK_ID_SYS,
        ffi::rkEN_ERR_CODE_E_RK_ERR_NOTREADY,
    );
    // A failed init doesn't leave MPI marked as initialized
    RockitMpi::init().unwrap();
}

#[test]
fn everything_released_on_drop() {
    let mpi = setup();
    {
        let cfg = venc_config(rusty_rockit::PixelFormat::Nv12, 64, 32, h264(10));
        let _venc = mpi.venc_channel(&cfg).unwrap().start().unwrap();
        let pool = mpi.pool(64 * 32 * 3 / 2).unwrap();
        let _buf = pool.get_buffer(64 * 32 * 3 / 2).unwrap();
        let cam = mpi.camera(Default::default()).unwrap();
        let pipe = cam.pipe();
        let _chn = pipe.create_channel(0, 64, 32).unwrap();
        assert_eq!(live(Live::VencChns), 1);
        assert_eq!(live(Live::Mb), 1);
        assert_eq!(live(Live::ViDevs), 1);
        assert_eq!(live(Live::ViChns), 1);
    }
    for kind in [Live::Mb, Live::Pools, Live::VencChns, Live::ViChns, Live::ViDevs] {
        assert_eq!(live(kind), 0, "{kind:?}");
    }
}
