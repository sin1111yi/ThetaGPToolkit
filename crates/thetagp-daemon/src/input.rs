//! The input device beside the port, read as the kernel holds it.
//!
//! A HID gamepad's reports reach the host's own driver, and the state that
//! driver holds is what a game reads. The kernel hands that state back through
//! two ioctls rather than through a stream -- one for the keys and one per axis
//! -- so a reading costs the device nothing and does not wait for a report to
//! change.

use std::os::unix::io::AsRawFd;

use thetagp_client::input::{Axis, Reading};

/// Reads the input node that sits beside `port`, on the same USB device.
pub fn read(port: &str) -> Result<Reading, String> {
    let node = node_for(port)
        .ok_or_else(|| format!("{port}: no input node beside the port"))?;
    let file = std::fs::File::open(&node).map_err(|error| format!("{}: {error}", node.display()))?;
    let fd = file.as_raw_fd();

    let mut keys = [0u8; KEY_BYTES];
    ioctl(fd, ioc_read(b'E', 0x18, KEY_BYTES), &mut keys)?;

    let mut axes_bits = [0u8; ABS_BYTES];
    ioctl(fd, ioc_read(b'E', 0x20 + EV_ABS, ABS_BYTES), &mut axes_bits)?;

    let name = std::fs::read_to_string(node.join("device/name"))
        .map(|text| text.trim().to_string())
        .unwrap_or_default();

    Ok(Reading {
        name,
        keys: (0..KEY_BITS as u16)
            .filter(|code| keys[(*code as usize) / 8] & (1 << (code % 8)) != 0)
            .collect(),
        axes: (0..ABS_BITS as u16)
            .filter(|code| axes_bits[(*code as usize) / 8] & (1 << (code % 8)) != 0)
            .filter_map(|code| {
                let mut info = AbsInfo::default();
                ioctl(fd, ioc_read(b'E', 0x40 + code as u8, std::mem::size_of::<AbsInfo>()), &mut info)
                    .ok()?;
                Some(Axis {
                    code,
                    value: info.value,
                    min: info.minimum,
                    max: info.maximum,
                })
            })
            .collect(),
    })
}

/// The input node under the port's own USB device: the interface the protocol
/// runs over and the input interface are siblings there.
fn node_for(port: &str) -> Option<std::path::PathBuf> {
    let tty = std::fs::canonicalize(port).ok()?;
    let tty_name = tty.file_name()?.to_str()?;
    let interface = std::fs::canonicalize(format!("/sys/class/tty/{tty_name}/device")).ok()?;
    let device = interface.parent()?;

    for entry in std::fs::read_dir("/sys/class/input").ok()? {
        let node = entry.ok()?.path();
        let name = node.file_name()?.to_str()?;
        if !name.starts_with("event") {
            continue;
        }
        let Ok(resolved) = std::fs::canonicalize(node.join("device")) else {
            continue;
        };
        if resolved.starts_with(device) {
            return Some(std::path::PathBuf::from(format!("/dev/input/{name}")));
        }
    }
    None
}

const EV_ABS: u8 = 0x03;
const KEY_BITS: usize = 0x300;
const KEY_BYTES: usize = KEY_BITS / 8;
const ABS_BITS: usize = 0x40;
const ABS_BYTES: usize = ABS_BITS / 8;

#[repr(C)]
#[derive(Default)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

/// The number an ioctl's request takes, as the kernel builds it from the
/// direction, the size, the group and the number.
const fn ioc_read(group: u8, number: u8, size: usize) -> std::ffi::c_ulong {
    ((2u64 << 30) | ((size as u64) << 16) | ((group as u64) << 8) | number as u64) as std::ffi::c_ulong
}

fn ioctl<T>(fd: i32, request: std::ffi::c_ulong, value: &mut T) -> Result<(), String> {
    // SAFETY: the request was built from this value's own size, and the pointer
    // is to a live, writable local of that size.
    let answered = unsafe { libc::ioctl(fd, request, value as *mut T) };
    if answered < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
