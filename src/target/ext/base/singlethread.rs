//! Base debugging operations for single threaded targets.

use crate::arch::Arch;
use crate::target::ext::breakpoints::WatchKind;
use crate::target::{Target, TargetResult};

// Convenient re-export
pub use super::{GdbInterrupt, ResumeAction};

/// Base debugging operations for single threaded targets.
#[allow(clippy::type_complexity)]
pub trait SingleThreadOps: Target {
    /// Resume execution on the target.
    ///
    /// `action` specifies how the target should be resumed (i.e:
    /// single-step vs. full continue).
    ///
    /// `gdb_interrupt` is a pollable handle which only resolves if a GDB client
    /// requests a interrupt (e.g: a user pressing Ctrl-C). The [`GdbInterrupt`]
    /// type implements several async interfaces, making it easy to integrate
    /// no matter what async model the target supports. Please refer to the
    /// type's documentation for more details.
    ///
    /// While targets are not required to handle GDB client interrupts, doing so
    /// is highly recommended.
    ///
    /// # Implementation requirements
    ///
    /// These requirements cannot be satisfied by `gdbstub` internally, and must
    /// be handled on a per-target basis.
    ///
    /// ### Adjusting PC after a breakpoint is hit
    ///
    /// The [GDB remote serial protocol documentation](https://sourceware.org/gdb/current/onlinedocs/gdb/Stop-Reply-Packets.html#swbreak-stop-reason)
    /// notes the following:
    ///
    /// > On some architectures, such as x86, at the architecture level, when a
    /// > breakpoint instruction executes the program counter points at the
    /// > breakpoint address plus an offset. On such targets, the stub is
    /// > responsible for adjusting the PC to point back at the breakpoint
    /// > address.
    ///
    /// Omitting PC adjustment may result in unexpected execution flow and/or
    /// breakpoints not appearing to work correctly.
    fn resume(
        &mut self,
        action: ResumeAction,
        gdb_interrupt: GdbInterrupt<'_>,
    ) -> Result<StopReason<<Self::Arch as Arch>::Usize>, Self::Error>;

    /// Read the target's registers.
    fn read_registers(
        &mut self,
        regs: &mut <Self::Arch as Arch>::Registers,
    ) -> TargetResult<(), Self>;

    /// Write the target's registers.
    fn write_registers(&mut self, regs: &<Self::Arch as Arch>::Registers)
        -> TargetResult<(), Self>;

    /// Read to a single register on the target.
    ///
    /// Implementations should write the value of the register using target's
    /// native byte order in the buffer `dst`.
    ///
    /// If the requested register could not be accessed, an appropriate
    /// non-fatal error should be returned.
    ///
    /// _Note:_ This method includes a stubbed default implementation which
    /// simply returns `Ok(())`. This is due to the fact that several built-in
    /// `arch` implementations haven't been updated with proper `RegId`
    /// implementations.
    fn read_register(
        &mut self,
        reg_id: <Self::Arch as Arch>::RegId,
        dst: &mut [u8],
    ) -> TargetResult<(), Self> {
        let _ = (reg_id, dst);
        Ok(())
    }

    /// Write from a single register on the target.
    ///
    /// The `val` buffer contains the new value of the register in the target's
    /// native byte order. It is guaranteed to be the exact length as the target
    /// register.
    ///
    /// If the requested register could not be accessed, an appropriate
    /// non-fatal error should be returned.
    ///
    /// _Note:_ This method includes a stubbed default implementation which
    /// simply returns `Ok(())`. This is due to the fact that several built-in
    /// `arch` implementations haven't been updated with proper `RegId`
    /// implementations.
    fn write_register(
        &mut self,
        reg_id: <Self::Arch as Arch>::RegId,
        val: &[u8],
    ) -> TargetResult<(), Self> {
        let _ = (reg_id, val);
        Ok(())
    }

    /// Read bytes from the specified address range.
    ///
    /// If the requested address range could not be accessed (e.g: due to
    /// MMU protection, unhanded page fault, etc...), an appropriate
    /// non-fatal error should be returned.
    fn read_addrs(
        &mut self,
        start_addr: <Self::Arch as Arch>::Usize,
        data: &mut [u8],
    ) -> TargetResult<(), Self>;

    /// Write bytes to the specified address range.
    ///
    /// If the requested address range could not be accessed (e.g: due to
    /// MMU protection, unhanded page fault, etc...), an appropriate
    /// non-fatal error should be returned.
    fn write_addrs(
        &mut self,
        start_addr: <Self::Arch as Arch>::Usize,
        data: &[u8],
    ) -> TargetResult<(), Self>;
}

/// Describes why the target stopped.
// NOTE: This is a simplified version of `multithread::ThreadStopReason` that omits any references
// to Tid or threads. Internally, it is converted into multithread::ThreadStopReason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StopReason<U> {
    /// Completed the single-step request.
    DoneStep,
    /// `check_gdb_interrupt` returned `true`
    GdbInterrupt,
    /// Halted
    Halted,
    /// Hit a software breakpoint (e.g. due to a trap instruction).
    ///
    /// NOTE: This does not necessarily have to be a breakpoint configured by
    /// the client/user of the current GDB session.
    SwBreak,
    /// Hit a hardware breakpoint.
    HwBreak,
    /// Hit a watchpoint.
    Watch {
        /// Kind of watchpoint that was hit
        kind: WatchKind,
        /// Address of watched memory
        addr: U,
    },
    /// The program received a signal
    Signal(u8),
}
