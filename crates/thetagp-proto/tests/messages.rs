//! What one side sets, the other reads: every message goes through the frame
//! layer and comes back as it left, so the generated types and the wire are
//! held against each other.

use prost::Message;
use thetagp_proto as proto;
use thetagp_wire as wire;

/// Encodes a message, frames it, feeds the frame to a fresh receive side and
/// decodes what comes out.
fn through_the_wire<M>(message: &M) -> M
where
    M: Message + Default + PartialEq + std::fmt::Debug,
{
    let payload = message.encode_to_vec();
    let frame = wire::encode_to_vec(&payload).expect("the message fits one frame");

    let mut decoder = wire::Decoder::new();
    decoder.feed(&frame, 0);
    let read = decoder.next_frame().expect("the frame came back");

    M::decode(read.as_slice()).expect("the payload decoded")
}

#[test]
fn a_request_reaches_the_other_side_as_it_left() {
    let request = proto::Request {
        queued: Some(7),
        kind: Some(proto::request::Kind::SysPing(proto::SysPing {})),
    };
    assert_eq!(through_the_wire(&request), request);
}

#[test]
fn a_request_that_names_a_key_carries_the_name() {
    let request = proto::Request {
        queued: Some(8),
        kind: Some(proto::request::Kind::ConfigGetKey(proto::ConfigGetKey {
            key: "led.rgb.hz".to_string(),
        })),
    };

    match through_the_wire(&request).kind {
        Some(proto::request::Kind::ConfigGetKey(arm)) => assert_eq!(arm.key, "led.rgb.hz"),
        other => panic!("read {other:?}"),
    }
}

#[test]
fn a_failure_reply_carries_its_code_and_its_reason() {
    let reply = proto::Reply {
        queued: Some(9),
        kind: Some(proto::reply::Kind::Error(proto::Error {
            code: Some(proto::ErrorCode::ErrUnknownCmd as i32),
            reason: proto::Reason::UnknownCommand as i32,
            param1: 0,
            param2: 0,
        })),
    };

    match through_the_wire(&reply).kind {
        Some(proto::reply::Kind::Error(error)) => {
            assert_eq!(error.code, Some(proto::ErrorCode::ErrUnknownCmd as i32));
            assert_eq!(error.reason, proto::Reason::UnknownCommand as i32);
        }
        other => panic!("read {other:?}"),
    }
}

#[test]
fn a_listing_reply_carries_one_entry_per_key() {
    let reply = proto::Reply {
        queued: Some(10),
        kind: Some(proto::reply::Kind::ConfigListKeys(
            proto::ConfigListKeysOk {
                count: 2,
                inputs: 2,
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
            },
        )),
    };

    match through_the_wire(&reply).kind {
        Some(proto::reply::Kind::ConfigListKeys(list)) => {
            assert_eq!(list.count, 2);
            assert_eq!(list.keys.len(), 2);
            assert_eq!(list.keys[1].key, "usb.input_mode");
        }
        other => panic!("read {other:?}"),
    }
}
