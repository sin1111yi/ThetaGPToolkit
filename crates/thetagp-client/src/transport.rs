//! What the bytes travel over.

use std::io;
use std::time::Duration;

/// One connection on which frames are written and read.
///
/// An implementation carries bytes and nothing else: it does not frame, number
/// or retry. A read that finds nothing before its timeout returns `Ok(0)`, which
/// is an empty hand rather than an end of anything.
pub trait Transport {
    /// Writes every byte of `bytes`.
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// Waits for the bytes already written to leave.
    fn flush(&mut self) -> io::Result<()>;

    /// Reads up to `buf.len()` bytes, waiting at most `timeout`, and returns how
    /// many arrived -- zero when none did.
    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize>;

    /// Discards bytes received but not read, so a retry starts at a boundary.
    fn flush_input(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A serial port, at the baud the CDC channel carries.
///
/// The port is a USB CDC channel, so the baud is nominal and the frame layer
/// does the framing either way; it is set because opening a port wants one.
pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    /// The baud the port is opened at.
    pub const BAUD: u32 = 115_200;

    /// Opens `path`, or the error the port gave.
    pub fn open(path: &str) -> Result<Self, serialport::Error> {
        let port = serialport::new(path, Self::BAUD)
            .timeout(Duration::from_millis(50))
            .open()?;
        Ok(SerialTransport { port })
    }
}

impl Transport for SerialTransport {
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.port.write_all(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.port.flush()
    }

    fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        self.port.set_timeout(timeout)?;
        match self.port.read(buf) {
            Ok(read) => Ok(read),
            // An empty hand, not a failure: the caller's deadline decides.
            Err(error) if error.kind() == io::ErrorKind::TimedOut => Ok(0),
            Err(error) => Err(error),
        }
    }

    fn flush_input(&mut self) -> io::Result<()> {
        self.port.clear(serialport::ClearBuffer::Input)?;
        Ok(())
    }
}
