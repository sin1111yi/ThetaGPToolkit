//! The ThetaGP protocol client: the wire and the messages, and the exchange
//! between them.
//!
//! One connection carries one exchange at a time. A [`Session`] numbers the
//! frames it writes, holds each reply against its request, and hands the reply
//! up; a [`Transport`] is what the bytes travel over -- a serial port in the
//! field, and the simulated device in [`sim`] for tests and for an offline run.
//!
//! Nothing here names a board or a port: the consumer above decides which device
//! to talk to, and every method below is one arm of the schema.

pub mod backup;
pub mod config;
pub mod dynamic;
pub mod input;
pub mod keys;
pub mod session;
pub mod sim;
pub mod transport;

pub use backup::{Restored, Snapshot};
pub use config::Config;
pub use dynamic::{Arm, DynamicError};
pub use input::{
    axis_name, button_index, is_hat, key_name, pad_button_index, Axis, Reading,
};
pub use keys::{Key, KeySpace, KeyType, Reject, Value};
pub use session::{Sequence, Session, SessionError};
pub use transport::{SerialTransport, Transport};

/// The arms, re-exported so a consumer names a command through this crate.
pub use thetagp_proto as proto;
/// The wire, re-exported for the same reason.
pub use thetagp_wire as wire;
