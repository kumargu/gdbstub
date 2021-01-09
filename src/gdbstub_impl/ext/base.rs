use super::prelude::*;
use crate::protocol::commands::ext::Base;

use crate::arch::{Arch, Registers};
use crate::protocol::{IdKind, SpecificIdKind, SpecificThreadId};
use crate::target::ext::base::multithread::ThreadStopReason;
use crate::target::ext::base::{BaseOps, ResumeAction};
use crate::{FAKE_PID, SINGLE_THREAD_TID};

impl<T: Target, C: Connection> GdbStubImpl<T, C> {
    pub(crate) fn handle_base<'a>(
        &mut self,
        res: &mut ResponseWriter<C>,
        target: &mut T,
        command: Base<'a>,
    ) -> Result<HandlerStatus, Error<T::Error, C::Error>> {
        let handler_status = match command {
            // ------------------ Handshaking and Queries ------------------- //
            Base::qSupported(cmd) => {
                // XXX: actually read what the client supports, and enable/disable features
                // appropriately
                let _features = cmd.features.into_iter();

                res.write_str("PacketSize=")?;
                res.write_num(cmd.packet_buffer_len)?;

                res.write_str(";vContSupported+")?;
                res.write_str(";multiprocess+")?;
                res.write_str(";QStartNoAckMode+")?;

                if let Some(ops) = target.extended_mode() {
                    if ops.configure_aslr().is_some() {
                        res.write_str(";QDisableRandomization+")?;
                    }

                    if ops.configure_env().is_some() {
                        res.write_str(";QEnvironmentHexEncoded+")?;
                        res.write_str(";QEnvironmentUnset+")?;
                        res.write_str(";QEnvironmentReset+")?;
                    }

                    if ops.configure_startup_shell().is_some() {
                        res.write_str(";QStartupWithShell+")?;
                    }

                    if ops.configure_working_dir().is_some() {
                        res.write_str(";QSetWorkingDir+")?;
                    }
                }

                if target.agent().is_some() {
                    res.write_str(";QAgent+")?;
                }

                if let Some(ops) = target.breakpoints() {
                    if ops.sw_breakpoint().is_some() {
                        res.write_str(";swbreak+")?;
                    }

                    if ops.hw_breakpoint().is_some() || ops.hw_watchpoint().is_some() {
                        res.write_str(";hwbreak+")?;
                    }

                    if ops.breakpoint_agent().is_some() {
                        res.write_str(";BreakpointCommands+")?;
                        res.write_str(";ConditionalBreakpoints+")?;
                    }
                }

                if T::Arch::target_description_xml().is_some() {
                    res.write_str(";qXfer:features:read+")?;
                }

                HandlerStatus::Handled
            }
            Base::QStartNoAckMode(_) => {
                self.no_ack_mode = true;
                HandlerStatus::NeedsOK
            }
            Base::qXferFeaturesRead(cmd) => {
                match T::Arch::target_description_xml() {
                    Some(xml) => {
                        let xml = xml.trim();
                        if cmd.offset >= xml.len() {
                            // no more data
                            res.write_str("l")?;
                        } else if cmd.offset + cmd.len >= xml.len() {
                            // last little bit of data
                            res.write_str("l")?;
                            res.write_binary(&xml.as_bytes()[cmd.offset..])?
                        } else {
                            // still more data
                            res.write_str("m")?;
                            res.write_binary(&xml.as_bytes()[cmd.offset..(cmd.offset + cmd.len)])?
                        }
                    }
                    // If the target hasn't provided their own XML, then the initial response to
                    // "qSupported" wouldn't have included  "qXfer:features:read", and gdb wouldn't
                    // send this packet unless it was explicitly marked as supported.
                    None => return Err(Error::PacketUnexpected),
                }
                HandlerStatus::Handled
            }

            // -------------------- "Core" Functionality -------------------- //
            // TODO: Improve the '?' response based on last-sent stop reason.
            // this will be particularly relevant when working on non-stop mode.
            Base::QuestionMark(_) => {
                res.write_str("S05")?;
                HandlerStatus::Handled
            }
            Base::qAttached(cmd) => {
                let is_attached = match target.extended_mode() {
                    // when _not_ running in extended mode, just report that we're attaching to an
                    // existing process.
                    None => true, // assume attached to an existing process
                    // When running in extended mode, we must defer to the target
                    Some(ops) => {
                        let pid: Pid = cmd.pid.ok_or(Error::PacketUnexpected)?;

                        #[cfg(feature = "alloc")]
                        {
                            let _ = ops; // doesn't actually query the target
                            *self.attached_pids.get(&pid).unwrap_or(&true)
                        }

                        #[cfg(not(feature = "alloc"))]
                        {
                            ops.query_if_attached(pid).handle_error()?.was_attached()
                        }
                    }
                };
                res.write_str(if is_attached { "1" } else { "0" })?;
                HandlerStatus::Handled
            }
            Base::g(_) => {
                let mut regs: <T::Arch as Arch>::Registers = Default::default();
                match target.base_ops() {
                    BaseOps::SingleThread(ops) => ops.read_registers(&mut regs),
                    BaseOps::MultiThread(ops) => {
                        ops.read_registers(&mut regs, self.current_mem_tid)
                    }
                }
                .handle_error()?;

                let mut err = Ok(());
                regs.gdb_serialize(|val| {
                    let res = match val {
                        Some(b) => res.write_hex_buf(&[b]),
                        None => res.write_str("xx"),
                    };
                    if let Err(e) = res {
                        err = Err(e);
                    }
                });
                err?;
                HandlerStatus::Handled
            }
            Base::G(cmd) => {
                let mut regs: <T::Arch as Arch>::Registers = Default::default();
                regs.gdb_deserialize(cmd.vals)
                    .map_err(|_| Error::TargetMismatch)?;

                match target.base_ops() {
                    BaseOps::SingleThread(ops) => ops.write_registers(&regs),
                    BaseOps::MultiThread(ops) => ops.write_registers(&regs, self.current_mem_tid),
                }
                .handle_error()?;

                HandlerStatus::NeedsOK
            }
            Base::m(cmd) => {
                let buf = cmd.buf;
                let addr = <T::Arch as Arch>::Usize::from_be_bytes(cmd.addr)
                    .ok_or(Error::TargetMismatch)?;

                let mut i = 0;
                let mut n = cmd.len;
                while n != 0 {
                    let chunk_size = n.min(buf.len());

                    use num_traits::NumCast;

                    let addr = addr + NumCast::from(i).ok_or(Error::TargetMismatch)?;
                    let data = &mut buf[..chunk_size];
                    match target.base_ops() {
                        BaseOps::SingleThread(ops) => ops.read_addrs(addr, data),
                        BaseOps::MultiThread(ops) => {
                            ops.read_addrs(addr, data, self.current_mem_tid)
                        }
                    }
                    .handle_error()?;

                    n -= chunk_size;
                    i += chunk_size;

                    res.write_hex_buf(data)?;
                }
                HandlerStatus::Handled
            }
            Base::M(cmd) => {
                let addr = <T::Arch as Arch>::Usize::from_be_bytes(cmd.addr)
                    .ok_or(Error::TargetMismatch)?;

                match target.base_ops() {
                    BaseOps::SingleThread(ops) => ops.write_addrs(addr, cmd.val),
                    BaseOps::MultiThread(ops) => {
                        ops.write_addrs(addr, cmd.val, self.current_mem_tid)
                    }
                }
                .handle_error()?;

                HandlerStatus::NeedsOK
            }
            Base::k(_) | Base::vKill(_) => {
                match target.extended_mode() {
                    // When not running in extended mode, stop the `GdbStub` and disconnect.
                    None => HandlerStatus::Disconnect(DisconnectReason::Kill),

                    // When running in extended mode, a kill command does not necessarily result in
                    // a disconnect...
                    Some(ops) => {
                        let pid = match command {
                            Base::vKill(cmd) => Some(cmd.pid),
                            _ => None,
                        };

                        let should_terminate = ops.kill(pid).handle_error()?;
                        if should_terminate.into() {
                            // manually write OK, since we need to return a DisconnectReason
                            res.write_str("OK")?;
                            HandlerStatus::Disconnect(DisconnectReason::Kill)
                        } else {
                            HandlerStatus::NeedsOK
                        }
                    }
                }
            }
            Base::D(_) => {
                // TODO: plumb-through Pid when exposing full multiprocess + extended mode
                res.write_str("OK")?; // manually write OK, since we need to return a DisconnectReason
                HandlerStatus::Disconnect(DisconnectReason::Disconnect)
            }
            Base::vCont(cmd) => {
                use crate::protocol::commands::_vCont::vCont;
                match cmd {
                    vCont::Query => {
                        res.write_str("vCont;c;C;s;S")?;
                        HandlerStatus::Handled
                    }
                    vCont::Actions(actions) => self.do_vcont(res, target, actions)?,
                }
            }
            // TODO?: support custom resume addr in 'c' and 's'
            //
            // unfortunately, this wouldn't be a particularly easy thing to implement, since the
            // vCont packet doesn't natively support custom resume addresses. This leaves a few
            // options for the implementation:
            //
            // 1. Adding new ResumeActions (i.e: ContinueWithAddr(U) and StepWithAddr(U))
            // 2. Automatically calling `read_registers`, updating the `pc`, and calling
            //    `write_registers` prior to resuming.
            //    - will require adding some sort of `get_pc_mut` method to the `Registers` trait.
            //
            // Option 1 is easier to implement, but puts more burden on the implementor. Option 2
            // will require more effort to implement (and will be less performant), but it will hide
            // this protocol wart from the end user.
            //
            // Oh, one more thought - there's a subtle pitfall to watch out for if implementing
            // Option 1: if the target is using conditional breakpoints, `do_vcont` has to be
            // modified to only pass the resume with address variants on the _first_ iteration
            // through the loop.
            Base::c(_) => {
                use crate::protocol::commands::_vCont::Actions;

                self.do_vcont(
                    res,
                    target,
                    Actions::new_continue(SpecificThreadId {
                        pid: None,
                        tid: self.current_resume_tid,
                    }),
                )?
            }
            Base::s(_) => {
                use crate::protocol::commands::_vCont::Actions;

                self.do_vcont(
                    res,
                    target,
                    Actions::new_step(SpecificThreadId {
                        pid: None,
                        tid: self.current_resume_tid,
                    }),
                )?
            }

            // ------------------- Multi-threading Support ------------------ //
            Base::H(cmd) => {
                use crate::protocol::commands::_h_upcase::Op;
                match cmd.kind {
                    Op::Other => match cmd.thread.tid {
                        IdKind::Any => {} // reuse old tid
                        // "All" threads doesn't make sense for memory accesses
                        IdKind::All => return Err(Error::PacketUnexpected),
                        IdKind::WithID(tid) => self.current_mem_tid = tid,
                    },
                    // technically, this variant is deprecated in favor of vCont...
                    Op::StepContinue => match cmd.thread.tid {
                        IdKind::Any => {} // reuse old tid
                        IdKind::All => self.current_resume_tid = SpecificIdKind::All,
                        IdKind::WithID(tid) => {
                            self.current_resume_tid = SpecificIdKind::WithID(tid)
                        }
                    },
                }
                HandlerStatus::NeedsOK
            }
            Base::qfThreadInfo(_) => {
                res.write_str("m")?;

                match target.base_ops() {
                    BaseOps::SingleThread(_) => res.write_specific_thread_id(SpecificThreadId {
                        pid: Some(SpecificIdKind::WithID(FAKE_PID)),
                        tid: SpecificIdKind::WithID(SINGLE_THREAD_TID),
                    })?,
                    BaseOps::MultiThread(ops) => {
                        let mut err: Result<_, Error<T::Error, C::Error>> = Ok(());
                        let mut first = true;
                        ops.list_active_threads(&mut |tid| {
                            // TODO: replace this with a try block (once stabilized)
                            let e = (|| {
                                if !first {
                                    res.write_str(",")?
                                }
                                first = false;
                                res.write_specific_thread_id(SpecificThreadId {
                                    pid: Some(SpecificIdKind::WithID(FAKE_PID)),
                                    tid: SpecificIdKind::WithID(tid),
                                })?;
                                Ok(())
                            })();

                            if let Err(e) = e {
                                err = Err(e)
                            }
                        })
                        .map_err(Error::TargetError)?;
                        err?;
                    }
                }

                HandlerStatus::Handled
            }
            Base::qsThreadInfo(_) => {
                res.write_str("l")?;
                HandlerStatus::Handled
            }
            Base::T(cmd) => {
                let alive = match cmd.thread.tid {
                    IdKind::WithID(tid) => match target.base_ops() {
                        BaseOps::SingleThread(_) => tid == SINGLE_THREAD_TID,
                        BaseOps::MultiThread(ops) => {
                            ops.is_thread_alive(tid).map_err(Error::TargetError)?
                        }
                    },
                    // TODO: double-check if GDB ever sends other variants
                    // Even after ample testing, this arm has never been hit...
                    _ => return Err(Error::PacketUnexpected),
                };
                if alive {
                    HandlerStatus::NeedsOK
                } else {
                    // any error code will do
                    return Err(Error::NonFatalError(1));
                }
            }
        };
        Ok(handler_status)
    }

    #[allow(clippy::type_complexity)]
    fn do_vcont_single_thread(
        ops: &mut dyn crate::target::ext::base::singlethread::SingleThreadOps<
            Arch = T::Arch,
            Error = T::Error,
        >,
        res: &mut ResponseWriter<C>,
        actions: &crate::protocol::commands::_vCont::Actions,
    ) -> Result<ThreadStopReason<<T::Arch as Arch>::Usize>, Error<T::Error, C::Error>> {
        use crate::protocol::commands::_vCont::VContKind;

        let action = actions
            .iter()
            .next()
            // TODO?: use a more descriptive error if vcont has multiple actions in
            // single-threaded mode?
            .ok_or(Error::PacketUnexpected)?
            .ok_or(Error::PacketParse(
                crate::protocol::PacketParseError::MalformedCommand,
            ))?;

        let action = match action.kind {
            VContKind::Step => ResumeAction::Step,
            VContKind::Continue => ResumeAction::Continue,
            VContKind::StepWithSig(sig) => ResumeAction::StepWithSignal(sig),
            VContKind::ContinueWithSig(sig) => ResumeAction::ContinueWithSignal(sig),
            // TODO: update this case when non-stop mode is implemented
            VContKind::Stop => return Err(Error::PacketUnexpected),
        };

        let mut err = Ok(());
        let mut check_gdb_interrupt = || match res.as_conn().peek() {
            Ok(Some(0x03)) => true, // 0x03 is the interrupt byte
            Ok(Some(_)) => false,   // it's nothing that can't wait...
            Ok(None) => false,
            Err(e) => {
                err = Err(Error::ConnectionRead(e));
                true // break ASAP if a connection error occurred
            }
        };

        let ret = ops
            .resume(action, &mut check_gdb_interrupt)
            .map_err(Error::TargetError)?
            .into();

        err?;

        Ok(ret)
    }

    #[allow(clippy::type_complexity)]
    fn do_vcont_multi_thread(
        ops: &mut dyn crate::target::ext::base::multithread::MultiThreadOps<
            Arch = T::Arch,
            Error = T::Error,
        >,
        res: &mut ResponseWriter<C>,
        actions: &crate::protocol::commands::_vCont::Actions,
    ) -> Result<ThreadStopReason<<T::Arch as Arch>::Usize>, Error<T::Error, C::Error>> {
        // this is a pretty arbitrary choice, but it seems reasonable for most cases.
        let mut default_resume_action = ResumeAction::Continue;

        ops.clear_resume_actions().map_err(Error::TargetError)?;

        for action in actions.iter() {
            use crate::protocol::commands::_vCont::VContKind;

            let action = action.ok_or(Error::PacketParse(
                crate::protocol::PacketParseError::MalformedCommand,
            ))?;

            let resume_action = match action.kind {
                VContKind::Step => ResumeAction::Step,
                VContKind::Continue => ResumeAction::Continue,
                // there seems to be a GDB bug where it doesn't use `vCont` unless
                // `vCont?` returns support for resuming with a signal.
                VContKind::StepWithSig(sig) => ResumeAction::StepWithSignal(sig),
                VContKind::ContinueWithSig(sig) => ResumeAction::ContinueWithSignal(sig),
                // TODO: update this case when non-stop mode is implemented
                VContKind::Stop => return Err(Error::PacketUnexpected),
            };

            match action.thread.map(|thread| thread.tid) {
                // An action with no thread-id matches all threads
                None | Some(SpecificIdKind::All) => default_resume_action = resume_action,
                Some(SpecificIdKind::WithID(tid)) => ops
                    .set_resume_action(tid, resume_action)
                    .map_err(Error::TargetError)?,
            };
        }

        let mut err = Ok(());
        let mut check_gdb_interrupt = || match res.as_conn().peek() {
            Ok(Some(0x03)) => true, // 0x03 is the interrupt byte
            Ok(Some(_)) => false,   // it's nothing that can't wait...
            Ok(None) => false,
            Err(e) => {
                err = Err(Error::ConnectionRead(e));
                true // break ASAP if a connection error occurred
            }
        };

        let ret = ops
            .resume(default_resume_action, &mut check_gdb_interrupt)
            .map_err(Error::TargetError)?;

        err?;

        Ok(ret)
    }

    fn do_vcont(
        &mut self,
        res: &mut ResponseWriter<C>,
        target: &mut T,
        actions: crate::protocol::commands::_vCont::Actions,
    ) -> Result<HandlerStatus, Error<T::Error, C::Error>> {
        loop {
            let stop_reason = match target.base_ops() {
                BaseOps::SingleThread(ops) => Self::do_vcont_single_thread(ops, res, &actions)?,
                BaseOps::MultiThread(ops) => Self::do_vcont_multi_thread(ops, res, &actions)?,
            };

            match self.finish_vcont(res, target, stop_reason)? {
                Some(status) => break Ok(status),
                None => continue,
            }
        }
    }

    #[inline(always)]
    fn finish_vcont(
        &mut self,
        res: &mut ResponseWriter<C>,
        target: &mut T,
        stop_reason: ThreadStopReason<<T::Arch as Arch>::Usize>,
    ) -> Result<Option<HandlerStatus>, Error<T::Error, C::Error>> {
        let status = match stop_reason {
            ThreadStopReason::DoneStep | ThreadStopReason::GdbInterrupt => {
                res.write_str("S05")?;
                HandlerStatus::Handled
            }
            ThreadStopReason::Signal(code) => {
                res.write_str("S")?;
                res.write_num(code)?;
                HandlerStatus::Handled
            }
            ThreadStopReason::Halted => {
                res.write_str("W19")?; // SIGSTOP
                HandlerStatus::Disconnect(DisconnectReason::TargetHalted)
            }
            ThreadStopReason::SwBreak(tid)
            | ThreadStopReason::HwBreak(tid)
            | ThreadStopReason::Watch { tid, .. } => {
                self.current_mem_tid = tid;
                self.current_resume_tid = SpecificIdKind::WithID(tid);

                if let Some(agent_ops) = target
                    .breakpoints()
                    .and_then(|bp_ops| bp_ops.breakpoint_agent())
                {
                    if agent_ops.breakpoint_bytecode_executor().is_gdbstub()
                        && !self.eval_breakpoint_bytecode(agent_ops, res)?
                    {
                        // condition didn't evaluate to true - resume the target
                        return Ok(None);
                    }
                }

                res.write_str("T05")?;

                res.write_str("thread:")?;
                res.write_specific_thread_id(SpecificThreadId {
                    pid: Some(SpecificIdKind::WithID(FAKE_PID)),
                    tid: SpecificIdKind::WithID(tid),
                })?;
                res.write_str(";")?;

                match stop_reason {
                    // don't include addr on sw/hw break
                    ThreadStopReason::SwBreak(_) => res.write_str("swbreak:")?,
                    ThreadStopReason::HwBreak(_) => res.write_str("hwbreak:")?,
                    ThreadStopReason::Watch { kind, addr, .. } => {
                        use crate::target::ext::breakpoints::WatchKind;
                        match kind {
                            WatchKind::Write => res.write_str("watch:")?,
                            WatchKind::Read => res.write_str("rwatch:")?,
                            WatchKind::ReadWrite => res.write_str("awatch:")?,
                        }
                        res.write_num(addr)?;
                    }
                    _ => unreachable!(),
                };

                res.write_str(";")?;
                HandlerStatus::Handled
            }
        };

        Ok(Some(status))
    }

    // evaluate any bytecode associated with the current breakpoint, returning a
    // boolean indicating whether or not to report the breakpoint hit back to the
    // client.
    fn eval_breakpoint_bytecode(
        &mut self,
        target: crate::target::ext::breakpoints::BreakpointAgentOps<T>,
        res: &mut ResponseWriter<C>,
    ) -> Result<bool, Error<T::Error, C::Error>> {
        use crate::target::ext::breakpoints::{BreakpointAgentOps, BreakpointBytecodeKind};

        let mut regs: <T::Arch as Arch>::Registers = Default::default();
        let err = match target.base_ops() {
            BaseOps::SingleThread(ops) => ops.read_registers(&mut regs),
            BaseOps::MultiThread(ops) => ops.read_registers(&mut regs, self.current_mem_tid),
        };

        if let Err(err) = err {
            if let crate::target::TargetError::Fatal(e) = err {
                return Err(Error::TargetError(e));
            } else {
                // a non-fatal error occurred. it's a bit arbitrary what the behavior in this
                // case should be, so lets just assume that the best course of action is to
                // notify GDB of the breakpoint hit.
                return Ok(true);
            }
        }

        let addr = regs.pc();

        // TODO: instead of hardcoding error messages, pass a `ConsoleOutput` to the
        // Agent implementation and allow them to dump debug output.
        let mut err: Result<(), Error<T::Error, C::Error>> = Ok(());

        let mut condition = None;
        let mut eval_condition = |target: BreakpointAgentOps<T>, id| {
            use num_traits::*;

            match target.evaluate(id) {
                Ok(val) => condition = Some(!val.is_zero() || condition.unwrap_or(false)),
                Err(crate::target::TargetError::Fatal(e)) => err = Err(Error::TargetError(e)),
                // non-fatal error during bytecode evaluation.
                Err(_) => {
                    // TODO: replace with a try block once stabilized
                    let e = (|| {
                        let mut res = ResponseWriter::new(res.as_conn());
                        res.write_str("O")?;
                        res.write_hex_buf(b"error while evaluating breakpoint ")?;
                        res.write_hex_buf(b"condition\n")?;
                        res.flush()?;
                        Ok(())
                    })();

                    if let Err(e) = e {
                        err = Err(e)
                    }
                }
            }
            Ok(())
        };

        // ****************** FIXME *********************** //
        // rewrite API to not require this sort of iteration - if end users use the
        // whole "Option::take" approach to avoiding copies, then bad things can happen.

        target
            .get_breakpoint_bytecode(BreakpointBytecodeKind::Condition, addr, &mut eval_condition)
            .map_err(Error::TargetError)?;

        let condition = condition.unwrap_or(true);
        if condition {
            let mut eval_command = |target: BreakpointAgentOps<T>, id| {
                match target.evaluate(id) {
                    Ok(_) => {} // result is ignored when evaluating commands
                    Err(crate::target::TargetError::Fatal(e)) => return Err(e),
                    // non-fatal error during bytecode evaluation.
                    Err(_) => {
                        // TODO: replace with a try block once stabilized
                        let e = (|| {
                            let mut res = ResponseWriter::new(res.as_conn());
                            res.write_str("O")?;
                            res.write_hex_buf(b"error while evaluating breakpoint ")?;
                            res.write_hex_buf(b"command\n")?;
                            res.flush()?;
                            Ok(())
                        })();

                        if let Err(e) = e {
                            err = Err(e)
                        }
                    }
                }
                Ok(())
            };

            target
                .get_breakpoint_bytecode(BreakpointBytecodeKind::Command, addr, &mut eval_command)
                .map_err(Error::TargetError)?;
        }

        Ok(condition)
    }
}

use crate::target::ext::base::singlethread::StopReason;
impl<U> From<StopReason<U>> for ThreadStopReason<U> {
    fn from(st_stop_reason: StopReason<U>) -> ThreadStopReason<U> {
        match st_stop_reason {
            StopReason::DoneStep => ThreadStopReason::DoneStep,
            StopReason::GdbInterrupt => ThreadStopReason::GdbInterrupt,
            StopReason::Halted => ThreadStopReason::Halted,
            StopReason::SwBreak => ThreadStopReason::SwBreak(SINGLE_THREAD_TID),
            StopReason::HwBreak => ThreadStopReason::HwBreak(SINGLE_THREAD_TID),
            StopReason::Watch { kind, addr } => ThreadStopReason::Watch {
                tid: SINGLE_THREAD_TID,
                kind,
                addr,
            },
            StopReason::Signal(sig) => ThreadStopReason::Signal(sig),
        }
    }
}
