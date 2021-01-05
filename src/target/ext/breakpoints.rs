//! Add/Remove various kinds of breakpoints.

use crate::arch::Arch;
use crate::target::{Target, TargetResult};

use super::agent::{Agent, BytecodeId};

/// Target Extension - Set/Remove Breakpoints.
pub trait Breakpoints: Target {
    /// Set/Remote software breakpoints.
    fn sw_breakpoint(&mut self) -> Option<SwBreakpointOps<Self>> {
        None
    }

    /// Set/Remote hardware breakpoints.
    fn hw_breakpoint(&mut self) -> Option<HwBreakpointOps<Self>> {
        None
    }

    /// Set/Remote hardware watchpoints.
    fn hw_watchpoint(&mut self) -> Option<HwWatchpointOps<Self>> {
        None
    }

    /// Support target-side breakpoint command and condition evaluation.
    ///
    /// The target must implement the [`Agent`](super::agent::Agent) protocol
    /// extension to use this feature.
    fn breakpoint_agent(&mut self) -> Option<BreakpointAgentOps<Self>> {
        None
    }
}

define_ext!(BreakpointsOps, Breakpoints);

/// Nested Target Extension - Set/Remove Software Breakpoints.
///
/// See [this stackoverflow discussion](https://stackoverflow.com/questions/8878716/what-is-the-difference-between-hardware-and-software-breakpoints)
/// about the differences between hardware and software breakpoints.
///
/// _Recommendation:_ If you're implementing `Target` for an emulator that's
/// using an _interpreted_ CPU (as opposed to a JIT), the simplest way to
/// implement "software" breakpoints would be to check the `PC` value after each
/// CPU cycle, ignoring the specified breakpoint `kind` entirely.
pub trait SwBreakpoint: Target + Breakpoints {
    /// Add a new software breakpoint.
    /// Return `Ok(false)` if the operation could not be completed.
    fn add_sw_breakpoint(
        &mut self,
        addr: <Self::Arch as Arch>::Usize,
        kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self>;

    /// Remove an existing software breakpoint.
    /// Return `Ok(false)` if the operation could not be completed.
    fn remove_sw_breakpoint(
        &mut self,
        addr: <Self::Arch as Arch>::Usize,
        kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self>;
}

define_ext!(SwBreakpointOps, SwBreakpoint);

/// Nested Target Extension - Set/Remove Hardware Breakpoints.
///
/// See [this stackoverflow discussion](https://stackoverflow.com/questions/8878716/what-is-the-difference-between-hardware-and-software-breakpoints)
/// about the differences between hardware and software breakpoints.
///
/// _Recommendation:_ If you're implementing `Target` for an emulator that's
/// using an _interpreted_ CPU (as opposed to a JIT), there shouldn't be any
/// reason to implement this extension (as software breakpoints are likely to be
/// just-as-fast).
pub trait HwBreakpoint: Target + Breakpoints {
    /// Add a new hardware breakpoint.
    /// Return `Ok(false)` if the operation could not be completed.
    fn add_hw_breakpoint(
        &mut self,
        addr: <Self::Arch as Arch>::Usize,
        kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self>;

    /// Remove an existing hardware breakpoint.
    /// Return `Ok(false)` if the operation could not be completed.
    fn remove_hw_breakpoint(
        &mut self,
        addr: <Self::Arch as Arch>::Usize,
        kind: <Self::Arch as Arch>::BreakpointKind,
    ) -> TargetResult<bool, Self>;
}

define_ext!(HwBreakpointOps, HwBreakpoint);

/// The kind of watchpoint that should be set/removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchKind {
    /// Fire when the memory location is written to.
    Write,
    /// Fire when the memory location is read from.
    Read,
    /// Fire when the memory location is written to and/or read from.
    ReadWrite,
}

/// Nested Target Extension - Set/Remove Hardware Watchpoints.
///
/// See the [GDB documentation](https://sourceware.org/gdb/current/onlinedocs/gdb/Set-Watchpoints.html)
/// regarding watchpoints for how they're supposed to work.
///
/// _Note:_ If this extension isn't implemented, GDB will default to using
/// _software watchpoints_, which tend to be excruciatingly slow (as hey are
/// implemented by single-stepping the system, and reading the watched memory
/// location after each step).
pub trait HwWatchpoint: Target + Breakpoints {
    /// Add a new hardware watchpoint.
    /// Return `Ok(false)` if the operation could not be completed.
    fn add_hw_watchpoint(
        &mut self,
        addr: <Self::Arch as Arch>::Usize,
        kind: WatchKind,
    ) -> TargetResult<bool, Self>;

    /// Remove an existing hardware watchpoint.
    /// Return `Ok(false)` if the operation could not be completed.
    fn remove_hw_watchpoint(
        &mut self,
        addr: <Self::Arch as Arch>::Usize,
        kind: WatchKind,
    ) -> TargetResult<bool, Self>;
}

define_ext!(HwWatchpointOps, HwWatchpoint);

/// Determines when and where breakpoint bytecode are evaluated.
///
/// See [`BreakpointAgent::condition_executor`] for more details.
#[derive(Debug)]
pub enum BytecodeExecutor {
    /// Automatically, within the `gdbstub`
    Gdbstub,
    /// Manually, within the target's `resume()` method
    Target,
}

impl BytecodeExecutor {
    pub(crate) fn is_gdbstub(&self) -> bool {
        matches!(self, BytecodeExecutor::Gdbstub)
    }
}

/// The kind of bytecode expression associated with a breakpoint.
#[derive(Debug)]
pub enum BreakpointBytecodeKind {
    /// A condition (evaluates to a boolean value).
    Condition,
    /// A command.
    Command,
}

/// Nested Target Extension - Support target-side breakpoint command and
/// condition evaluation.
///
/// TODO: more docs
///
/// reference: https://sourceware.org/gdb/current/onlinedocs/gdb/Set-Breaks.html#Set-Breaks
pub trait BreakpointAgent: Target + Breakpoints + Agent {
    /// Specify when and where breakpoint bytecode are evaluated.
    ///
    /// Depending on what kind of performance you're looking for, you can choose
    /// where and when breakpoint bytecode should be executed.
    ///
    /// - `BytecodeExecutor::GdbStub` is the simpler option, and is enabled by
    ///   defualt. Bytecode is automatically executed by `gdbstub` after the
    ///   target returns a `SwBreak` or `HwBreak` stop-reason from `resume()`.
    ///
    /// - `BytecodeExecutor::Target` is the more advanced option - bytecode must
    ///   be manually evaluated within the target's `resume()` method, and the
    ///   `SwBreak` or `HwBreak` stop-reasons are only returned if the condition
    ///   is fulfilled.
    ///
    /// The default `BytecodeExecutor::GdbStub` option should be sufficient for
    /// most use cases, though if you're looking to squeeze out the maximum
    /// performance possible out of conditional breakpoints (e.g: by evaluating
    /// expressions within the hardware breakpoint handler and/or using a JIT),
    /// consider using `BytecodeExecutor::Target` instead.
    fn breakpoint_bytecode_executor(&self) -> BytecodeExecutor {
        BytecodeExecutor::Gdbstub
    }

    /// Add a new bytecode expression to evaluate when a breakpoint at `addr` is
    /// hit.
    ///
    /// A single breakpoint can have multiple conditions and commands associated
    /// with it. This operation must not overwrite any previously registered
    /// commands.
    ///
    /// If the bytecode `kind` is a `Command` and `persist` is set, the
    /// breakpoint _may_ remain active and commands _may_ continue to run
    /// even after GDB has disconnected from the target. The `persist` flag has
    /// no meaning when `kind` is `Condition`.
    fn add_breakpoint_bytecode(
        &mut self,
        kind: BreakpointBytecodeKind,
        addr: <Self::Arch as Arch>::Usize,
        id: BytecodeId,
        persist: bool,
    ) -> TargetResult<(), Self>;

    /// Remove all registered bytecode expressions of the specified `kind`
    /// associated with the `addr`.
    ///
    /// Implementors are responsible for performing any necessary "garbage
    /// collection" of bytecode expressions, and should not assume that
    /// [`Agent::unregister_bytecode`] will be called before/after executing
    /// this method.
    fn clear_breakpoint_bytecode(
        &mut self,
        kind: BreakpointBytecodeKind,
        addr: <Self::Arch as Arch>::Usize,
    ) -> TargetResult<(), Self>;

    /// Iterate over all bytecodes of the specified `kind` associated with
    /// breakpoint at `addr`, calling `callback(self, id)` for each
    /// `BytecodeId`.
    ///
    /// If no breakpoint is registered at `addr`, this method should be a no-op.
    fn get_breakpoint_bytecode(
        &mut self,
        kind: BreakpointBytecodeKind,
        addr: <Self::Arch as Arch>::Usize,
        callback: &mut dyn FnMut(BreakpointAgentOps<Self>, BytecodeId) -> Result<(), Self::Error>,
    ) -> Result<(), Self::Error>;
}

define_ext!(BreakpointAgentOps, BreakpointAgent);
