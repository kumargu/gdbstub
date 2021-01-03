use paste::paste;

use crate::protocol::packet::PacketBuf;
use crate::target::Target;

pub(self) mod prelude {
    pub use super::ParseCommand;
    pub use crate::common::*;
    pub use crate::protocol::common::*;
    pub use crate::protocol::packet::PacketBuf;
    pub use core::convert::{TryFrom, TryInto};
}

pub trait ParseCommand<'a>: Sized {
    /// Try to parse a packet from the packet buffer.
    fn from_packet(buf: PacketBuf<'a>) -> Option<Self>;
}

// Breakpoint packets are special-cased, as the "Z" packet is parsed differently
// depending on whether or not the target implements the `Agent` extension.
//
// While it's entirely possible to eagerly parse the "Z" packet for bytecode,
// doing so would unnecessary bloat implementations that do not support
// evaluating agent expressions.

macro_rules! commands {
    (
        $(
            $ext:ident $(use $lt:lifetime)? {
                $($name:literal => $mod:ident::$command:ident$(<$lifetime:lifetime>)?,)*
            }
        )*
    ) => {paste! {
        $($(
            #[allow(non_snake_case, non_camel_case_types)]
            pub mod $mod;
        )*)*
        pub mod breakpoint;

        pub mod ext {
            $(
                #[allow(non_camel_case_types)]
                pub enum [<$ext:camel>] $(<$lt>)? {
                    $($command(super::$mod::$command<$($lifetime)?>),)*
                }
            )*

            use super::breakpoint::{BasicBreakpoint, BytecodeBreakpoint};
            #[allow(non_camel_case_types)]
            pub enum Breakpoints<'a> {
                z(BasicBreakpoint<'a>),
                Z(BasicBreakpoint<'a>),
                ZWithBytecode(BytecodeBreakpoint<'a>),
            }

        }

        /// GDB commands
        pub enum Command<'a> {
            $(
                [<$ext:camel>](ext::[<$ext:camel>]$(<$lt>)?),
            )*
            Breakpoints(ext::Breakpoints<'a>),
            Unknown(&'a [u8]),
        }

        impl<'a> Command<'a> {
            pub fn from_packet(
                target: &mut impl Target,
                mut buf: PacketBuf<'a>
            ) -> Option<Command<'a>> {
                // This locally-scoped trait enables using `base` as an `$ext`,
                // even through the `base` method on `Target` doesn't return an
                // Option.
                trait Hack { fn base(&mut self) -> Option<()> { Some(()) } }
                impl<T: Target> Hack for T {}

                // TODO?: use tries for more efficient longest prefix matching

                $(
                #[allow(clippy::string_lit_as_bytes)]
                if target.$ext().is_some() {
                    $(
                    if buf.strip_prefix($name.as_bytes()) {
                        crate::__dead_code_marker!($name, "prefix_match");

                        let cmd = $mod::$command::from_packet(buf)?;

                        return Some(
                            Command::[<$ext:camel>](
                                ext::[<$ext:camel>]::$command(cmd)
                            )
                        )
                    }
                    )*
                }
                )*

                if let Some(breakpoint_ops) = target.breakpoints() {
                    use breakpoint::{BasicBreakpoint, BytecodeBreakpoint};

                    if buf.strip_prefix(b"z") {
                        let cmd = BasicBreakpoint::from_slice(buf.into_body())?;
                        return Some(Command::Breakpoints(ext::Breakpoints::z(cmd)))
                    }

                    if buf.strip_prefix(b"Z") {
                        if breakpoint_ops.breakpoint_agent().is_none() {
                            let cmd = BasicBreakpoint::from_slice(buf.into_body())?;
                            return Some(Command::Breakpoints(ext::Breakpoints::Z(cmd)))
                        } else {
                            let cmd = BytecodeBreakpoint::from_slice(buf.into_body())?;
                            return Some(Command::Breakpoints(ext::Breakpoints::ZWithBytecode(cmd)))
                        }
                    }
                }

                Some(Command::Unknown(buf.into_body()))
            }
        }
    }};
}

commands! {
    base use 'a {
        "?" => question_mark::QuestionMark,
        "c" => _c::c<'a>,
        "D" => _d_upcase::D,
        "g" => _g::g,
        "G" => _g_upcase::G<'a>,
        "H" => _h_upcase::H,
        "k" => _k::k,
        "m" => _m::m<'a>,
        "M" => _m_upcase::M<'a>,
        "p" => _p::p,
        "P" => _p_upcase::P<'a>,
        "qAttached" => _qAttached::qAttached,
        "qfThreadInfo" => _qfThreadInfo::qfThreadInfo,
        "QStartNoAckMode" => _QStartNoAckMode::QStartNoAckMode,
        "qsThreadInfo" => _qsThreadInfo::qsThreadInfo,
        "qSupported" => _qSupported::qSupported<'a>,
        "qXfer:features:read" => _qXfer_features_read::qXferFeaturesRead,
        "s" => _s::s<'a>,
        "T" => _t_upcase::T,
        "vCont" => _vCont::vCont<'a>,
        "vKill" => _vKill::vKill,
    }

    extended_mode use 'a {
        "!" => exclamation_mark::ExclamationMark,
        "QDisableRandomization" => _QDisableRandomization::QDisableRandomization,
        "QEnvironmentHexEncoded" => _QEnvironmentHexEncoded::QEnvironmentHexEncoded<'a>,
        "QEnvironmentReset" => _QEnvironmentReset::QEnvironmentReset,
        "QEnvironmentUnset" => _QEnvironmentUnset::QEnvironmentUnset<'a>,
        "QSetWorkingDir" => _QSetWorkingDir::QSetWorkingDir<'a>,
        "QStartupWithShell" => _QStartupWithShell::QStartupWithShell,
        "R" => _r_upcase::R,
        "vAttach" => _vAttach::vAttach,
        "vRun" => _vRun::vRun<'a>,
    }

    monitor_cmd use 'a {
        "qRcmd" => _qRcmd::qRcmd<'a>,
    }

    section_offsets {
        "qOffsets" => _qOffsets::qOffsets,
    }

    agent {
        "QAgent" => _QAgent::QAgent,
    }
}
