//! The CDC wire, one frame at a time.
//!
//! A frame is a varint length, the payload, and the payload's byte sum in the
//! low 16 bits, little endian. The length counts the payload's bytes and neither
//! the prefix nor the checksum; the checksum covers the payload's bytes and
//! neither the prefix nor itself.
//!
//! The receive side is fed one byte or one buffer at a time and accumulates, so
//! a port's reader feeds it and the caller takes whole frames out. A frame whose
//! checksum does not hold is refused whole, a third length byte is refused where
//! it arrives, and a declared length whose bytes stop arriving is dropped after
//! [`IDLE_TIMEOUT_MS`]. Every refusal is counted by its reason.

use std::collections::VecDeque;

/// Largest payload one frame carries.
pub const PAYLOAD_MAX: usize = 1024;
/// Most bytes one frame takes on the wire: a two-byte length, the payload, the checksum.
pub const FRAME_MAX: usize = 2 + PAYLOAD_MAX + 2;
/// Lengths are 0..=PAYLOAD_MAX, so a prefix is one byte below 128 and two from 128 up.
pub const PREFIX_MAX_BYTES: usize = 2;
/// Bytes the checksum takes, low byte first.
pub const CHECKSUM_BYTES: usize = 2;
/// Idle time a declared length is given to arrive in before the half frame is dropped.
pub const IDLE_TIMEOUT_MS: u64 = 1000;

/// Sum of a payload's bytes in the low 16 bits, wrapping at 16 bits.
pub fn checksum(payload: &[u8]) -> u16 {
    payload
        .iter()
        .fold(0u16, |sum, &byte| sum.wrapping_add(u16::from(byte)))
}

/// Bytes a length takes as a prefix: one below 128, two up to `PAYLOAD_MAX`.
pub fn prefix_size(length: usize) -> usize {
    if length < 0x80 { 1 } else { 2 }
}

/// Writes one frame into `out` and returns its size in bytes, or `None` when the
/// payload is above `PAYLOAD_MAX` or `out` is too short for this frame. A frame
/// of an empty payload is three bytes, so `None` is never a frame.
pub fn encode(payload: &[u8], out: &mut [u8]) -> Option<usize> {
    if payload.len() > PAYLOAD_MAX {
        return None;
    }
    let prefix = prefix_size(payload.len());
    let total = prefix + payload.len() + CHECKSUM_BYTES;
    if out.len() < total {
        return None;
    }

    let mut used = 0;
    if prefix == 1 {
        out[used] = payload.len() as u8;
        used += 1;
    } else {
        out[used] = ((payload.len() & 0x7F) as u8) | 0x80;
        out[used + 1] = ((payload.len() >> 7) & 0x7F) as u8;
        used += 2;
    }
    out[used..used + payload.len()].copy_from_slice(payload);
    used += payload.len();

    let sum = checksum(payload);
    out[used] = sum as u8;
    out[used + 1] = (sum >> 8) as u8;
    Some(total)
}

/// [`encode`] into a buffer of its own.
pub fn encode_to_vec(payload: &[u8]) -> Option<Vec<u8>> {
    let mut out = vec![0u8; prefix_size(payload.len()) + payload.len() + CHECKSUM_BYTES];
    encode(payload, &mut out).map(|used| {
        out.truncate(used);
        out
    })
}

/// Why a frame was refused. The reason names the step that refused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Drop {
    /// A length prefix of more than [`PREFIX_MAX_BYTES`] bytes.
    Prefix = 0,
    /// A length above [`PAYLOAD_MAX`].
    Length = 1,
    /// A checksum that is not the payload's sum.
    Checksum = 2,
    /// A declared length whose bytes stopped arriving.
    Idle = 3,
}

impl Drop {
    /// Every reason, for iterating the counters.
    pub const ALL: [Drop; 4] = [Drop::Prefix, Drop::Length, Drop::Checksum, Drop::Idle];

    /// The reason's name, as a report spells it.
    pub fn label(self) -> &'static str {
        match self {
            Drop::Prefix => "prefix",
            Drop::Length => "length",
            Drop::Checksum => "checksum",
            Drop::Idle => "idle",
        }
    }
}

/// The step the receive side is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Reading a length prefix.
    Prefix,
    /// Reading the payload the prefix declared.
    Payload,
    /// Reading the checksum's two bytes.
    Checksum,
}

