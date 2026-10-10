//! The exchange: the numbers the host writes, and holding a reply against the
//! request it answers.

use std::io;
use std::time::{Duration, Instant};

use prost::Message;
use thetagp_proto as proto;
use thetagp_wire as wire;

use crate::transport::Transport;

/// The frame numbers the host writes.
///
/// Every frame the host puts on the wire takes the next number, a reply carries
/// the number of its request plus one, and the host's next frame carries that
/// reply's number plus one. The requests are therefore numbered 0, 2, 4, ... and
/// the replies that answer them 1, 3, 5, ..., which is what tells a reply of the
/// exchange under way from a frame of another one.
#[derive(Debug, Default)]
pub struct Sequence {
    next: u32,
}

impl Sequence {
    pub fn new() -> Self {
        Sequence { next: 0 }
    }

    /// The number the next frame takes.
    pub fn take(&mut self) -> u32 {
        let number = self.next;
        self.next = number.wrapping_add(1);
        number
    }

    /// The number the host writes after a reply carrying `number`.
    pub fn after_reply(&mut self, number: u32) {
        self.next = number.wrapping_add(1);
    }

    /// The number the next frame would take.
    pub fn next(&self) -> u32 {
        self.next
    }
}

/// Why an exchange did not produce an answer.
#[derive(Debug)]
pub enum SessionError {
    /// The connection failed.
    Io(io::Error),
    /// No reply arrived before the deadline.
    Timeout(Duration),
    /// The request did not fit one frame.
    Payload,
    /// A reply's payload did not decode.
    Decode(prost::DecodeError),
    /// A reply carried a number that is not its request's plus one.
    Numbering { expected: u32, got: Option<u32> },
    /// The device read the request and refused the command.
    Refused(Box<proto::Error>),
    /// The device could not read the frame at all.
    Transport(Box<proto::TransportError>),
    /// The device answered the exchange with an arm that was not asked for.
    UnexpectedArm,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Io(error) => write!(formatter, "the connection failed: {error}"),
            SessionError::Timeout(waited) => {
                write!(formatter, "no reply within {:.3} s", waited.as_secs_f64())
            }
            SessionError::Payload => write!(formatter, "the request did not fit one frame"),
            SessionError::Decode(error) => write!(formatter, "a reply did not decode: {error}"),
            SessionError::Numbering { expected, got } => match got {
                Some(got) => write!(
                    formatter,
                    "a reply is numbered {got}, and the request it answers is {expected}"
                ),
                None => write!(formatter, "a reply carries no number; {expected} was due"),
            },
            SessionError::Refused(error) => write!(
                formatter,
                "the device refused the command: {} ({})",
                code_name(error.code),
                reason_name(error.reason)
            ),
            SessionError::Transport(fault) => write!(
                formatter,
                "the device could not read the frame: {} (param1 {})",
                reason_name(fault.reason),
                fault.param1
            ),
            SessionError::UnexpectedArm => {
                write!(formatter, "the device answered with another arm")
            }
        }
    }
}

impl std::error::Error for SessionError {}

impl From<io::Error> for SessionError {
    fn from(error: io::Error) -> Self {
        SessionError::Io(error)
    }
}

impl From<prost::DecodeError> for SessionError {
    fn from(error: prost::DecodeError) -> Self {
        SessionError::Decode(error)
    }
}

fn code_name(code: Option<i32>) -> String {
    match code {
        Some(code) => proto::ErrorCode::try_from(code)
            .map(|code| code.as_str_name().to_string())
            .unwrap_or_else(|_| code.to_string()),
        None => "no code".to_string(),
    }
}

fn reason_name(reason: i32) -> String {
    proto::Reason::try_from(reason)
        .map(|reason| reason.as_str_name().to_string())
        .unwrap_or_else(|_| reason.to_string())
}

/// One connection, one exchange at a time.
pub struct Session<T: Transport> {
    transport: T,
    sequence: Sequence,
    decoder: wire::Decoder,
    timeout: Duration,
    started: Instant,
}

impl<T: Transport> Session<T> {
    /// A session over `transport` that waits at most `timeout` for a reply.
    pub fn new(transport: T, timeout: Duration) -> Self {
        Session {
            transport,
            sequence: Sequence::new(),
            decoder: wire::Decoder::new(),
            timeout,
            started: Instant::now(),
        }
    }

    /// The connection, for an implementation that has to reach it directly.
    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    /// The numbers written so far.
    pub fn sequence(&self) -> &Sequence {
        &self.sequence
    }

