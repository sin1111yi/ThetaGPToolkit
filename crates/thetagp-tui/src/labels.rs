//! What a key is called in front of a person.
//!
//! The device declares a key's name, shape and range; it declares no prose for
//! it. These labels are presentation only -- a key this table does not know is
//! shown by its name, and nothing about what it holds or what it may be set to
//! comes from here.

/// What a key, or a segment of the menu that holds keys, is for. A device
/// declares names and ranges and no prose, so this is the screen's own account
/// of what the value it shows does.
pub fn about(name: &str) -> Option<&'static str> {
    Some(match name {
        // ── Segments ──
        "map" => "How the buttons, the d-pad and the sticks are routed.",
        "stick" => "How each analog stick's position is shaped before it is sent.",
        "trig" => "When an analog trigger counts as pressed.",
        "led" => "What the strip on the board shows.",
        "led.rgb" => "How often the strip is written to.",
        "usb" => "What the board reports as to the host.",
        "sys" => "How the firmware itself behaves: its log and its keys.",
        "cal" => "The centre offset of each analog axis.",

        // ── Keys ──
        "map.socd" => "How opposite directions are cleaned when both are held: which input wins.",
        "map.four_way" => "Of two opposing directions, keep only the one pressed last.",
        "map.dpad" => "Where the d-pad is routed: the d-pad itself, the left stick or the right.",
        "map.inv_x" => "Send the left stick's horizontal axis inverted.",
        "map.inv_y" => "Send the left stick's vertical axis inverted.",
        "map.inv_rx" => "Send the right stick's horizontal axis inverted.",
        "map.inv_ry" => "Send the right stick's vertical axis inverted.",
        "map.swap" => "Send the two sticks in each other's place.",
        "map.btn_map" => "The physical inputs this board wires, and the button each one feeds.",
        "stick.lx_dz" => "How far the left stick is ignored before it reads as moved, across its circle.",
        "stick.ly_dz" => "How far the left stick is ignored before it reads as moved, across its circle.",
        "stick.rx_dz" => "How far the right stick is ignored before it reads as moved, across its circle.",
        "stick.ry_dz" => "How far the right stick is ignored before it reads as moved, across its circle.",
        "stick.lx_sens" => "How far a full left-stick deflection reaches.",
        "stick.ly_sens" => "How far a full left-stick deflection reaches.",
        "stick.rx_sens" => "How far a full right-stick deflection reaches.",
        "stick.ry_sens" => "How far a full right-stick deflection reaches.",
        "stick.curve" => "The shape of the stick response between the centre and the edge.",
        "stick.ema" => "How much of the previous position is carried into the next.",
        "trig.lt_dz" => "How far the left trigger is ignored before it reads as pressed.",
        "trig.rt_dz" => "How far the right trigger is ignored before it reads as pressed.",
        "led.bri" => "Brightness as a share of the strip's limit: 128 is the whole of it.",
        "led.mode" => "The effect the strip shows: dark, the colour wheel, or one colour held.",
        "led.hue" => "Where the colour sits on the wheel: red at 0, green at 120, blue at 240.",
        "led.sat" => "How far the colour is from white: 0 is white, 255 is the colour itself.",
        "led.rgb.hz" => "Frames a second the strip is written at.",
        "usb.input_mode" => "The device the board reports as, which is what the host binds.",
        "sys.log" => "The threshold the firmware writes at: from Info up, Debug shows every line.",
        "sys.deb_samp" => "Scans a key has to read as pressed before the press is committed.",
        "sys.deb_thr" => "Scans a key has to read as released before the release is committed.",
        "cal.lx_c" => "Where the left stick's centre is taken to be.",
        "cal.ly_c" => "Where the left stick's centre is taken to be.",
        "cal.rx_c" => "Where the right stick's centre is taken to be.",
        "cal.ry_c" => "Where the right stick's centre is taken to be.",
        _ => return None,
    })
}

