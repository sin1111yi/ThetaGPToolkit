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

/// The name an enum value carries in the schema, by the enum's own name and the
/// number a value holds; the enum's prefix is left off, so `SOCDMode` at 0 reads
/// `UpPriority`.
pub fn enum_value_name(enum_name: &str, value: i32) -> Option<String> {
    use std::sync::OnceLock;

    static POOL: OnceLock<Option<prost_reflect::DescriptorPool>> = OnceLock::new();
    let pool = POOL
        .get_or_init(|| prost_reflect::DescriptorPool::decode(DESCRIPTOR_SET).ok())
        .as_ref()?;

    let descriptor = pool
        .get_enum_by_name(enum_name)
        .or_else(|| pool.get_enum_by_name(&format!("ThetaGP.{enum_name}")))?;
    let found = descriptor.get_value(value)?;
    let name = found.name();

    // The enumerators of one enum share a prefix -- the enum's own name, or its
    // values spelled in capitals -- and it is the same word on every line: what
    // a reader needs is the part that tells them apart.
    let spellings: Vec<String> = descriptor
        .values()
        .map(|entry| entry.name().to_string())
        .collect();
    let first = spellings.first().cloned().unwrap_or_else(|| name.to_string());
    let mut shared = first.len();
    for other in &spellings {
        shared = shared.min(
            first
                .chars()
                .zip(other.chars())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    let cut = first[..shared].rfind('_').map(|at| at + 1).unwrap_or(0);
    Some(match name.get(cut..) {
        Some(short) if !short.is_empty() => short.to_string(),
        _ => name.to_string(),
    })
}

/// Every value an enum declares, as the name a reader sees and the number it
/// stands for, in number order. The prefix every enumerator shares is left off,
/// as it is for one value.
pub fn enum_values(enum_name: &str) -> Vec<(String, i32)> {
    use std::sync::OnceLock;

    static POOL: OnceLock<Option<prost_reflect::DescriptorPool>> = OnceLock::new();
    let Some(pool) = POOL
        .get_or_init(|| prost_reflect::DescriptorPool::decode(DESCRIPTOR_SET).ok())
        .as_ref()
    else {
        return Vec::new();
    };
    let Some(descriptor) = pool
        .get_enum_by_name(enum_name)
        .or_else(|| pool.get_enum_by_name(&format!("ThetaGP.{enum_name}")))
    else {
        return Vec::new();
    };

    let spellings: Vec<(String, i32)> = descriptor
        .values()
        .map(|entry| (entry.name().to_string(), entry.number()))
        .collect();
    let Some((first, _)) = spellings.first() else {
        return Vec::new();
    };
    let mut shared = first.len();
    for (other, _) in &spellings {
        shared = shared.min(
            first
                .chars()
                .zip(other.chars())
                .take_while(|(a, b)| a == b)
                .count(),
        );
    }
    let cut = first[..shared].rfind('_').map(|at| at + 1).unwrap_or(0);

    let mut values: Vec<(String, i32)> = spellings
        .into_iter()
        .map(|(name, number)| {
            let short = name.get(cut..).unwrap_or(&name).to_string();
            (short, number)
        })
        .collect();
    values.sort_by_key(|(_, number)| *number);
    values
}
