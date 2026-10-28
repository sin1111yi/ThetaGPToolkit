//! What the device's input node holds, and the names the kernel gives its codes.
//!
//! The reports a game binds to are handled by the host's own driver, so the
//! state that driver holds is the state a game reads. These types carry it
//! across the daemon's socket unchanged: the codes are the kernel's, and the
//! names are what a reader recognises them by.

/// One axis as the kernel holds it: the code that names it and where it stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axis {
    pub code: u16,
    pub value: i32,
    pub min: i32,
    pub max: i32,
}

impl Axis {
    /// Where the value stands between the ends of its range, in thousandths.
    pub fn position(&self) -> i32 {
        let span = self.max - self.min;
        if span <= 0 {
            return 0;
        }
        ((self.value - self.min) as i64 * 1000 / span as i64) as i32
    }
}

/// What the input node holds at the moment it is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// The node's own name, as the kernel carries it.
    pub name: String,
    /// The keys held down, by key code.
    pub keys: Vec<u16>,
    /// Every axis the node carries, whether anything moves it or not.
    pub axes: Vec<Axis>,
}

/// The name an axis code carries, out of the set a pad or a wheel reports.
pub fn axis_name(code: u16) -> Option<&'static str> {
    let name = match code {
        0x00 => "X",
        0x01 => "Y",
        0x02 => "Z",
        0x03 => "Rx",
        0x04 => "Ry",
        0x05 => "Rz",
        0x06 => "Throttle",
        0x07 => "Rudder",
        0x08 => "Wheel",
        0x09 => "Gas",
        0x0a => "Brake",
        0x10 => "Hat X",
        0x11 => "Hat Y",
        _ => return None,
    };
    Some(name)
}

/// The name a key code carries, out of the gamepad set the kernel names.
pub fn key_name(code: u16) -> Option<&'static str> {
    let name = match code {
        0x130 => "South",
        0x131 => "East",
        0x132 => "C",
        0x133 => "North",
        0x134 => "West",
        0x135 => "Z",
        0x136 => "TL",
        0x137 => "TR",
        0x138 => "TL2",
        0x139 => "TR2",
        0x13a => "Select",
        0x13b => "Start",
        0x13c => "Mode",
        0x13d => "ThumbL",
        0x13e => "ThumbR",
        // A pad's buttons past the fifteen the kernel names land in its
        // trigger-happy run, which is a bucket rather than a name: the ordinal
        // is what a reader can hold on to.
        0x2c0..=0x2df => HAPPY[(code - 0x2c0) as usize],
        _ => return None,
    };
    Some(name)
}

/// The names of the kernel's trigger-happy run, one per slot.
static HAPPY: [&str; 32] = [
    "H1", "H2", "H3", "H4", "H5", "H6", "H7", "H8", "H9", "H10", "H11", "H12", "H13", "H14",
    "H15", "H16", "H17", "H18", "H19", "H20", "H21", "H22", "H23", "H24", "H25", "H26", "H27",
    "H28", "H29", "H30", "H31", "H32",
];

/// The button index a key code stands for on a gamepad, as the host numbers the
/// buttons a report carries: the fifteen the kernel names, then the trigger-happy
/// run the rest of a pad's buttons land in.
pub fn button_index(code: u16) -> Option<u32> {
    match code {
        0x130..=0x13e => Some(u32::from(code - 0x130)),
        // The bucket starts at the pad's seventeenth button, so the first
        // code stands for the index sixteen.
        0x2c0..=0x2df => Some(u32::from(code - 0x2c0) + 16),
        _ => None,
    }
}

/// The button a held key stands for, in the order the profile numbers them. The
/// generic pad sends B1 to B3 in the order a host expects to read them, so the
/// three are put back before the mapping is looked up: a light that follows the
/// wire is worth more than one that follows the byte.
pub fn pad_button_index(code: u16) -> Option<u32> {
    let sent = button_index(code)?;
    Some(match sent {
        4 => 6,
        5 => 4,
        6 => 5,
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_axis_is_placed_along_its_own_range() {
        let centre = Axis {
            code: 0,
            value: 32767,
            min: 0,
            max: 65535,
        };
        assert_eq!(centre.position(), 499);

        let rested = Axis {
            code: 1,
            value: 0,
            min: 0,
            max: 255,
        };
        assert_eq!(rested.position(), 0);
    }

    #[test]
    fn the_codes_a_pad_reports_are_named() {
        assert_eq!(axis_name(0x00), Some("X"));
        assert_eq!(axis_name(0x10), Some("Hat X"));
        assert_eq!(key_name(0x130), Some("South"));
        assert_eq!(button_index(0x130), Some(0));
        assert_eq!(button_index(0x13e), Some(14));
        assert_eq!(button_index(0x2c0), Some(16));
        assert_eq!(button_index(0x2c4), Some(20));
        assert_eq!(button_index(0x2cf), Some(31));
        assert_eq!(button_index(0x123), None);
        assert_eq!(key_name(0x2c4), Some("H5"));
        // B1 travels as the report's fifth bit, and reads back as the fourth.
        assert_eq!(pad_button_index(0x135), Some(4));
        assert_eq!(pad_button_index(0x134), Some(6));
        assert_eq!(pad_button_index(0x133), Some(3));
        assert_eq!(axis_name(0x3f), None);
        assert_eq!(key_name(0x2ff), None);
    }
}