/// The receive side: fed bytes, it holds whole frames until they are taken.
pub struct Decoder {
    state: State,
    prefix: [u8; PREFIX_MAX_BYTES],
    prefix_len: usize,
    length: usize,
    payload: [u8; PAYLOAD_MAX],
    used: usize,
    sum: u16,
    checksum: [u8; CHECKSUM_BYTES],
    checksum_len: usize,
    last_byte_ms: u64,
    frames: VecDeque<Vec<u8>>,
    drops: [u64; 4],
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// A receive side reading the length prefix of a frame.
    pub fn new() -> Self {
        Decoder {
            state: State::Prefix,
            prefix: [0; PREFIX_MAX_BYTES],
            prefix_len: 0,
            length: 0,
            payload: [0; PAYLOAD_MAX],
            used: 0,
            sum: 0,
            checksum: [0; CHECKSUM_BYTES],
            checksum_len: 0,
            last_byte_ms: 0,
            frames: VecDeque::new(),
            drops: [0; 4],
        }
    }

    /// Feeds received bytes, stamped with their arrival time for the idle
    /// timeout of a half frame.
    pub fn feed(&mut self, bytes: &[u8], now_ms: u64) {
        for &byte in bytes {
            self.feed_byte(byte, now_ms);
        }
    }

    /// Feeds one received byte, stamped with its arrival time.
    pub fn feed_byte(&mut self, byte: u8, now_ms: u64) {
        self.last_byte_ms = now_ms;

        match self.state {
            State::Prefix => {
                // A third prefix byte would declare a length above PAYLOAD_MAX
                // whatever the byte's own bits say, so it is refused where it
                // arrives rather than decoded and then refused.
                if self.prefix_len == PREFIX_MAX_BYTES {
                    self.drop(Drop::Prefix);
                    return;
                }
                self.prefix[self.prefix_len] = byte;
                self.prefix_len += 1;
                if byte & 0x80 != 0 {
                    return; // the length continues in the byte after this one
                }

                let mut declared = 0usize;
                for i in 0..self.prefix_len {
                    declared |= usize::from(self.prefix[i] & 0x7F) << (7 * i);
                }
                if declared > PAYLOAD_MAX {
                    self.drop(Drop::Length);
                    return;
                }

                self.length = declared;
                self.used = 0;
                self.sum = 0;
                self.checksum_len = 0;
                self.state = if declared == 0 {
                    State::Checksum
                } else {
                    State::Payload
                };
            }
            State::Payload => {
                self.payload[self.used] = byte;
                self.used += 1;
                self.sum = self.sum.wrapping_add(u16::from(byte));
                if self.used == self.length {
                    self.state = State::Checksum;
                }
            }
            State::Checksum => {
                self.checksum[self.checksum_len] = byte;
                self.checksum_len += 1;
                if self.checksum_len < CHECKSUM_BYTES {
                    return;
                }
                if u16::from_le_bytes(self.checksum) != self.sum {
                    // The payload never travels: the frame is refused whole and
                    // the byte after it is read as a new length prefix.
                    self.drop(Drop::Checksum);
                    return;
                }
                let frame = self.payload[..self.length].to_vec();
                self.frames.push_back(frame);
                self.reset();
            }
        }
    }

    /// The caller's tick: drops a declared length whose bytes stopped arriving
    /// and returns whether this tick dropped one.
    ///
    /// Only a length that was declared can be half a frame: a prefix that
    /// promised another prefix byte declares nothing yet and is left where it is.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if self.state == State::Prefix {
            return false;
        }
        if now_ms.wrapping_sub(self.last_byte_ms) < IDLE_TIMEOUT_MS {
            return false;
        }
        self.drop(Drop::Idle);
        true
    }

    /// Takes the oldest whole frame and returns it, or `None` when none is held.
    pub fn next_frame(&mut self) -> Option<Vec<u8>> {
        self.frames.pop_front()
    }

    /// Whole frames waiting to be taken.
    pub fn queued(&self) -> usize {
        self.frames.len()
    }

    /// How many frames the reason has refused.
    pub fn drops(&self, reason: Drop) -> u64 {
        self.drops[reason as usize]
    }

    /// Every refusal, in `Drop::ALL` order.
    pub fn drops_all(&self) -> [u64; 4] {
        self.drops
    }

    fn drop(&mut self, reason: Drop) {
        self.drops[reason as usize] += 1;
        self.reset();
    }

    fn reset(&mut self) {
        self.state = State::Prefix;
        self.prefix_len = 0;
        self.length = 0;
        self.used = 0;
        self.sum = 0;
        self.checksum_len = 0;
    }
}
