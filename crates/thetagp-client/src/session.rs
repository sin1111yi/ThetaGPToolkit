//! The exchange: the numbers the host writes, and holding a reply against the
//! request it answers.

use std::io;
use std::time::{Duration, Instant};

use prost::Message;
use thetagp_proto as proto;
use thetagp_wire as wire;

use crate::keys::{KeySpace, Value};
use crate::transport::Transport;

/// Bytes of a body one piece carries on its way to the store.
///
/// A payload is at most 1024 bytes and a piece travels inside a message of its
/// own, so this leaves room for the offset and the framing around it.
const WRITE_PIECE_BYTES: usize = 1000;

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
    /// A stream ended with fewer bytes than its opening arm declared.
    ShortStream { received: usize, declared: usize },
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
            SessionError::ShortStream { received, declared } => write!(
                formatter,
                "a stream carried {received} of the {declared} bytes it declared"
            ),
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
        let reply = self.send_request(proto::Request {
            queued: None,
            kind: Some(kind),
        })?;

        // A refusal and a frame the device could not read are failures of the
        // exchange itself: only the arm that was asked for reaches the caller.
        match reply.kind {
            Some(proto::reply::Kind::Error(error)) => Err(SessionError::Refused(Box::new(error))),
            Some(proto::reply::Kind::TransportError(fault)) => {
                Err(SessionError::Transport(Box::new(fault)))
            }
            _ => Ok(reply),
        }
    }

    /// Writes a request built elsewhere -- an arm this build has no method for
    /// -- and returns the reply that answers it. The number is the session's, so
    /// a caller does not write one, and the reply is handed back as it arrived,
    /// refusal and all.
    pub fn send_request(
        &mut self,
        mut request: proto::Request,
    ) -> Result<proto::Reply, SessionError> {
        let number = self.sequence.take();
        request.queued = Some(number);
        let frame = wire::encode_to_vec(&request.encode_to_vec()).ok_or(SessionError::Payload)?;
        self.transport.write_all(&frame)?;
        self.transport.flush()?;
        self.await_reply(number)
    }

    /// Reads until the reply of `number`'s request arrives, or the deadline
    /// passes.
    fn await_reply(&mut self, number: u32) -> Result<proto::Reply, SessionError> {
        let deadline = Instant::now() + self.timeout;
        let expected = number.wrapping_add(1);
        let mut misnumbered = None;
        loop {
            let reply = match self.next_reply(deadline) {
                Ok(reply) => reply,
                Err(SessionError::Timeout(_)) => {
                    return match misnumbered {
                        Some(got) => Err(SessionError::Numbering { expected, got }),
                        None => Err(SessionError::Timeout(self.timeout)),
                    };
                }
                Err(error) => return Err(error),
            };

            // A frame the device could not read answers no command and carries
            // no number of its own, so it is handed up as it is: the caller
            // decides whether to ask again.
            if matches!(reply.kind, Some(proto::reply::Kind::TransportError(_))) {
                return Ok(reply);
            }

            // A frame of another exchange -- a reply that arrived after its own
            // request was given up on -- is not this one's answer. It is read
            // past and remembered, and named only if nothing else comes.
            if reply.queued != Some(expected) {
                misnumbered = Some(reply.queued);
                continue;
            }

            self.sequence.after_reply(expected);
            return Ok(reply);
        }
    }

    /// The next frame that arrives, decoded, or the timeout.
    fn next_reply(&mut self, deadline: Instant) -> Result<proto::Reply, SessionError> {
        let mut buffer = [0u8; 256];
        loop {
            if let Some(frame) = self.decoder.next_frame() {
                return Ok(proto::Reply::decode(frame.as_slice())?);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(SessionError::Timeout(self.timeout));
            }
            let read = self.transport.read(&mut buffer, deadline - now)?;
            if read > 0 {
                let now_ms = self.started.elapsed().as_millis() as u64;
                self.decoder.feed(&buffer[..read], now_ms);
            }
        }
    }

    /// Reads frames until one is accepted, or the deadline passes.
    fn read_until(
        &mut self,
        accept: impl Fn(&proto::Reply) -> bool,
    ) -> Result<proto::Reply, SessionError> {
        let deadline = Instant::now() + self.timeout;
        loop {
            let reply = self.next_reply(deadline)?;
            if accept(&reply) {
                return Ok(reply);
            }
        }
    }

    /// Writes one frame and does not wait: for an exchange whose answer belongs
    /// to the whole of it rather than to one frame of the host's.
    fn write_frame(&mut self, kind: proto::request::Kind) -> Result<(), SessionError> {
        let number = self.sequence.take();
        let request = proto::Request {
            queued: Some(number),
            kind: Some(kind),
        };
        let frame = wire::encode_to_vec(&request.encode_to_vec()).ok_or(SessionError::Payload)?;
        self.transport.write_all(&frame)?;
        self.transport.flush()?;
        Ok(())
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
    /// The lines the firmware logged after `since`, and how many it has written.
    pub fn log(&mut self, since: u32) -> Result<proto::SysGetLogOk, SessionError> {
        let reply = self.send(proto::request::Kind::SysGetLog(proto::SysGetLog { since }))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::SysGetLog(ok) => Some(ok),
            _ => None,
        })
    }

    pub fn usage(&mut self) -> Result<proto::SysGetUsageOk, SessionError> {
        let reply = self.send(proto::request::Kind::SysGetUsage(proto::SysGetUsage {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::SysGetUsage(ok) => Some(ok),
            _ => None,
        })
    }

    /// Every key the device declares, in the order it declared them.
    pub fn key_space(&mut self) -> Result<KeySpace, SessionError> {
        let declared = self.list_keys()?;
        Ok(KeySpace::from_entries_and_inputs(
            &declared.keys,
            declared.inputs,
        ))
    }

    /// Writes one configuration value, whichever of the two shapes it has.
    pub fn write_value(&mut self, name: &str, value: &Value) -> Result<(), SessionError> {
        match value {
            Value::Scalar(number) => self.set_key(name, *number),
            Value::Array(elements) => self.set_key_array(name, elements),
        }
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

    /// The record of one task: its period, its measured cycle and its execution.
    pub fn task_info(&mut self, tid: i32) -> Result<proto::SysGetTaskInfoOk, SessionError> {
        let reply = self.send(proto::request::Kind::SysGetTaskInfo(
            proto::SysGetTaskInfo { tid },
        ))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::SysGetTaskInfo(ok) => Some(ok),
            _ => None,
        })
    }

    /// Asks the device to enter its bootloader, where it waits for an image.
    pub fn enter_dfu(&mut self) -> Result<(), SessionError> {
        let reply = self.send(proto::request::Kind::SysEnterDfu(proto::SysEnterDfu {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::SysEnterDfu(_) => Some(()),
            _ => None,
        })
    }

    /// Reads one configuration key's value.
    pub fn get_key(&mut self, key: &str) -> Result<proto::ConfigGetKeyOk, SessionError> {
        let reply = self.send(proto::request::Kind::ConfigGetKey(proto::ConfigGetKey {
            key: key.to_string(),
        }))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ConfigGetKey(ok) => Some(ok),
            _ => None,
        })
    }

    /// Writes one configuration key's value, the value being one number.
    pub fn set_key(&mut self, key: &str, value: i64) -> Result<(), SessionError> {
        self.write_key(proto::ConfigSetKey {
            key: key.to_string(),
            value: Some(proto::config_set_key::Value::Scalar(value)),
        })
    }

    /// Writes one configuration key's value, the value being a run of numbers in
    /// element order.
    pub fn set_key_array(&mut self, key: &str, values: &[i32]) -> Result<(), SessionError> {
        self.write_key(proto::ConfigSetKey {
            key: key.to_string(),
            value: Some(proto::config_set_key::Value::Array(proto::Int32Array {
                elements: values.to_vec(),
            })),
        })
    }

    fn write_key(&mut self, arm: proto::ConfigSetKey) -> Result<(), SessionError> {
        let reply = self.send(proto::request::Kind::ConfigSetKey(arm))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ConfigSetKey(_) => Some(()),
            _ => None,
        })
    }

    /// Writes the store the configuration is held in.
    ///
    /// The reply says whether it reached the flash and, when some keys were left
    /// out, how many: a key the store has no room for is dropped rather than
    /// refusing the whole write.
    pub fn config_save(&mut self) -> Result<proto::ConfigSaveOk, SessionError> {
        let reply = self.send(proto::request::Kind::ConfigSave(proto::ConfigSave {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ConfigSave(ok) => Some(ok),
            _ => None,
        })
    }

    /// Reads the store back into the running configuration.
    pub fn config_load(&mut self) -> Result<proto::ConfigLoadOk, SessionError> {
        let reply = self.send(proto::request::Kind::ConfigLoad(proto::ConfigLoad {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ConfigLoad(ok) => Some(ok),
            _ => None,
        })
    }

    /// Puts the store back to the defaults the key table declares.
    pub fn factory_reset(&mut self) -> Result<proto::ConfigFactoryResetOk, SessionError> {
        let reply = self.send(proto::request::Kind::ConfigFactoryReset(
            proto::ConfigFactoryReset {},
        ))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ConfigFactoryReset(ok) => Some(ok),
            _ => None,
        })
    }

    /// Every profile the store carries, and which of them is active.
    pub fn profiles(&mut self) -> Result<proto::ProfileListOk, SessionError> {
        let reply = self.send(proto::request::Kind::ProfileList(proto::ProfileList {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ProfileList(ok) => Some(ok),
            _ => None,
        })
    }

    /// How the store's flash is accounted for.
    pub fn profile_status(&mut self) -> Result<proto::ProfileStatusOk, SessionError> {
        let reply = self.send(proto::request::Kind::ProfileStatus(proto::ProfileStatus {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ProfileStatus(ok) => Some(ok),
            _ => None,
        })
    }

    /// Makes a profile the one the device runs from.
    pub fn profile_select(&mut self, id: u32) -> Result<proto::ProfileSelectOk, SessionError> {
        let reply = self.send(proto::request::Kind::ProfileSelect(proto::ProfileSelect {
            id: Some(id),
        }))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ProfileSelect(ok) => Some(ok),
            _ => None,
        })
    }

    /// Deletes a profile.
    pub fn profile_delete(&mut self, id: u32) -> Result<proto::ProfileDeleteOk, SessionError> {
        let reply = self.send(proto::request::Kind::ProfileDelete(proto::ProfileDelete {
            id,
        }))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ProfileDelete(ok) => Some(ok),
            _ => None,
        })
    }

    /// Writes the running configuration into the active profile.
    pub fn profile_save(&mut self) -> Result<proto::ProfileSaveOk, SessionError> {
        let reply = self.send(proto::request::Kind::ProfileSave(proto::ProfileSave {}))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ProfileSave(ok) => Some(ok),
            _ => None,
        })
    }

    /// Reads a profile's body into the running configuration.
    pub fn profile_load(&mut self, id: u32) -> Result<proto::ProfileLoadOk, SessionError> {
        let reply = self.send(proto::request::Kind::ProfileLoad(proto::ProfileLoad {
            id: Some(id),
        }))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::ProfileLoad(ok) => Some(ok),
            _ => None,
        })
    }

    /// A profile's body, read whole.
    ///
    /// The opening arm answers the request; the pieces and the end that closes
    /// the stream answer the exchange and carry the zero the schema reserves for
    /// that, so they are read until the end arm arrives.
    pub fn profile_get(&mut self, id: u32) -> Result<Vec<u8>, SessionError> {
        let reply = self.send(proto::request::Kind::ProfileGet(proto::ProfileGet {
            id: Some(id),
        }))?;
        let start = expect(reply, |kind| match kind {
            proto::reply::Kind::ProfileGet(start) => Some(start),
            _ => None,
        })?;

        let declared = start.total as usize;
        let mut body = vec![0u8; declared];
        let mut filled = 0usize;
        loop {
            let reply = self.read_until(is_stream_frame)?;
            match reply.kind {
                Some(proto::reply::Kind::ProfileChunk(chunk)) => {
                    let at = chunk.offset as usize;
                    let end = at + chunk.data.len();
                    if end > body.len() {
                        return Err(SessionError::ShortStream {
                            received: filled,
                            declared,
                        });
                    }
                    body[at..end].copy_from_slice(&chunk.data);
                    filled = filled.max(end);
                }
                Some(proto::reply::Kind::ProfileEnd(end)) => {
                    if filled != declared || end.total as usize != declared {
                        return Err(SessionError::ShortStream {
                            received: filled,
                            declared,
                        });
                    }
                    return Ok(body);
                }
                Some(proto::reply::Kind::Error(error)) => {
                    return Err(SessionError::Refused(Box::new(error)));
                }
                Some(proto::reply::Kind::TransportError(fault)) => {
                    return Err(SessionError::Transport(Box::new(fault)));
                }
                _ => return Err(SessionError::UnexpectedArm),
            }
        }
    }

    /// Writes a body to a profile the store already carries; id 0 is the factory
    /// profile. Returns the id the body was written under.
    pub fn profile_put(&mut self, id: u32, body: &[u8]) -> Result<u32, SessionError> {
        let opening = proto::request::Kind::ProfileStart(proto::ProfilePutStart {
            id,
            total: body.len() as u32,
        });
        self.write_body(opening, body, |kind| match kind {
            proto::reply::Kind::ProfileStart(ok) => Some(ok.id),
            _ => None,
        })
    }

    /// Writes a body to a new profile. Returns the id the store assigned.
    pub fn profile_create(&mut self, body: &[u8]) -> Result<u32, SessionError> {
        let opening = proto::request::Kind::ProfileCreate(proto::ProfileCreate {
            total: body.len() as u32,
        });
        self.write_body(opening, body, |kind| match kind {
            proto::reply::Kind::ProfileCreate(ok) => Some(ok.id),
            _ => None,
        })
    }

    /// Writes a body through the stream the opening arm begins.
    ///
    /// The opening frame is not answered on success: the answer belongs to the
    /// exchange the whole body makes up, so the pieces and the end follow it on
    /// the wire with no reply between them. A frame the device refuses is
    /// answered at once, and the first answer read is the outcome -- the rest of
    /// a refused body is refused too, and those frames carry numbers behind the
    /// session's own, so the next exchange reads past them.
    fn write_body(
        &mut self,
        opening: proto::request::Kind,
        body: &[u8],
        id_of: impl Fn(proto::reply::Kind) -> Option<u32>,
    ) -> Result<u32, SessionError> {
        self.write_frame(opening)?;

        let mut sent = 0usize;
        while sent < body.len() {
            let end = (sent + WRITE_PIECE_BYTES).min(body.len());
            self.write_frame(proto::request::Kind::ProfilePutChunk(
                proto::ProfilePutChunk {
                    offset: sent as u32,
                    data: body[sent..end].to_vec(),
                },
            ))?;
            sent = end;
        }
        self.write_frame(proto::request::Kind::ProfilePutEnd(proto::ProfilePutEnd {}))?;

        let reply = self.read_until(|reply| {
            is_stream_frame(reply) || matches!(reply.kind, Some(proto::reply::Kind::Error(_)))
        })?;
        match reply.kind {
            Some(proto::reply::Kind::Error(error)) => Err(SessionError::Refused(Box::new(error))),
            Some(proto::reply::Kind::TransportError(fault)) => {
                Err(SessionError::Transport(Box::new(fault)))
            }
            Some(kind) => id_of(kind).ok_or(SessionError::UnexpectedArm),
            None => Err(SessionError::UnexpectedArm),
        }
    }

    /// What the flash chip is: how large, how it is programmed, how it is erased.
    pub fn flash_info(&mut self) -> Result<proto::TestFlashInfoOk, SessionError> {
        let reply = self.send(proto::request::Kind::TestFlashInfo(
            proto::TestFlashInfo::default(),
        ))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::TestFlashInfo(ok) => Some(ok),
            _ => None,
        })
    }

    /// What the keypad driver is doing: the scan rate, the mat it is wired with,
    /// and the scan and commit timings it has seen.
    pub fn keypad_scan(&mut self) -> Result<proto::TestKeypadScanOk, SessionError> {
        let reply = self.send(proto::request::Kind::TestKeypadScan(
            proto::TestKeypadScan::default(),
        ))?;
        expect(reply, |kind| match kind {
            proto::reply::Kind::TestKeypadScan(ok) => Some(ok),
            _ => None,
        })
    }
}

/// Whether a reply is a frame of a stream: the frames that carry a body answer
/// the exchange and not one frame of the host's, so they carry the zero the
/// schema reserves for that.
fn is_stream_frame(reply: &proto::Reply) -> bool {
    reply.queued == Some(0)
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
