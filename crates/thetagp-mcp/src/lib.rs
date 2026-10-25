//! The ThetaGP device, as tools a model can call.
//!
//! The tools are executed by `thetagp-tools`, which the daemon runs: this crate
//! is the MCP half, the JSON-RPC a model host speaks over stdio, and it passes a
//! call to the daemon and the answer back. `Backend` is what keeps the two
//! shapes -- the device in this process and the daemon that owns it -- the same
//! to everything above them.

pub mod protocol;
