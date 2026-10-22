//! Fake Rockit library for non-ARM hosts.
//!
//! `rockit-sys` links it statically on non-ARM targets. It is also built as a
//! standalone `librockit.so` (cdylib).
//!
//! Implements the subset of the Rockit MPI and rkaiq API used by `rusty-rockit` with
//! functional fakes: memory buffers live on the heap, VI produces a synthetic test pattern,
//! VPSS converts/scales frames and VENC emits fake Annex-B packets.
//!
//! Besides the Rockit API the library exports a small test-control API (`rockit_stub_*`),
//! see [`control`].

#![allow(non_snake_case)]
#![allow(clippy::missing_safety_doc)]

/// Returns the error code from the enclosing API function.
macro_rules! try_rk {
    ($e:expr) => {
        if let Err(code) = $e {
            return code;
        }
    };
}

pub mod aiq;
pub mod control;
mod image;
pub mod mb;
mod state;
pub mod sys;
pub mod venc;
pub mod vi;
pub mod vpss;

pub(crate) use rockit_bindings::mpi as ffi;
