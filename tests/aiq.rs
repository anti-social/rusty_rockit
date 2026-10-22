#![cfg(feature = "aiq")]

mod common;

use common::*;
use rusty_rockit::aiq::{AiqContext, Error};
use rusty_rockit::vi::CameraId;

#[test]
fn start_stop() {
    let _guard = lock();
    let ctx = AiqContext::init(CameraId::Zero, None).unwrap();
    assert_eq!(live(Live::AiqCtxs), 1);
    let ctx = ctx.start().unwrap().stop().unwrap();
    let ctx = ctx.start().unwrap();
    clear_call_log();
    drop(ctx);
    let log = call_log();
    assert!(call_index(&log, "rk_aiq_uapi2_sysctl_stop") < call_index(&log, "rk_aiq_uapi2_sysctl_deinit"));
    assert_eq!(live(Live::AiqCtxs), 0);
}

#[test]
fn missing_sensor() {
    let _guard = lock();
    assert!(matches!(AiqContext::init(CameraId::One, None), Err(Error::Aiq { .. })));
}

#[test]
fn init_failure() {
    let _guard = lock();
    fail_next("rk_aiq_uapi2_sysctl_init", 0);
    assert!(matches!(AiqContext::init(CameraId::Zero, None), Err(Error::AiqSystemContext)));
}
