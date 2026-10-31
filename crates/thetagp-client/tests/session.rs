//! The exchange, held against a device that answers in the process: the
//! numbering, the arms that come back, and the ways one does not.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use thetagp_client::backup::{Snapshot, StoredProfile};
use thetagp_client::sim::{SimulatedDevice, numbered};
use thetagp_client::{Session, SessionError, proto};

fn session_with<F>(answer: F) -> Session<SimulatedDevice>
where
    F: Fn(proto::Request) -> Vec<proto::Reply> + Send + 'static,
{
    Session::new(SimulatedDevice::new(answer), Duration::from_millis(50))
}

fn session() -> Session<SimulatedDevice> {
    Session::new(SimulatedDevice::common(), Duration::from_millis(50))
}

/// A reply of `kind` that is numbered as another exchange's answer.
fn stale(request: &proto::Request, kind: proto::reply::Kind) -> proto::Reply {
    let mut reply = numbered(request, kind);
    reply.queued = Some(99);
    reply
}

#[test]
fn the_first_frame_is_numbered_zero_and_its_reply_carries_one() {
    let mut session = session();
    session.ping().unwrap();
    // 0 was taken, the reply carried 1, so the next frame takes 2.
    assert_eq!(session.sequence().next(), 2);
}

#[test]
fn a_second_exchange_carries_the_numbering_on() {
    let mut session = session();
    session.ping().unwrap();
    session.ping().unwrap();
    session.ping().unwrap();
    assert_eq!(session.sequence().next(), 6);
}

#[test]
fn the_firmware_version_comes_back() {
    let mut session = session();
    let version = session.fw_version().unwrap();
    assert_eq!(version.board, "BoringTechH743");
    assert_eq!(version.version, "0.2.0");
}

#[test]
fn the_usage_report_comes_back() {
    let mut session = session();
    let usage = session.usage().unwrap();
    assert_eq!(usage.cpu_load_percent, 7);
    assert_eq!(usage.mcu_flash_used_bytes, 173_236);
    assert_eq!(usage.profile_count, 1);
}

#[test]
fn the_key_listing_comes_back_one_entry_per_key() {
    let mut session = session();
    let keys = session.list_keys().unwrap();
    assert_eq!(keys.count, 2);
    assert_eq!(keys.keys.len(), 2);
    assert_eq!(keys.keys[1].key, "usb.input_mode");
}

#[test]
fn the_device_can_be_asked_to_reset() {
    let mut session = session();
    session.reset().unwrap();
}

#[test]
fn a_command_the_device_does_not_answer_is_refused() {
    let mut session = session();
    let error = session
        .send(proto::request::Kind::TestMemInfo(
            proto::TestMemInfo::default(),
        ))
        .unwrap_err();

    match error {
        SessionError::Refused(error) => {
            assert_eq!(error.code, Some(proto::ErrorCode::ErrUnknownCmd as i32));
            assert_eq!(error.reason, proto::Reason::UnknownCommand as i32);
        }
        other => panic!("{other}"),
    }
}

#[test]
fn a_refusal_is_spelled_with_the_schema_names() {
    let mut session = session();
    let spelled = session
        .send(proto::request::Kind::TestMemInfo(
            proto::TestMemInfo::default(),
        ))
        .unwrap_err()
        .to_string();
    assert!(spelled.contains("ERR_UNKNOWN_CMD"), "{spelled}");
    assert!(spelled.contains("UNKNOWN_COMMAND"), "{spelled}");
}

#[test]
fn a_frame_of_another_exchange_is_read_past() {
    let mut session = session_with(|request| {
        // A reply that arrived after its own request was given up on, and then
        // the answer this exchange is waiting for.
        vec![
            stale(&request, proto::reply::Kind::SysPing(proto::SysPingOk {})),
            numbered(&request, proto::reply::Kind::SysPing(proto::SysPingOk {})),
        ]
    });

    session.ping().unwrap();
    assert_eq!(session.sequence().next(), 2);
}

