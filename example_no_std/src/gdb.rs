use crate::print_str::print_str;
use gdbstub::{arch, Actions, BreakOp, OptResult, StopReason, Target, Tid, SINGLE_THREAD_TID};

pub struct DummyTarget {}

impl DummyTarget {
    pub fn new() -> DummyTarget {
        DummyTarget {}
    }
}

// NOTE: to try and make this a more realistic example, methods are marked as
// `#[inline(never)]` to prevent the optimizer from too aggressively coalescing
// the stubbed implementations.

impl Target for DummyTarget {
    type Arch = arch::arm::Armv4t;
    type Error = &'static str;

    #[inline(never)]
    fn resume(
        &mut self,
        _actions: Actions,
        _check_gdb_interrupt: &mut dyn FnMut() -> bool,
    ) -> Result<(Tid, StopReason<u32>), Self::Error> {
        print_str("> resume");
        Ok((SINGLE_THREAD_TID, StopReason::DoneStep))
    }

    #[inline(never)]
    fn read_registers(
        &mut self,
        _regs: &mut arch::arm::reg::ArmCoreRegs,
    ) -> Result<(), &'static str> {
        print_str("> read_registers");
        Ok(())
    }

    #[inline(never)]
    fn write_registers(&mut self, _regs: &arch::arm::reg::ArmCoreRegs) -> Result<(), &'static str> {
        print_str("> write_registers");
        Ok(())
    }

    #[inline(never)]
    fn read_addrs(&mut self, _start_addr: u32, data: &mut [u8]) -> Result<bool, &'static str> {
        print_str("> read_addrs");
        data.iter_mut().for_each(|b| *b = 0x55);
        Ok(true)
    }

    #[inline(never)]
    fn write_addrs(&mut self, _start_addr: u32, _data: &[u8]) -> Result<bool, &'static str> {
        print_str("> write_addrs");
        Ok(true)
    }

    #[inline(never)]
    fn update_sw_breakpoint(&mut self, _addr: u32, _op: BreakOp) -> Result<bool, &'static str> {
        print_str("> update_sw_breakpoint");
        Ok(true)
    }

    #[inline(never)]
    fn list_active_threads(
        &mut self,
        register_thread: &mut dyn FnMut(Tid),
    ) -> Result<(), Self::Error> {
        print_str("> list_active_threads");
        register_thread(Tid::new(1).unwrap());
        register_thread(Tid::new(2).unwrap());
        Ok(())
    }

    #[inline(never)]
    fn set_current_thread(&mut self, _tid: Tid) -> OptResult<(), Self::Error> {
        Ok(())
    }
}
