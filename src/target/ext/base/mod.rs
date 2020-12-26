//! Base operations required to debug any target (read/write memory/registers,
//! step/resume, etc...)
//!
//! While not strictly required, it's recommended that single threaded targets
//! implement the simplified `singlethread` API.

#[cfg(all(feature = "std", target_family = "unix"))]
use std::os::unix::io::RawFd;

pub mod multithread;
pub mod singlethread;

/// Base operations for single/multi threaded targets.
pub enum BaseOps<'a, A, E> {
    /// Single-threaded target
    SingleThread(&'a mut dyn singlethread::SingleThreadOps<Arch = A, Error = E>),
    /// Multi-threaded target
    MultiThread(&'a mut dyn multithread::MultiThreadOps<Arch = A, Error = E>),
}

/// Describes how the target should be resumed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeAction {
    /// Continue execution (until the next event occurs).
    Continue,
    /// Step forward a single instruction.
    Step,
    /* ContinueWithSignal(u8),
     * StepWithSignal(u8),
     * Stop, // NOTE: won't be relevant until `gdbstub` supports non-stop mode
     * StepInRange(core::ops::Range<U>), */
}

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

/// A pollable handle which only resolves if a GDB client requests a interrupt
/// (e.g: a user pressing Ctrl-C).
///
/// The `GdbInterrupt` type implements several async interfaces, making it
/// easy to integrate no matter what async model the target supports:
///
/// # 1. Manual Polling
///
/// # 2. `async/await`
///
/// [`GdbInterrupt`] implements Rust's standard [`Future`] interface, resolving
/// to `()` if the GDB client sends an interrupt. This future is directly
/// derived from the
/// [`Connection::poll_readable`](crate::Connection::poll_readable) method.
///
/// **NOTE:** While `gdbstub` does include built-in implementations of
/// [`Connection`](crate::Connection) for standard library types such as
/// `TcpStream` and `UnixStream`, these implementations do _not_ hook into any
/// futures executor
///
/// # 3. (*nix only) `as_raw_fd` + `poll`
///
/// If the underlying `Connection` object is backed by a file descriptor, the
/// `as_raw_fd()` method can be used to get a copy of the underling [`RawFd`].
///
/// This file descriptor can then be used alongside a
/// [`poll`](https://man7.org/linux/man-pages/man2/poll.2.html)-like API to
/// wait for GDB interrupts in conjunction with other events.
pub struct GdbInterrupt<'a> {
    #[cfg(all(feature = "std", target_family = "unix"))]
    fd: Option<RawFd>,
    inner: GdbInterruptInner<'a>,
}

enum GdbInterruptInner<'a> {
    Poll(&'a mut dyn FnMut() -> bool),
    Future(Pin<&'a mut dyn Future<Output = ()>>),
}

impl<'a> GdbInterrupt<'a> {
    pub(crate) fn new_pollable(fun: &'a mut dyn FnMut() -> bool) -> GdbInterrupt<'a> {
        GdbInterrupt {
            #[cfg(all(feature = "std", target_family = "unix"))]
            fd: None,
            inner: GdbInterruptInner::Poll(fun),
        }
    }

    pub(crate) fn new_future(fut: Pin<&'a mut dyn Future<Output = ()>>) -> GdbInterrupt<'a> {
        GdbInterrupt {
            #[cfg(all(feature = "std", target_family = "unix"))]
            fd: None,
            inner: GdbInterruptInner::Future(fut),
        }
    }

    /// Returns a [`GdbInterruptManualPoll`] struct which can be polled using a
    /// simple non-blocking
    /// [`pending(&mut self) -> bool`](GdbInterruptManualPoll::pending) method.
    pub fn manual_poll(self) -> GdbInterruptManualPoll<'a> {
        GdbInterruptManualPoll {
            ready: false,
            interrupt: self,
        }
    }

    /// If the underlying [`Connection`](crate::Connection) implements an async
    /// interface, return a [`Future<Output = ()>`] which resolves if a GDB
    /// client requests an interrupt.
    pub fn into_future(self) -> Option<Pin<&'a mut dyn Future<Output = ()>>> {
        match self.inner {
            GdbInterruptInner::Poll(_) => None,
            GdbInterruptInner::Future(fut) => Some(fut),
        }
    }

    #[cfg(all(feature = "std", target_family = "unix"))]
    pub(crate) fn set_fd(&mut self, fd: Option<RawFd>) {
        self.fd = fd;
    }

    /// Extracts the connection's underlying raw file descriptor, if available.
    #[cfg(all(feature = "std", target_family = "unix"))]
    pub fn as_raw_fd(&self) -> Option<RawFd> {
        self.fd
    }
}

/// Manually poll for a pending GDB interrupt.
pub struct GdbInterruptManualPoll<'a> {
    ready: bool,
    interrupt: GdbInterrupt<'a>,
}

impl<'a> GdbInterruptManualPoll<'a> {
    /// Checks if there is a pending GDB interrupt.
    pub fn pending(&mut self) -> bool {
        // polling a future after its returned `Ready` is forbidden.
        if self.ready {
            return true;
        }

        match &mut self.interrupt.inner {
            GdbInterruptInner::Poll(fun) => self.ready = fun(),
            GdbInterruptInner::Future(fut) => {
                match fut.as_mut().poll(&mut Context::from_waker(&dummy_waker())) {
                    Poll::Ready(_) => self.ready = true,
                    Poll::Pending => self.ready = false,
                }
            }
        }

        self.ready
    }
}

use core::task::{RawWaker, RawWakerVTable, Waker};

fn dummy_waker() -> Waker {
    fn dummy_raw_waker() -> RawWaker {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            dummy_raw_waker()
        }
        let vtable = &RawWakerVTable::new(clone, no_op, no_op, no_op);
        RawWaker::new(core::ptr::null(), vtable)
    }

    unsafe { Waker::from_raw(dummy_raw_waker()) }
}