#[test]
fn a_reply_that_is_never_numbered_as_this_exchange_is_named() {
    let mut session = session_with(|request| {
        vec![stale(
            &request,
            proto::reply::Kind::SysPing(proto::SysPingOk {}),
        )]
    });

    match session.ping().unwrap_err() {
        SessionError::Numbering { expected, got } => {
            assert_eq!(expected, 1);
            assert_eq!(got, Some(99));
        }
        other => panic!("{other}"),
    }
}

#[test]
fn a_request_that_is_never_answered_times_out() {
    let mut session = session_with(|_| Vec::new());
    match session.ping().unwrap_err() {
        SessionError::Timeout(_) => {}
        other => panic!("{other}"),
    }
}

#[test]
fn a_frame_the_device_could_not_read_is_handed_up() {
    let mut session = session_with(|_| {
        vec![proto::Reply {
            queued: None,
            kind: Some(proto::reply::Kind::TransportError(proto::TransportError {
                reason: proto::Reason::FrameChecksum as i32,
                param1: 0,
            })),
        }]
    });

    match session.ping().unwrap_err() {
        SessionError::Transport(fault) => {
            assert_eq!(fault.reason, proto::Reason::FrameChecksum as i32);
        }
        other => panic!("{other}"),
    }
    // A frame the device could not read answers no exchange, so the numbering
    // is where it was.
    assert_eq!(session.sequence().next(), 1);
}

fn profiles_session() -> Session<SimulatedDevice> {
    Session::new(thetagp_client::sim::device(), Duration::from_millis(50))
}

/// A body longer than one piece carries, so the stream is more than one frame.
fn a_body(bytes: usize) -> Vec<u8> {
    let mut body = br#"{"ver":2,"map":{"socd":"#.to_vec();
    while body.len() < bytes {
        body.push(b'0' + (body.len() % 10) as u8);
    }
    body.push(b'}');
    body
}

#[test]
fn a_body_longer_than_one_piece_is_written_and_read_back_whole() {
    let mut session = profiles_session();
    let body = a_body(2500);

    let id = session.profile_create(&body).unwrap();
    assert_eq!(id, 1);
    assert_eq!(session.profile_get(id).unwrap(), body);
}

#[test]
fn a_body_written_to_a_named_profile_keeps_its_id() {
    let mut session = profiles_session();
    let body = a_body(64);

    let id = session.profile_put(3, &body).unwrap();
    assert_eq!(id, 3);
    assert_eq!(session.profile_get(3).unwrap(), body);
}

#[test]
fn the_exchange_is_still_numbered_after_a_staged_write() {
    let mut session = profiles_session();
    let body = a_body(1500);

    let id = session.profile_put(2, &body).unwrap();
    // The staged write's answer belongs to the exchange and carries zero, so it
    // does not set the numbering back; the arms after it are numbered on.
    session.ping().unwrap();
    assert_eq!(session.profile_get(id).unwrap(), body);
    assert!(session.sequence().next() > 5);
}

#[test]
fn a_profile_can_be_listed_selected_and_deleted() {
    let mut session = profiles_session();
    let first = session.profile_put(1, &a_body(32)).unwrap();
    let second = session.profile_put(2, &a_body(32)).unwrap();

    let listed = session.profiles().unwrap();
    // The store carries its factory profile 0 besides the two written here.
    assert_eq!(listed.profiles.len(), 3);

    let selected = session.profile_select(second).unwrap();
    assert_eq!(selected.id, second);

    session.profile_delete(first).unwrap();
    let listed = session.profiles().unwrap();
    assert_eq!(listed.profiles.len(), 2);
    let active = listed.profiles.iter().find(|entry| entry.active).unwrap();
    assert_eq!(active.id, second);
}

#[test]
fn reading_a_profile_the_store_does_not_carry_is_refused() {
    let mut session = profiles_session();
    match session.profile_get(9).unwrap_err() {
        SessionError::Refused(error) => {
            assert_eq!(error.reason, proto::Reason::ProfileNotFound as i32)
        }
        other => panic!("{other}"),
    }
}