    /// Writes one arm and returns the reply that answers it.
    pub fn send(&mut self, kind: proto::request::Kind) -> Result<proto::Reply, SessionError> {
        let number = self.sequence.take();
        let request = proto::Request {
            queued: Some(number),
            kind: Some(kind),
        };
        let frame = wire::encode_to_vec(&request.encode_to_vec()).ok_or(SessionError::Payload)?;
        self.transport.write_all(&frame)?;
        self.transport.flush()?;

        // A refusal and a frame the device could not read are failures of the
        // exchange itself: only the arm that was asked for reaches the caller.
        let reply = self.await_reply(number)?;
        match reply.kind {
            Some(proto::reply::Kind::Error(error)) => Err(SessionError::Refused(Box::new(error))),
            Some(proto::reply::Kind::TransportError(fault)) => {
                Err(SessionError::Transport(Box::new(fault)))
            }
            _ => Ok(reply),
        }
    }

    /// Reads until the reply of `number`'s request arrives, or the deadline
    /// passes.
    fn await_reply(&mut self, number: u32) -> Result<proto::Reply, SessionError> {
        let deadline = Instant::now() + self.timeout;
        let expected = number.wrapping_add(1);
        let mut misnumbered = None;
        let mut buffer = [0u8; 256];
        loop {
            while let Some(frame) = self.decoder.next_frame() {
                let reply = proto::Reply::decode(frame.as_slice())?;

                // A frame the device could not read answers no command and
                // carries no number of its own, so it is handed up as it is:
                // the caller decides whether to ask again.
                if matches!(reply.kind, Some(proto::reply::Kind::TransportError(_))) {
                    return Ok(reply);
                }

                // A frame of another exchange -- a reply that arrived after its
                // own request was given up on -- is not this one's answer. It is
                // read past and remembered, and named only if nothing else comes.
                if reply.queued != Some(expected) {
                    misnumbered = Some(reply.queued);
                    continue;
                }

                self.sequence.after_reply(expected);
                return Ok(reply);
            }

            let now = Instant::now();
            if now >= deadline {
                return match misnumbered {
                    Some(got) => Err(SessionError::Numbering { expected, got }),
                    None => Err(SessionError::Timeout(self.timeout)),
                };
            }
            let read = self.transport.read(&mut buffer, deadline - now)?;
            if read > 0 {
                let now_ms = self.started.elapsed().as_millis() as u64;
                self.decoder.feed(&buffer[..read], now_ms);
            }
        }
    }

    /// The device is there and answering.
    pub fn ping(&mut self) -> Result<(), SessionError> {
        let reply = self.send(proto::request::Kind::SysPing(proto::SysPing {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::SysPing(_) => Some(()),
            _ => None,
        })
    }

    /// What the firmware is: board, version and build.
    pub fn fw_version(&mut self) -> Result<proto::SysGetFwVersionOk, SessionError> {
        let reply = self.send(proto::request::Kind::SysGetFwVersion(
            proto::SysGetFwVersion {},
        ))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::SysGetFwVersion(ok) => Some(ok),
            _ => None,
        })
    }

    /// The device's own readings: load, memory, flash, profiles.
    pub fn usage(&mut self) -> Result<proto::SysGetUsageOk, SessionError> {
        let reply = self.send(proto::request::Kind::SysGetUsage(proto::SysGetUsage {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::SysGetUsage(ok) => Some(ok),
            _ => None,
        })
    }

    /// Every configuration key the device carries, with its type and range.
    pub fn list_keys(&mut self) -> Result<proto::ConfigListKeysOk, SessionError> {
        let reply = self.send(proto::request::Kind::ConfigListKeys(
            proto::ConfigListKeys {},
        ))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ConfigListKeys(ok) => Some(ok),
            _ => None,
        })
    }

    /// Asks the device to reset.
    pub fn reset(&mut self) -> Result<(), SessionError> {
        let reply = self.send(proto::request::Kind::SysReset(proto::SysReset {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::SysReset(_) => Some(()),
            _ => None,
        })
    }
}

/// The arm that was asked for out of a reply that carries no failure, or the
/// error of a reply that carries another arm.
fn expect<T>(
    reply: proto::Reply,
    take: impl FnOnce(proto::reply::Kind) -> Option<T>,
) -> Result<T, SessionError> {
    match reply.kind {
        Some(kind) => take(kind).ok_or(SessionError::UnexpectedArm),
        None => Err(SessionError::UnexpectedArm),
    }
}
