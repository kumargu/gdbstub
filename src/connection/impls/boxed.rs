use core::task::{Context, Poll};

use alloc::boxed::Box;

use crate::Connection;

impl<E> Connection for Box<dyn Connection<Error = E>> {
    type Error = E;

    fn read(&mut self) -> Result<u8, Self::Error> {
        (**self).read()
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Self::Error> {
        (**self).read_exact(buf)
    }

    fn write(&mut self, byte: u8) -> Result<(), Self::Error> {
        (**self).write(byte)
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        (**self).write_all(buf)
    }

    fn peek(&mut self) -> Result<u8, Self::Error> {
        (**self).peek()
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        (**self).flush()
    }

    fn on_session_start(&mut self) -> Result<(), Self::Error> {
        (**self).on_session_start()
    }

    fn poll_readable(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        (**self).poll_readable(cx)
    }
}