#[test]
fn a_body_the_store_refuses_is_reported() {
    // A device that refuses the opening arm of a staged write: the refusal is
    // answered at once, and it is what the exchange ends as.
    let mut session = session_with(|request| {
        let number = request.queued.unwrap_or(0).wrapping_add(1);
        vec![proto::Reply {
            queued: Some(number),
            kind: Some(proto::reply::Kind::Error(proto::Error {
                code: Some(proto::ErrorCode::ErrInvalidParam as i32),
                reason: proto::Reason::ValueOutOfRange as i32,
                param1: 1,
                param2: 4096,
            })),
        }]
    });

    match session.profile_create(&a_body(16)).unwrap_err() {
        SessionError::Refused(error) => {
            assert_eq!(error.reason, proto::Reason::ValueOutOfRange as i32);
            assert_eq!(error.param2, 4096);
        }
        other => panic!("{other}"),
    }
}

#[test]
fn a_snapshot_does_not_write_the_factory_profile() {
    let mut session = profiles_session();
    let body = a_body(64);
    let id = session.profile_put(1, &body).unwrap();

    let snapshot = Snapshot {
        board: "test".to_string(),
        version: "1".to_string(),
        config: Default::default(),
        profiles: vec![
            StoredProfile {
                id: 0,
                body: "{}".to_string(),
            },
            StoredProfile {
                id,
                body: String::from_utf8(body).unwrap(),
            },
        ],
    };

    let restored = snapshot.restore(&mut session).expect("the snapshot restores");
    assert_eq!(restored.profiles_written, 1, "the user's profile is written");
    assert!(
        restored.profiles_refused.is_empty(),
        "the factory profile is skipped, not refused: {:?}",
        restored.profiles_refused
    );
}

// ── the flash, read and programmed raw ──

/// A two-sector image, and the answers a board gives to the arms that reach the
/// chip directly. `write_ok` and `erase_ok` are what the chip says: a refusal is
/// a value of the arm rather than an error, so both arrive as `false`.
fn flash_device(
    image: Arc<Mutex<Vec<u8>>>,
    write_ok: bool,
    erase_ok: bool,
) -> impl Fn(proto::Request) -> Vec<proto::Reply> + Send + 'static {
    move |request| {
        let number = request.queued.map(|taken| taken.wrapping_add(1));
        let Some(asked) = request.kind else {
            return Vec::new();
        };
        match asked {
            proto::request::Kind::TestFlashRead(asked) => {
                let image = image.lock().expect("the image is not poisoned");
                let at = asked.addr as usize;
                let len = asked.len as usize;
                assert!(at + len <= image.len(), "the run is inside the image");
                let run = image[at..at + len].to_vec();
                let mut replies = vec![proto::Reply {
                    queued: number,
                    kind: Some(proto::reply::Kind::TestFlashRead(
                        proto::TestFlashReadStart {
                            total: len as u32,
                            addr: asked.addr,
                        },
                    )),
                }];
                let mut piece = 0;
                while piece < len {
                    let end = (piece + 1000).min(len);
                    replies.push(proto::Reply {
                        queued: Some(0),
                        kind: Some(proto::reply::Kind::TestFlashChunk(proto::TestFlashChunk {
                            offset: piece as u32,
                            data: run[piece..end].to_vec(),
                        })),
                    });
                    piece = end;
                }
                replies.push(proto::Reply {
                    queued: Some(0),
                    kind: Some(proto::reply::Kind::TestFlashEnd(proto::TestFlashEnd {
                        total: len as u32,
                        addr: asked.addr,
                    })),
                });
                replies
            }
            proto::request::Kind::TestFlashWrite(asked) => {
                if write_ok {
                    let mut image = image.lock().expect("the image is not poisoned");
                    for (slot, byte) in image[asked.addr as usize..].iter_mut().zip(&asked.data) {
                        *slot &= *byte;
                    }
                }
                vec![proto::Reply {
                    queued: number,
                    kind: Some(proto::reply::Kind::TestFlashWrite(
                        proto::TestFlashWriteOk {
                            addr: asked.addr,
                            len: asked.len,
                            ok: write_ok,
                        },
                    )),
                }]
            }
            proto::request::Kind::TestEraseSector(asked) => {
                if erase_ok {
                    let mut image = image.lock().expect("the image is not poisoned");
                    let sector = asked.addr as usize & !4095;
                    image[sector..sector + 4096].fill(0xFF);
                }
                vec![proto::Reply {
                    queued: number,
                    kind: Some(proto::reply::Kind::TestEraseSector(
                        proto::TestEraseSectorOk {
                            addr: asked.addr,
                            ok: erase_ok,
                        },
                    )),
                }]
            }
            _ => Vec::new(),
        }
    }
}

