//! The exchange, held against a device that answers in the process: the
//! numbering, the arms that come back, and the ways one does not.

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
    assert_eq!(listed.profiles.len(), 2);

    let selected = session.profile_select(second).unwrap();
    assert_eq!(selected.id, second);

    session.profile_delete(first).unwrap();
    let listed = session.profiles().unwrap();
    assert_eq!(listed.profiles.len(), 1);
    assert_eq!(listed.profiles[0].id, second);
    assert!(listed.profiles[0].active);
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
