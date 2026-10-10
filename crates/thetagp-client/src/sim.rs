//! A device in the same process.
//!
//! It is a [`Transport`] that answers the frames it is handed, so a session can
//! be exercised with no board on the desk and a consumer can be tried offline.
//! What it answers is a function the caller supplies; [`common`] is the answer
//! to the arms a device always carries, and refuses the rest the way the
//! firmware refuses them.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
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
    answer: Box<dyn Fn(proto::Request) -> Vec<proto::Reply> + Send>,
}

impl SimulatedDevice {
    /// A device whose answer to a request is the replies `answer` returns. An
    /// empty answer leaves the request unanswered, which is how a request that
    /// is never answered is exercised; more than one reply is how a stream --
    /// a flash read, a profile body -- arrives.
    pub fn new<F>(answer: F) -> Self
    where
        F: Fn(proto::Request) -> Vec<proto::Reply> + Send + 'static,
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
        for reply in (self.answer)(request.clone()) {
            let frame = wire::encode_to_vec(&reply.encode_to_vec()).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "the reply is above one frame")
            })?;
            self.outbound.extend(frame);
        }
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
pub fn common(request: proto::Request) -> Vec<proto::Reply> {
    let Some(asked) = request.kind.as_ref() else {
        return Vec::new();
    };
    let kind = match asked {
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
    vec![numbered(&request, kind)]
}

/// A device whose profile store is in memory.
///
/// It answers the profile domain the way the firmware does: a staged write is
/// answered once, when the body has been written, and that answer carries the
/// zero the schema reserves for an answer that belongs to the exchange rather
/// than to one frame; a read answers with its opening arm and then the pieces,
/// which carry the same zero. The common arms are answered beside those.
///
/// It is what a consumer is tried against when no board is on the desk, and
/// what the shape of the exchange is pinned by.
pub fn profiles() -> SimulatedDevice {
    let store = RefCell::new(ProfileStore::default());

    SimulatedDevice::new(move |request| {
        let number = request.queued.unwrap_or(0).wrapping_add(1);
        let answer = |kind| {
            vec![proto::Reply {
                queued: Some(number),
                kind: Some(kind),
            }]
        };
        let exchange = |kind| {
            vec![proto::Reply {
                queued: Some(0),
                kind: Some(kind),
            }]
        };
        let refused = |code: proto::ErrorCode, reason: proto::Reason| {
            answer(proto::reply::Kind::Error(proto::Error {
                code: Some(code as i32),
                reason: reason as i32,
                param1: 0,
                param2: 0,
            }))
        };

        let Some(kind) = request.kind.as_ref() else {
            return Vec::new();
        };

        match kind {
            // ── a body on its way to the store ──
            proto::request::Kind::ProfileStart(arm) => {
                store.borrow_mut().begin(Some(arm.id), arm.total as usize);
                Vec::new()
            }
            proto::request::Kind::ProfileCreate(arm) => {
                store.borrow_mut().begin(None, arm.total as usize);
                Vec::new()
            }
            proto::request::Kind::ProfilePutChunk(chunk) => {
                match store.borrow_mut().piece(chunk.offset as usize, &chunk.data) {
                    Ok(()) => Vec::new(),
                    Err(()) => refused(
                        proto::ErrorCode::ErrInvalidParam,
                        proto::Reason::ValueOutOfRange,
                    ),
                }
            }
            proto::request::Kind::ProfilePutEnd(_) => match store.borrow_mut().finish() {
                Some((id, len, false)) => {
                    exchange(proto::reply::Kind::ProfileStart(proto::ProfileStartOk {
                        id,
                        len,
                    }))
                }
                Some((id, len, true)) => {
                    exchange(proto::reply::Kind::ProfileCreate(proto::ProfileCreateOk {
                        id,
                        len,
                    }))
                }
                None => refused(proto::ErrorCode::ErrInvalidState, proto::Reason::None),
            },

            // ── a body on its way back to the host ──
            proto::request::Kind::ProfileGet(arm) => {
                let id = arm.id.unwrap_or(0);
                let body = match store.borrow().body(id) {
                    Some(body) => body.to_vec(),
                    None => {
                        return refused(
                            proto::ErrorCode::ErrInvalidParam,
                            proto::Reason::ProfileNotFound,
                        );
                    }
                };
                let mut replies = vec![proto::Reply {
                    queued: Some(number),
                    kind: Some(proto::reply::Kind::ProfileGet(proto::ProfileGetStart {
                        total: body.len() as u32,
                        id,
                    })),
                }];
                let mut at = 0;
                while at < body.len() {
                    let end = (at + 1000).min(body.len());
                    replies.push(proto::Reply {
                        queued: Some(0),
                        kind: Some(proto::reply::Kind::ProfileChunk(proto::ProfileChunk {
                            offset: at as u32,
                            data: body[at..end].to_vec(),
                        })),
                    });
                    at = end;
                }
                replies.push(proto::Reply {
                    queued: Some(0),
                    kind: Some(proto::reply::Kind::ProfileEnd(proto::ProfileEnd {
                        total: body.len() as u32,
                        id,
                    })),
                });
                replies
            }

            proto::request::Kind::ProfileList(_) => {
                let store = store.borrow();
                answer(proto::reply::Kind::ProfileList(proto::ProfileListOk {
                    active: store.active,
                    profiles: store
                        .bodies
                        .keys()
                        .map(|id| proto::ProfileEntry {
                            id: *id,
                            active: *id == store.active,
                        })
                        .collect(),
                }))
            }
            proto::request::Kind::ProfileSelect(arm) => {
                let mut store = store.borrow_mut();
                let id = arm.id.unwrap_or(0);
                store.active = id;
                answer(proto::reply::Kind::ProfileSelect(proto::ProfileSelectOk {
                    id,
                }))
            }
            proto::request::Kind::ProfileDelete(arm) => {
                store.borrow_mut().bodies.remove(&arm.id);
                answer(proto::reply::Kind::ProfileDelete(proto::ProfileDeleteOk {
                    id: arm.id,
                }))
            }

            // ── everything else is the common device ──
            _ => common(request.clone()),
        }
    })
}

/// The store the simulated device keeps: one body per profile, and the one body
/// being staged for it.
#[derive(Default)]
struct ProfileStore {
    bodies: BTreeMap<u32, Vec<u8>>,
    active: u32,
    staging: Option<Staging>,
}

struct Staging {
    /// The profile asked for, or `None` when the store is to assign one.
    id: Option<u32>,
    total: usize,
    bytes: Vec<u8>,
}

impl ProfileStore {
    fn begin(&mut self, id: Option<u32>, total: usize) {
        self.staging = Some(Staging {
            id,
            total,
            bytes: Vec::with_capacity(total),
        });
    }

    fn piece(&mut self, offset: usize, data: &[u8]) -> Result<(), ()> {
        let Some(staging) = self.staging.as_mut() else {
            return Err(());
        };
        if offset != staging.bytes.len() || staging.bytes.len() + data.len() > staging.total {
            return Err(());
        }
        staging.bytes.extend_from_slice(data);
        Ok(())
    }

    /// The body the stream carried, as `(id, length, created)`.
    fn finish(&mut self) -> Option<(u32, u32, bool)> {
        let staging = self.staging.take()?;
        if staging.bytes.len() != staging.total {
            return None;
        }
        let created = staging.id.is_none_or(|id| id == 0);
        let id = staging
            .id
            .unwrap_or_else(|| (1..).find(|id| !self.bodies.contains_key(id)).unwrap());
        let len = staging.bytes.len() as u32;
        self.bodies.insert(id, staging.bytes);
        Some((id, len, created))
    }

    fn body(&self, id: u32) -> Option<&[u8]> {
        self.bodies.get(&id).map(Vec::as_slice)
    }
}
