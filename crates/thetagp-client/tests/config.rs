//! The device model: the key space the device declares, the configuration held
//! to it, and a snapshot of the whole of it.

use std::time::Duration;

use thetagp_client::{Config, KeyType, Reject, Session, Snapshot, Value, sim};

fn session() -> Session<sim::SimulatedDevice> {
    Session::new(sim::device(), Duration::from_millis(50))
}

#[test]
fn the_key_space_is_the_device_own_declaration() {
    let mut session = session();
    let space = session.key_space().unwrap();

    assert_eq!(space.len(), 5);
    assert_eq!(space.domains(), vec!["map", "led", "led.rgb", "usb"]);
    assert_eq!(space.domain("map").len(), 2);

    let key = space.get("led.rgb.hz").unwrap();
    assert_eq!(key.domain, "led.rgb");
    assert_eq!(key.leaf, "hz");
    assert_eq!(key.kind, KeyType::U8);
    assert_eq!((key.min, key.max), (1, 100));
    assert!(!key.reboot);

    assert!(space.get("usb.input_mode").unwrap().reboot);
    assert_eq!(space.get("map.btn_map").unwrap().count, 8);
}

#[test]
fn the_configuration_is_read_one_key_at_a_time() {
    let mut session = session();
    let config = Config::read(&mut session).unwrap();

    assert_eq!(config.values().len(), 5);
    assert_eq!(config.get("led.bri"), Some(&Value::Scalar(128)));
    assert_eq!(
        config.get("map.btn_map"),
        Some(&Value::Array(vec![1, 2, 3, 4, 5, 6, 7, 8]))
    );
}

#[test]
fn a_value_out_of_range_is_refused_before_it_travels() {
    let mut session = session();
    let mut config = Config::read(&mut session).unwrap();

    match config.set("led.bri", Value::Scalar(999)) {
        Err(Reject::OutOfRange { min, max, got, .. }) => {
            assert_eq!((min, max, got), (0, 255, 999));
        }
        other => panic!("{other:?}"),
    }

    // Nothing was written: the device still holds what it held.
    assert_eq!(
        Config::read(&mut session).unwrap().get("led.bri"),
        Some(&Value::Scalar(128))
    );
}

#[test]
fn a_key_the_device_does_not_carry_is_refused_before_it_travels() {
    let mut session = session();
    let mut config = Config::read(&mut session).unwrap();
    assert!(matches!(
        config.set("led.brightness", Value::Scalar(1)),
        Err(Reject::UnknownKey(_))
    ));
}

#[test]
fn a_value_of_the_wrong_shape_or_length_is_refused() {
    let mut session = session();
    let mut config = Config::read(&mut session).unwrap();

    assert!(matches!(
        config.set("map.btn_map", Value::Array(vec![1, 2, 3])),
        Err(Reject::ElementCount {
            expected: 8,
            got: 3,
            ..
        })
    ));
    assert!(matches!(
        config.set("map.btn_map", Value::Scalar(1)),
        Err(Reject::NotAnArray(_))
    ));
    assert!(matches!(
        config.set("led.bri", Value::Array(vec![1])),
        Err(Reject::NotAScalar(_))
    ));
    assert!(matches!(
        config.set("map.btn_map", Value::Array(vec![1, 2, 3, 4, 5, 6, 7, 99])),
        Err(Reject::ElementOutOfRange { at: 7, got: 99, .. })
    ));
}

#[test]
fn only_what_changed_is_written() {
    let mut session = session();
    let was = Config::read(&mut session).unwrap();

    let mut wanted = was.clone();
    wanted.set("led.bri", Value::Scalar(200)).unwrap();
    wanted.set("map.socd", Value::Scalar(2)).unwrap();

    let written = wanted.apply(&mut session, &was).unwrap();
    assert_eq!(written, vec!["led.bri".to_string(), "map.socd".to_string()]);
    assert_eq!(wanted.diff(&was), written);

    let read = Config::read(&mut session).unwrap();
    assert_eq!(read.get("led.bri"), Some(&Value::Scalar(200)));
    assert_eq!(read.get("map.socd"), Some(&Value::Scalar(2)));
}

#[test]
fn a_snapshot_of_a_device_goes_onto_another() {
    let mut first = session();
    let before = Config::read(&mut first).unwrap();
    let mut wanted = before.clone();
    wanted.set("led.bri", Value::Scalar(77)).unwrap();
    wanted
        .set("map.btn_map", Value::Array(vec![8, 7, 6, 5, 4, 3, 2, 1]))
        .unwrap();
    wanted.apply(&mut first, &before).unwrap();
    let body = br#"{"ver":2,"led":{"bri":77}}"#;
    first.profile_put(1, body).unwrap();

    // The snapshot is the file a device is kept as.
    let snapshot = Snapshot::take(&mut first).unwrap();
    assert_eq!(snapshot.board, "BoringTechH743");
    let text = snapshot.to_json().unwrap();
    let carried = Snapshot::from_json(&text).unwrap();
    assert_eq!(carried.config, snapshot.config);
    // The store carries its factory profile besides the one written here.
    assert_eq!(carried.profiles.len(), 2);

    // Onto a device that held something else.
    let mut second = session();
    let restored = carried.restore(&mut second).unwrap();
    assert_eq!(restored.values_written, carried.config.len());
    assert_eq!(
        restored.profiles_written, 1,
        "the factory profile is left where it is"
    );
    assert!(restored.keys_unknown.is_empty());
    assert!(restored.keys_refused.is_empty());

    let after = Config::read(&mut second).unwrap();
    assert_eq!(after.get("led.bri"), Some(&Value::Scalar(77)));
    assert_eq!(
        after.get("map.btn_map"),
        Some(&Value::Array(vec![8, 7, 6, 5, 4, 3, 2, 1]))
    );
    assert_eq!(second.profile_get(1).unwrap(), body.to_vec());
}

#[test]
fn a_snapshot_the_target_cannot_take_is_reported() {
    let mut session = session();
    let mut snapshot = Snapshot::take(&mut session).unwrap();
    snapshot
        .config
        .insert("board.only_key".to_string(), Value::Scalar(1));
    snapshot
        .config
        .insert("led.bri".to_string(), Value::Scalar(9999));

    let restored = snapshot.restore(&mut session).unwrap();
    assert_eq!(restored.keys_unknown, vec!["board.only_key".to_string()]);
    assert_eq!(restored.keys_refused, vec!["led.bri".to_string()]);
}
