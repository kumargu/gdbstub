use core::task::{Context, Poll};

use std::net::TcpStream;
#[cfg(all(feature = "std", target_family = "unix"))]
use std::os::unix::io::{AsRawFd, RawFd};

use crate::Connection;

impl Connection for TcpStream {
    type Error = std::io::Error;

    fn read(&mut self) -> Result<u8, Self::Error> {
        use std::io::Read;

        let mut buf = [0u8];
        match Read::read_exact(self, &mut buf) {
            Ok(_) => Ok(buf[0]),
            Err(e) => Err(e),
        }
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Self::Error> {
        use std::io::Read;

        Read::read_exact(self, buf)
    }

    fn peek(&mut self) -> Result<u8, Self::Error> {
        let mut buf = [0u8];
        match TcpStream::peek(self, &mut buf) {
            Ok(_) => Ok(buf[0]),
            Err(e) => Err(e),
        }
    }

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        use std::io::Write;

        Write::write_all(self, &[byte])
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        use std::io::Write;

        Write::write_all(self, buf)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        use std::io::Write;

        Write::flush(self)
    }

    fn on_session_start(&mut self) -> Result<(), Self::Error> {
        self.set_nonblocking(false)?;
        // see issue #28
        self.set_nodelay(true)?;
        Ok(())
    }

    fn poll_readable(&self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.set_nonblocking(true)?;

        let mut buf = [0u8];
        let res = match TcpStream::peek(self, &mut buf) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => Poll::Pending,
            Err(e) => Poll::Ready(Err(e)),
        };

        self.set_nonblocking(false)?;

        res
    }

    #[cfg(all(feature = "std", target_family = "unix"))]
    fn as_raw_fd(&self) -> Option<RawFd> {
        Some(AsRawFd::as_raw_fd(self))
    }
}
