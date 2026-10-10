//! The protocol's messages, generated from the schema this crate carries.
//!
//! Every request and reply of the wire is here, so nothing above this crate
//! derives a message by hand: a field a schema adds appears here and nowhere
//! else. The schema is the submodule at the workspace root, pinned to a tag;
//! `build.rs` compiles it, and the pinned commit is what the pin is.

#![allow(clippy::all)]

pub mod thetagp {
    //! The types of `package ThetaGP`, both files of the schema together.
    include!(concat!(env!("OUT_DIR"), "/theta_gp.rs"));
}

pub use thetagp::*;
