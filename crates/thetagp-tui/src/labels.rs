//! What a key is called in front of a person.
//!
//! The device declares a key's name, shape and range; it declares no prose for
//! it. These labels are presentation only -- a key this table does not know is
//! shown by its name, and nothing about what it holds or what it may be set to
//! comes from here.

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

/// The buttons a remap slot may feed, in the order the firmware's own mask table
/// carries them; a slot past the named ones is left to its index.
const BUTTONS: [&str; 24] = [
    "B1", "B2", "B3", "B4", "L1", "R1", "L2", "R2", "S1", "S2", "L3", "R3", "A1", "A2", "A3",
    "A4", "E1", "E2", "E3", "E4", "E5", "E6", "E7", "E8",
];

/// The firmware's value for a slot that feeds nothing.
pub const UNMAPPED: i32 = 0xFF;

/// The button a slot index names, when the firmware names one.
pub fn button(index: i32) -> Option<&'static str> {
    usize::try_from(index)
        .ok()
        .and_then(|index| BUTTONS.get(index))
        .copied()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_is_named_by_the_button_it_feeds() {
        assert_eq!(slot(0), "B1 (0)");
        assert_eq!(slot(4), "L1 (4)");
        assert_eq!(slot(23), "E8 (23)");
        assert_eq!(slot(24), "24");
        assert_eq!(slot(UNMAPPED), "unmapped");
    }

    #[test]
    fn a_known_key_is_named_and_an_unknown_one_is_not() {
        assert_eq!(label("map.socd"), Some("SOCD Mode"));
        assert_eq!(label("led.bri"), Some("LED Brightness"));
        assert_eq!(label("no.such.key"), None);
    }
}
