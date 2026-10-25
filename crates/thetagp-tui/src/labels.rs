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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_key_is_named_and_an_unknown_one_is_not() {
        assert_eq!(label("map.socd"), Some("SOCD Mode"));
        assert_eq!(label("led.bri"), Some("LED Brightness"));
        assert_eq!(label("no.such.key"), None);
    }
}
