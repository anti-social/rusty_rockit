#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]


#[cfg(feature = "mpi")]
pub mod mpi {
    include!(concat!(env!("OUT_DIR"), "/bindings_mpi.rs"));
}

#[cfg(feature = "aiq")]
pub mod aiq {
    #![allow(unused)]

    include!(concat!(env!("OUT_DIR"), "/bindings_aiq.rs"));
}