/// The readable name of a key, when one is known.
pub fn label(name: &str) -> Option<&'static str> {
    Some(match name {
        "map.socd" => "SOCD Mode",
        "map.four_way" => "Four-way Mode",
        "map.dpad" => "D-pad Mode",
        "map.inv_x" => "Invert LX",
        "map.inv_y" => "Invert LY",
        "map.inv_rx" => "Invert RX",
        "map.inv_ry" => "Invert RY",
        "map.swap" => "Swap Sticks",
        "map.btn_map" => "Button Remap",
        "stick.lx_dz" => "LX Dead Zone",
        "stick.ly_dz" => "LY Dead Zone",
        "stick.rx_dz" => "RX Dead Zone",
        "stick.ry_dz" => "RY Dead Zone",
        "stick.lx_sens" => "LX Sensitivity",
        "stick.ly_sens" => "LY Sensitivity",
        "stick.rx_sens" => "RX Sensitivity",
        "stick.ry_sens" => "RY Sensitivity",
        "stick.curve" => "Stick Curve",
        "stick.ema" => "Stick Smoothing",
        "trig.lt_dz" => "LT Dead Zone",
        "trig.rt_dz" => "RT Dead Zone",
        "led.bri" => "LED Brightness",
        "led.mode" => "LED Mode",
        "led.hue" => "LED Hue",
        "led.sat" => "LED Saturation",
        "led.spd" => "LED Speed",
        "led.rgb.hz" => "RGB Refresh Rate",
        "cal.lx_c" => "LX Centre",
        "cal.ly_c" => "LY Centre",
        "cal.rx_c" => "RX Centre",
        "cal.ry_c" => "RY Centre",
        "sys.log" => "Log Level",
        "sys.deb_samp" => "Debounce Samples",
        "sys.deb_thr" => "Debounce Threshold",
        "usb.input_mode" => "USB Input Mode",
        "usb.poll" => "USB Poll Interval",
        _ => return None,
    })
}

/// The firmware's value for a slot that feeds nothing.
pub const UNMAPPED: i32 = 0xFF;

/// The button a slot index names, from the schema's own GamepadButton.
pub fn button(index: i32) -> Option<String> {
    thetagp_proto::enum_value_name("GamepadButton", index)
}

/// A remap value as the table shows it: the button's name and its index, or
/// `unmapped` for the slot that feeds nothing.
pub fn slot(value: i32) -> String {
    if value == UNMAPPED {
        return "unmapped".to_string();
    }
    match button(value) {
        Some(name) => format!("{name} ({value})"),
        None => value.to_string(),
    }
}

/// The input modes this firmware does not present: the schema carries the numbers
/// for boards that do, and a mode named here is one no board answers with.
pub fn unsupported_mode(number: i32) -> bool {
    number == thetagp_proto::thetagp::InputMode::Keyboard as i32
}

/// A key's value as the table shows it: the name its enum gives the number,
/// with the number beside it, or the bare number when the schema names none. An
/// input mode this firmware leaves out reads as unsupported rather than by a
/// name the board does not act on.
pub fn value(key: &thetagp_client::Key, number: i32) -> String {
    if key.name == "usb.input_mode" && unsupported_mode(number) {
        return format!("unsupported ({number})");
    }
    match &key.value_enum {
        Some(enum_name) => match thetagp_proto::enum_value_name(enum_name, number) {
            Some(name) => format!("{name} ({number})"),
            None => number.to_string(),
        },
        None => number.to_string(),
    }
}

/// A run of values, each shown the way its key's enum reads it.
pub fn values(key: &thetagp_client::Key, run: &[i32]) -> String {
    run.iter()
        .map(|number| value(key, *number))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_is_named_by_the_button_the_schema_lists() {
        // The value is the bit the mask names: UP is 0, B1 is 4, E8 is 31.
        assert_eq!(button(0).as_deref(), Some("UP"));
        assert_eq!(button(4).as_deref(), Some("B1"));
        assert_eq!(button(20).as_deref(), Some("DU"));
        assert_eq!(button(31).as_deref(), Some("E8"));
        assert_eq!(button(32), None);
        assert_eq!(slot(0), "UP (0)");
        assert_eq!(slot(4), "B1 (4)");
        assert_eq!(slot(31), "E8 (31)");
        assert_eq!(slot(32), "32");
        assert_eq!(slot(UNMAPPED), "unmapped");
    }

    #[test]
    fn a_value_is_named_by_the_enum_its_key_declares() {
        let key = thetagp_client::Key {
            name: "map.socd".into(),
            domain: "map".into(),
            leaf: "socd".into(),
            kind: thetagp_client::KeyType::U8,
            min: 0,
            max: 4,
            count: 1,
            reboot: false,
            accepts_unmapped: false,
            value_enum: Some("SOCDMode".into()),
        };
        assert_eq!(value(&key, 0), "UpPriority (0)");
        assert_eq!(value(&key, 2), "SecondInputPriority (2)");
    }

    #[test]
    fn an_input_mode_the_firmware_leaves_out_is_not_named() {
        let key = thetagp_client::Key {
            name: "usb.input_mode".into(),
            domain: "usb".into(),
            leaf: "input_mode".into(),
            kind: thetagp_client::KeyType::U8,
            min: 0,
            max: 255,
            count: 1,
            reboot: true,
            accepts_unmapped: false,
            value_enum: Some("InputMode".into()),
        };
        assert_eq!(value(&key, 3), "unsupported (3)");
        assert_eq!(value(&key, 14), "GENERIC (14)");
    }

    #[test]
    fn a_known_key_is_named_and_an_unknown_one_is_not() {
        assert_eq!(label("map.socd"), Some("SOCD Mode"));
        assert_eq!(label("led.bri"), Some("LED Brightness"));
        assert_eq!(label("no.such.key"), None);
    }
}
