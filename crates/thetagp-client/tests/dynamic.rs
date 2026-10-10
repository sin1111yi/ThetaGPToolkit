//! The schema read by name: the arms it declares, and an arm built from text.

use thetagp_client::dynamic;
use thetagp_proto as proto;

#[test]
fn the_arms_are_the_schema_own() {
    let arms = dynamic::arms().unwrap();

    let ping = arms.iter().find(|arm| arm.name == "sys.ping").unwrap();
    assert_eq!(ping.number, 1);
    assert_eq!(ping.carries, "SysPing");

    let get = arms
        .iter()
        .find(|arm| arm.name == "config.get_key")
        .unwrap();
    assert_eq!(get.carries, "ConfigGetKey");

    let put = arms
        .iter()
        .find(|arm| arm.name == "profile.put_chunk")
        .unwrap();
    assert_eq!(put.carries, "ProfilePutChunk");
}

#[test]
fn an_arm_is_built_from_the_text_it_was_given() {
    let request =
        dynamic::build_request("config.get_key", &["key=led.rgb.hz".to_string()]).unwrap();

    match request.kind {
        Some(proto::request::Kind::ConfigGetKey(arm)) => assert_eq!(arm.key, "led.rgb.hz"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_dotted_name_walks_into_a_nested_message() {
    let request = dynamic::build_request(
        "config.set_key",
        &[
            "key=map.btn_map".to_string(),
            "value.array.elements=1,2,3,4,5,6,7,8".to_string(),
        ],
    )
    .unwrap();

    match request.kind {
        Some(proto::request::Kind::ConfigSetKey(arm)) => {
            assert_eq!(arm.key, "map.btn_map");
            match arm.value {
                Some(proto::config_set_key::Value::Array(array)) => {
                    assert_eq!(array.elements, vec![1, 2, 3, 4, 5, 6, 7, 8]);
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_enum_is_named_by_its_schema_name() {
    let request = dynamic::build_request("sys.get_task_info", &["tid=2".to_string()]).unwrap();
    match request.kind {
        Some(proto::request::Kind::SysGetTaskInfo(arm)) => assert_eq!(arm.tid, 2),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_arm_the_schema_has_not_is_named() {
    assert!(matches!(
        dynamic::build_request("sys.nothing", &[]),
        Err(dynamic::DynamicError::NoSuchArm(_))
    ));
}

#[test]
fn a_frame_is_the_text_a_report_prints() {
    let bytes = dynamic::parse_hex("03 01 02 03 06 00").unwrap();
    let mut decoder = thetagp_client::wire::Decoder::new();
    decoder.feed(&bytes, 0);
    let payload = decoder.next_frame().unwrap();
    assert_eq!(dynamic::hex_of(&payload), "01 02 03");
    // Not a message this build knows, so it is handed back as its bytes.
    assert_eq!(dynamic::describe(&payload), "01 02 03");
}
