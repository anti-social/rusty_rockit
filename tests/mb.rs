mod common;

use common::*;
use rusty_rockit::Error;

#[test]
fn buffer_is_writable() {
    let mpi = setup();
    let pool = mpi.pool(1024).unwrap();
    let mut buf = pool.get_buffer(1024).unwrap();
    let data = buf.data_mut().unwrap();
    assert_eq!(data.len(), 1024);
    for (i, b) in data.iter_mut().enumerate() {
        *b = i as u8;
    }
    assert!(buf.data().unwrap().iter().enumerate().all(|(i, b)| *b == i as u8));
}

#[test]
fn pool_holds_a_single_buffer() {
    let mpi = setup();
    let pool = mpi.pool(1024).unwrap();
    let buf = pool.get_buffer(1024).unwrap();
    assert!(matches!(pool.get_buffer(1024), Err(Error::GetBuffer)));
    drop(buf);
    pool.get_buffer(1024).unwrap();
}

#[test]
fn buffer_larger_than_pool_block() {
    let mpi = setup();
    let pool = mpi.pool(1024).unwrap();
    assert!(matches!(pool.get_buffer(1025), Err(Error::GetBuffer)));
}

#[test]
fn owned_pool_outlives_borrowed_handles() {
    let mpi = setup();
    let pool = mpi.pool(16).unwrap().into_owned();
    let buf = pool.get_buffer(16).unwrap();
    drop(pool);
    // The buffer keeps the pool alive
    assert_eq!(live(Live::Pools), 1);
    drop(buf);
    assert_eq!(live(Live::Pools), 0);
    assert_eq!(live(Live::Mb), 0);
}

#[test]
fn create_pool_failure() {
    let mpi = setup();
    fail_next("RK_MPI_MB_CreatePool", 0);
    assert!(matches!(mpi.pool(16), Err(Error::CreatePool)));
}
