//! A device in the same process.
//!
//! It is a [`Transport`] that answers the frames it is handed, so a session can
//! be exercised with no board on the desk and a consumer can be tried offline.
//! What it answers is a function the caller supplies; [`common`] is the answer
//! to the arms a device always carries, and refuses the rest the way the
//! firmware refuses them.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use prost::Message;
use thetagp_proto as proto;
use thetagp_wire as wire;

use crate::transport::Transport;

/// The reply that answers `request`, numbered as the firmware numbers one.
pub fn numbered(request: &proto::Request, kind: proto::reply::Kind) -> proto::Reply {
    proto::Reply {
        queued: request.queued.map(|number| number.wrapping_add(1)),
        kind: Some(kind),
    }
}

/// A device that answers in the process.
pub struct SimulatedDevice {
    inbound: wire::Decoder,
    outbound: VecDeque<u8>,
    answer: Box<dyn Fn(proto::Request) -> Option<proto::Reply> + Send>,
}

impl SimulatedDevice {
    /// A device whose answer to a request is `answer`. An answer of `None`
    /// leaves the request unanswered, which is how a request that is never
    /// answered is exercised.
    pub fn new<F>(answer: F) -> Self
    where
        F: Fn(proto::Request) -> Option<proto::Reply> + Send + 'static,
    {
        SimulatedDevice {
            inbound: wire::Decoder::new(),
            outbound: VecDeque::new(),
            answer: Box::new(answer),
        }
    }

    /// A device that answers the common arms and refuses the rest.
    pub fn common() -> Self {
        SimulatedDevice::new(common)
    }

    fn answer(&mut self, request: &proto::Request) -> io::Result<()> {
        let Some(reply) = (self.answer)(request.clone()) else {
            return Ok(());
        };
        let frame = wire::encode_to_vec(&reply.encode_to_vec()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "the reply is above one frame")
        })?;
        self.outbound.extend(frame);
        Ok(())
    }
}

impl Transport for SimulatedDevice {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.inbound.feed(bytes, 0);
        while let Some(frame) = self.inbound.next_frame() {
            let request = proto::Request::decode(frame.as_slice())
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            self.answer(&request)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], _timeout: Duration) -> io::Result<usize> {
        let read = buf.len().min(self.outbound.len());
        for slot in &mut buf[..read] {
            *slot = self.outbound.pop_front().expect("the count was taken");
        }
        Ok(read)
    }
}

/// The answer to the arms a device always carries; anything else is refused with
/// `ERR_UNKNOWN_CMD`, the way an arm no handler answers is answered.
pub fn common(request: proto::Request) -> Option<proto::Reply> {
    let kind = match request.kind.as_ref()? {
        proto::request::Kind::SysPing(_) => proto::reply::Kind::SysPing(proto::SysPingOk {}),
        proto::request::Kind::SysReset(_) => proto::reply::Kind::SysReset(proto::SysResetOk {}),
        proto::request::Kind::SysGetFwVersion(_) => {
            proto::reply::Kind::SysGetFwVersion(proto::SysGetFwVersionOk {
                board: "BoringTechH743".to_string(),
                version: "0.2.0".to_string(),
                build_date: "Oct 10 2026".to_string(),
                build_time: "12:00:00".to_string(),
            })
        }
        proto::request::Kind::SysGetUsage(_) => {
            proto::reply::Kind::SysGetUsage(proto::SysGetUsageOk {
                cpu_load_percent: 7,
                task_count: 9,
                mcu_flash_used_bytes: 173_236,
                mcu_flash_total_bytes: 2 * 1024 * 1024,
                ram_used_bytes: 46_208,
                ram_total_bytes: 512 * 1024,
                ram_reserved_bytes: 16_384,
                ext_flash_total_sectors: 4096,
                ext_flash_used_sectors: 3,
                ext_flash_free_sectors: 4090,
                ext_flash_reserved_sectors: 3,
                profile_count: 1,
                region_count: 0,
                regions: Vec::new(),
            })
        }
        proto::request::Kind::ConfigListKeys(_) => {
            proto::reply::Kind::ConfigListKeys(proto::ConfigListKeysOk {
                count: 2,
                keys: vec![
                    proto::ConfigKeyEntry {
                        key: "led.bri".to_string(),
                        min: 0,
                        max: 255,
                        ..Default::default()
                    },
                    proto::ConfigKeyEntry {
                        key: "usb.input_mode".to_string(),
                        min: 0,
                        max: 255,
                        ..Default::default()
                    },
                ],
            })
        }
        _ => proto::reply::Kind::Error(proto::Error {
            code: Some(proto::ErrorCode::ErrUnknownCmd as i32),
            reason: proto::Reason::UnknownCommand as i32,
            param1: 0,
            param2: 0,
        }),
    };
    Some(numbered(&request, kind))
}
