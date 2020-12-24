//! Base operations required to debug any target (read/write memory/registers,
//! step/resume, etc...)
//!
//! While not strictly required, it's recommended that single threaded targets
//! implement the simplified `singlethread` API.

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

/// A [`Future`] that resolves if the GDB client sent an interrupt (e.g: on
/// Ctrl-C).
pub struct GdbInterrupt<'a> {
    inner: Pin<&'a mut dyn Future<Output = ()>>,
}

impl<'a> GdbInterrupt<'a> {
    pub(crate) fn new(inner: Pin<&'a mut dyn Future<Output = ()>>) -> GdbInterrupt<'a> {
        GdbInterrupt { inner }
    }

    /// Returns a [`GdbInterruptNoAsync`] struct which can be polled using a
    /// simple non-blocking [`pending(&mut self) ->
    /// bool`](GdbInterruptNoAsync::pending) method.
    pub fn no_async(self) -> GdbInterruptNoAsync<'a> {
        GdbInterruptNoAsync {
            ready: false,
            interrupt: self,
        }
    }
}

impl<'a> Future for GdbInterrupt<'a> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.inner.as_mut().poll(cx)
    }
}

/// A simplified interface to [`GdbInterrupt`] for projects without
/// async/await infrastructure.
pub struct GdbInterruptNoAsync<'a> {
    ready: bool,
    interrupt: GdbInterrupt<'a>,
}

impl<'a> GdbInterruptNoAsync<'a> {
    /// Checks if there is a pending GDB interrupt.
    pub fn pending(&mut self) -> bool {
        // polling a future after its returned `Ready` is forbidden.
        if self.ready {
            return true;
        }

        match self
            .interrupt
            .inner
            .as_mut()
            .poll(&mut Context::from_waker(&dummy_waker()))
        {
            Poll::Ready(_) => self.ready = true,
            Poll::Pending => self.ready = false,
        }

        self.ready
    }
}

use core::task::{RawWaker, RawWakerVTable, Waker};

fn dummy_raw_waker() -> RawWaker {
    fn no_op(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        dummy_raw_waker()
    }

    let vtable = &RawWakerVTable::new(clone, no_op, no_op, no_op);
    RawWaker::new(core::ptr::null(), vtable)
}

fn dummy_waker() -> Waker {
    unsafe { Waker::from_raw(dummy_raw_waker()) }
}
