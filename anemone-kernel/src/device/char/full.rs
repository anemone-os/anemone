use crate::{
    device::char::{CharDev, CharSeekCtx, devfs::publish_char_device, register_char_device},
    prelude::*,
};

#[derive(Debug)]
struct Full;

const FULL_DEVNUM: CharDevNum = CharDevNum::new(
    MajorNum::new(devnum::char::major::MEMORY),
    MinorNum::new(devnum::char::minor::FULL),
);

impl CharDev for Full {
    fn devnum(&self) -> CharDevNum {
        FULL_DEVNUM
    }

    fn read(&self, buf: &mut [u8]) -> Result<usize, SysError> {
        buf.fill(0x00);
        Ok(buf.len())
    }

    fn write(&self, buf: &[u8]) -> Result<usize, SysError> {
        Err(SysError::NoSpace)
    }

    fn seek(&self, mut ctx: CharSeekCtx<'_>) -> Result<usize, SysError> {
        let _ = ctx.from();
        ctx.set_pos(0);
        Ok(0)
    }

    fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        // Linux reports the default writable mask for /dev/full even though a
        // write completes with ENOSPC: readiness means it will not block, not
        // that the operation will succeed.
        Ok(request.ready_or_unsupported(
            request.interests() & (PollEvent::READABLE | PollEvent::WRITABLE),
        ))
    }
}

#[initcall(probe)]
fn init() {
    match register_char_device("full".to_string(), Arc::new(Full)) {
        Ok(()) => {
            if let Err(err) = publish_char_device(FULL_DEVNUM) {
                knoticeln!(
                    "full device registered, but devfs publish failed: {:?}",
                    err
                );
            } else {
                knoticeln!("full device registered");
            }
        },
        Err(e) => {
            knoticeln!("failed to register full device: {:?}", e);
        },
    }
}
