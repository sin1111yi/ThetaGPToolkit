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

/// The schema's own description, as a descriptor set.
///
/// It is what a consumer reads to name a message, an arm or a field that this
/// build has no Rust type for: a console lists the arms from it, and a sender
/// builds one it was given by name.
pub const DESCRIPTOR_SET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/theta_gp.bin"));