/// Two sectors, the second holding a pattern of its own.
fn two_sectors() -> (Arc<Mutex<Vec<u8>>>, Vec<u8>) {
    let pattern: Vec<u8> = (0..4096u32)
        .map(|i| ((i * 73 + 0x5a) & 0xff) as u8)
        .collect();
    let mut image = vec![0xFFu8; 8192];
    image[4096..].copy_from_slice(&pattern);
    (Arc::new(Mutex::new(image)), pattern)
}

#[test]
fn a_flash_run_comes_back_whole() {
    let (image, pattern) = two_sectors();
    let mut session = session_with(flash_device(image, true, true));
    let bytes = session.flash_read(0x1000, 4096).unwrap();
    assert_eq!(bytes, pattern);
    // The pieces answer the exchange, not the request: the numbering stands.
    assert_eq!(session.sequence().next(), 2);
}

#[test]
fn a_sector_is_erased_and_programmed_through_the_raw_arms() {
    let (image, pattern) = two_sectors();
    let mut session = session_with(flash_device(image.clone(), true, true));
    assert!(session.erase_sector(0x1000).unwrap().ok);
    let blank = session.flash_read(0x1000, 4096).unwrap();
    assert!(
        blank.iter().all(|byte| *byte == 0xFF),
        "the sector is erased"
    );
    let wrote = session.flash_write(0x1000, &pattern[..256]).unwrap();
    assert!(wrote.ok);
    assert_eq!(wrote.len, 256);
    assert_eq!(session.flash_read(0x1000, 256).unwrap(), &pattern[..256]);
    assert_eq!(&image.lock().unwrap()[4096..4352], &pattern[..256]);
}

#[test]
fn a_chip_that_refuses_says_so_rather_than_failing_the_exchange() {
    let (image, _) = two_sectors();
    let mut session = session_with(flash_device(image, false, false));
    assert!(!session.erase_sector(0x1000).unwrap().ok);
    assert!(!session.flash_write(0x1000, &[0x00; 4]).unwrap().ok);
}

#[test]
fn a_flash_run_that_stops_short_is_refused() {
    let mut session = session_with(|request| {
        let number = request.queued.map(|taken| taken.wrapping_add(1));
        vec![
            proto::Reply {
                queued: number,
                kind: Some(proto::reply::Kind::TestFlashRead(
                    proto::TestFlashReadStart { total: 20, addr: 0 },
                )),
            },
            proto::Reply {
                queued: Some(0),
                kind: Some(proto::reply::Kind::TestFlashChunk(proto::TestFlashChunk {
                    offset: 0,
                    data: vec![0u8; 10],
                })),
            },
            proto::Reply {
                queued: Some(0),
                kind: Some(proto::reply::Kind::TestFlashEnd(proto::TestFlashEnd {
                    total: 20,
                    addr: 0,
                })),
            },
        ]
    });
    match session.flash_read(0, 20).unwrap_err() {
        SessionError::ShortStream { received, declared } => {
            assert_eq!((received, declared), (10, 20));
        }
        other => panic!("{other}"),
    }
}
