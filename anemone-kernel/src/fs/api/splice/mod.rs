//! splice syscall family.
//!
//! Stage 1 keeps real data movement limited to copy-backed `splice(2)`.
//! `tee(2)` and `vmsplice(2)` are registered for errno compatibility only.

use anemone_abi::fs::linux::splice::*;

use crate::{
    fs::pipe::{PipeEndpointInfo, pipe_endpoint_info},
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
    task::files::{Fd, FileDesc},
};

pub mod splice;
pub mod tee;
pub mod vmsplice;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct SpliceFlags: u32 {
        const MOVE = SPLICE_F_MOVE;
        const NONBLOCK = SPLICE_F_NONBLOCK;
        const MORE = SPLICE_F_MORE;
        const GIFT = SPLICE_F_GIFT;
    }
}

impl SpliceFlags {
    pub(super) fn parse(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }

    pub(super) fn reject_nonblock_functional_path(
        self,
        syscall_name: &'static str,
    ) -> Result<(), SysError> {
        if self.contains(Self::NONBLOCK) {
            // Per-call nonblocking is not equivalent to the opened-description
            // O_NONBLOCK bit already carried in FileIoCtx. Fail closed until
            // splice has an explicit per-call I/O context rather than silently
            // running a blocking transfer.
            knoticeln!(
                "{}: SPLICE_F_NONBLOCK is not supported on functional paths yet",
                syscall_name
            );
            return Err(SysError::InvalidArgument);
        }

        Ok(())
    }

    pub(super) fn notice_copy_backed_splice_noops(self) {
        let ignored = self & (Self::MOVE | Self::MORE | Self::GIFT);
        if !ignored.is_empty() {
            // MOVE/MORE are Linux splice hints, and GIFT is meaningful only to
            // vmsplice page donation. This copy-backed stage has no page-share
            // or stream-hint path, so the known bits are visible no-ops until
            // a later zero-copy/pipe-buffer iteration replaces this bridge.
            knoticeln!(
                "sys_splice: copy-backed stage ignores known splice hint flags: {:#x}",
                ignored.bits()
            );
        }
    }
}

pub(super) fn parse_fd(raw: u64) -> Result<Fd, SysError> {
    Fd::try_from_syscall_arg(raw)
}

pub(super) fn pipe_endpoint_of(file: &FileDesc) -> Option<PipeEndpointInfo> {
    pipe_endpoint_info(file.vfs_file().as_ref())
}
