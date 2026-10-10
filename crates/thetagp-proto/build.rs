//! Generates the Rust types from the pinned schema.
//!
//! The schema is this crate's submodule at `schema/`, pinned to a tag: the
//! generated types are the wire's messages, and nothing here is hand-written
//! from them. Both files of the schema carry the same package, so one module
//! comes out of them.

use std::path::PathBuf;

fn main() {
    let schema = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema");
    if !schema.join("protocol/ThetaGP.proto").is_file() {
        panic!(
            "the schema submodule is not there: {} is missing. Run \
             `git submodule update --init`.",
            schema.display()
        );
    }

    let protos = [
        schema.join("protocol/ThetaGP.proto"),
        schema.join("enums/enums.proto"),
    ];
    for proto in &protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }

    prost_build::Config::new()
        .compile_protos(&protos, &[schema])
        .expect("the schema did not compile");
}
