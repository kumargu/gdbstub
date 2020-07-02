use super::prelude::*;

#[derive(PartialEq, Eq, Debug)]
pub struct vContQuestionMark;

impl<'a> ParseCommand<'a> for vContQuestionMark {
    fn from_packet(buf: PacketBuf<'a>) -> Option<Self> {
        if !buf.into_body().is_empty() {
            return None;
        }
        Some(vContQuestionMark)
    }
}
