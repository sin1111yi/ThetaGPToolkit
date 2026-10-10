//! The exchange, held against a device that answers in the process: the
//! numbering, the arms that come back, and the ways one does not.

use std::time::Duration;

use thetagp_client::sim::{SimulatedDevice, numbered};
use thetagp_client::{Session, SessionError, proto};

fn session_with<F>(answer: F) -> Session<SimulatedDevice>
where
    F: Fn(proto::Request) -> Option<proto::Reply> + Send + 'static,
{
    Session::new(SimulatedDevice::new(answer), Duration::from_millis(50))
}

fn session() -> Session<SimulatedDevice> {
    Session::new(SimulatedDevice::common(), Duration::from_millis(50))
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
    // The refusal is spelled with the schema's own names.
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
fn a_reply_numbered_wrongly_is_an_error() {
    let mut session = session_with(|request| {
        let mut reply = numbered(&request, proto::reply::Kind::SysPing(proto::SysPingOk {}));
        reply.queued = Some(99);
        Some(reply)
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
    let mut session = session_with(|_| None);
    match session.ping().unwrap_err() {
        SessionError::Timeout(_) => {}
        other => panic!("{other}"),
    }
}

#[test]
fn a_frame_the_device_could_not_read_is_handed_up() {
    let mut session = session_with(|_| {
        Some(proto::Reply {
            queued: None,
            kind: Some(proto::reply::Kind::TransportError(proto::TransportError {
                reason: proto::Reason::FrameChecksum as i32,
                param1: 0,
            })),
        })
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
