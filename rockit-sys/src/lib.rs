pub use rockit_bindings::*;

// On non-ARM targets the Rockit symbols come from the fake library.
// Referencing the crate makes sure it is linked into every dependent binary.
#[cfg(not(target_arch = "arm"))]
extern crate rockit as _;
