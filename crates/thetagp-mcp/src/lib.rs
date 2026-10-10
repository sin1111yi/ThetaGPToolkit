//! The ThetaGP device, as tools a model can call.
//!
//! The backend is `thetagp-client`, a library this server calls in its own
//! process; nothing here goes through a command line. The protocol half is the
//! JSON-RPC a model host speaks over stdio; the tools are the device's own
//! commands and the protocol's.

pub mod protocol;
pub mod tools;
