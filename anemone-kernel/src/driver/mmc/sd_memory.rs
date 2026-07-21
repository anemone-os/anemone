//! Synchronous SD Memory block endpoint.
//!
//! The card has already completed protocol attach before this driver probes.
//! Stage 2 deliberately issues only single-block polling/PIO commands. Caller
//! buffers spanning multiple logical blocks are split sequentially and are not
//! atomic if a later block fails.

use crate::{
    device::{
        block::{
            BlockDev, BlockDevClass, BlockDevRegistration, BlockDriver, BlockSize,
            devfs::publish_block_device, register_block_device, register_block_driver,
        },
        devnum::GeneralMinorAllocator,
        kobject::{KObjIdent, KObjectBase, KObjectOps},
        mmc::{
            MmcCardDevice, MmcCardDriver, MmcCardIdentity, MmcCardKind, MmcCardMatch, MmcData,
            MmcHost, MmcHostError, MmcRequest, MmcResponseType, SdCardState, SdCommand,
            SdProtocolError, SdR1Flags, SdR1Response, SdRelativeAddress, command_argument,
            register_driver,
        },
    },
    prelude::*,
};

#[derive(Debug, KObject, Driver)]
struct SdMemoryBlockDriver {
    #[kobject]
    kobj_base: KObjectBase,
    #[driver]
    drv_base: DriverBase,
}

#[derive(Debug)]
struct SdMemoryBlockDev {
    devnum: BlockDevNum,
    card: Arc<MmcCardDevice>,
    /// Stable block geometry derived once from the card's immutable identity.
    /// Stage 2 has no replacement/hotplug path, so this cannot become stale.
    total_blocks: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SdBlockIoError {
    Host(MmcHostError),
    Protocol(SdProtocolError),
}

impl KObjectOps for SdMemoryBlockDriver {}

impl DriverOps for SdMemoryBlockDriver {
    fn probe(&self, device: Arc<dyn Device>) -> Result<(), SysError> {
        let card =
            Arc::downcast::<MmcCardDevice>(device).map_err(|_| SysError::DriverIncompatible)?;
        if card.kind() != MmcCardKind::SdMemory {
            return Err(SysError::DriverIncompatible);
        }
        let sd = match card.identity() {
            MmcCardIdentity::SdMemory(identity) => identity,
        };
        let blocks = sd.capacity_bytes / BlockSize::UNIT_BYTES as u64;
        if blocks == 0 || sd.capacity_bytes % BlockSize::UNIT_BYTES as u64 != 0 {
            return Err(SysError::ProbeFailed);
        }
        let total_blocks = usize::try_from(blocks).map_err(|_| SysError::ResourceExhausted)?;

        let minor = MINORS
            .lock_irqsave()
            .alloc()
            .ok_or(SysError::NoMinorAvailable)?;
        let devnum = BlockDevNum::new(*MAJOR.get(), minor);
        let endpoint = Arc::new(SdMemoryBlockDev {
            devnum,
            card: card.clone(),
            total_blocks,
        });
        let name = register_block_device(BlockDevRegistration {
            devnum,
            class: BlockDevClass::Mmc,
            device: endpoint,
        })?;

        kinfoln!(
            "sd-memory card{}: registered as {} devnum={} blocks={} block_size={}B",
            card.id().get(),
            name,
            devnum,
            total_blocks,
            BlockSize::UNIT_BYTES
        );
        if let Err(error) = publish_block_device(devnum) {
            knoticeln!(
                "sd-memory card{} registered as {}, but devfs publish failed: {:?}",
                card.id().get(),
                name,
                error
            );
        }
        Ok(())
    }

    fn shutdown(&self, _device: &dyn Device) {
        // CMD24 returns only after the controller observes data completion and
        // the driver confirms transfer state with CMD13. There is no queued
        // writeback to flush in this synchronous Stage-2 endpoint.
    }

    fn as_mmc_card_driver(&self) -> Option<&dyn MmcCardDriver> {
        Some(self)
    }
}

impl MmcCardDriver for SdMemoryBlockDriver {
    fn match_table(&self) -> &[MmcCardMatch] {
        &[MmcCardMatch {
            kind: MmcCardKind::SdMemory,
        }]
    }
}

impl BlockDriver for SdMemoryBlockDriver {
    fn major(&self) -> MajorNum {
        *MAJOR.get()
    }
}

impl BlockDev for SdMemoryBlockDev {
    fn devnum(&self) -> BlockDevNum {
        self.devnum
    }

    fn block_size(&self) -> BlockSize {
        BlockSize::new(1)
    }

    fn total_blocks(&self) -> usize {
        self.total_blocks
    }

    fn read_blocks(&self, block_idx: usize, buffer: &mut [u8]) -> Result<(), SysError> {
        // step 1: Validate the complete caller-visible range before issuing the
        // first command, so argument errors cannot cause partial I/O.
        let blocks = validate_range(self.total_blocks, block_idx, buffer.len())?;

        // step 2: Resolve the attached card's command capability and immutable
        // addressing mode without duplicating controller or card state.
        let host = self.card.host().ok_or(SysError::NoSuchDevice)?;
        let sd = match self.card.identity() {
            MmcCardIdentity::SdMemory(identity) => identity,
        };

        // step 3: Split the block-layer range into ordered CMD17 requests; the
        // first failure stops the caller-visible operation.
        for (offset, chunk) in buffer.chunks_exact_mut(BlockSize::UNIT_BYTES).enumerate() {
            let lba = block_idx + offset;
            if let Err(error) = read_one(host.as_ref(), sd.addressing, lba, chunk) {
                kerrln!(
                    "sd-memory card{}: read failed lba={} opcode={} error={:?}",
                    self.card.id().get(),
                    lba,
                    SdCommand::ReadSingleBlock as u8,
                    error
                );
                return Err(SysError::IO);
            }
        }
        assert_eq!(blocks * BlockSize::UNIT_BYTES, buffer.len());
        Ok(())
    }

    fn write_blocks(&self, block_idx: usize, buffer: &[u8]) -> Result<(), SysError> {
        // step 1: Validate the complete caller-visible range before issuing the
        // first command, so argument errors cannot cause a partial write.
        let blocks = validate_range(self.total_blocks, block_idx, buffer.len())?;

        // step 2: Resolve the attached card's command capability, addressing
        // mode, and RCA from the committed identity snapshot.
        let host = self.card.host().ok_or(SysError::NoSuchDevice)?;
        let sd = match self.card.identity() {
            MmcCardIdentity::SdMemory(identity) => identity,
        };

        // step 3: Split the block-layer range into ordered CMD24 requests; no
        // later block is attempted after a partial failure.
        for (offset, chunk) in buffer.chunks_exact(BlockSize::UNIT_BYTES).enumerate() {
            let lba = block_idx + offset;
            if let Err(error) = write_one(host.as_ref(), sd.addressing, sd.rca, lba, chunk) {
                kerrln!(
                    "sd-memory card{}: write failed lba={} opcode={} error={:?}",
                    self.card.id().get(),
                    lba,
                    SdCommand::WriteBlock as u8,
                    error
                );
                return Err(SysError::IO);
            }
        }
        assert_eq!(blocks * BlockSize::UNIT_BYTES, buffer.len());
        Ok(())
    }
}

fn validate_range(total_blocks: usize, block_idx: usize, len: usize) -> Result<usize, SysError> {
    if len == 0 || !len.is_multiple_of(BlockSize::UNIT_BYTES) {
        return Err(SysError::InvalidArgument);
    }
    let blocks = len / BlockSize::UNIT_BYTES;
    let end = block_idx
        .checked_add(blocks)
        .ok_or(SysError::InvalidArgument)?;
    if end > total_blocks {
        return Err(SysError::IO);
    }
    Ok(blocks)
}

fn read_one(
    host: &dyn MmcHost,
    addressing: crate::device::mmc::SdAddressing,
    lba: usize,
    buffer: &mut [u8],
) -> Result<(), SdBlockIoError> {
    // step 1: Convert the logical block index according to SDSC versus
    // SDHC/SDXC addressing, rejecting overflow before issuing I/O.
    let argument = command_argument(addressing, lba).map_err(SdBlockIoError::Protocol)?;

    // step 2: Issue one 512-byte CMD17 read through the synchronous host.
    let mut request = MmcRequest {
        command: crate::device::mmc::MmcCommand::new(
            SdCommand::ReadSingleBlock as u8,
            argument,
            MmcResponseType::R1,
        ),
        data: Some(MmcData::Read {
            block_size: BlockSize::UNIT_BYTES as u32,
            blocks: 1,
            buffer,
        }),
        stop: None,
    };
    host.execute(&mut request).map_err(SdBlockIoError::Host)?;

    // step 3: Accept data only when the card's R1 reports no protocol error.
    SdR1Response::decode(request.command.response[0])
        .check()
        .map_err(SdBlockIoError::Protocol)
}

fn write_one(
    host: &dyn MmcHost,
    addressing: crate::device::mmc::SdAddressing,
    rca: SdRelativeAddress,
    lba: usize,
    buffer: &[u8],
) -> Result<(), SdBlockIoError> {
    // step 1: Convert and validate the card command address before writing.
    let argument = command_argument(addressing, lba).map_err(SdBlockIoError::Protocol)?;

    // step 2: Issue one 512-byte CMD24 write through the synchronous host.
    let mut request = MmcRequest {
        command: crate::device::mmc::MmcCommand::new(
            SdCommand::WriteBlock as u8,
            argument,
            MmcResponseType::R1,
        ),
        data: Some(MmcData::Write {
            block_size: BlockSize::UNIT_BYTES as u32,
            blocks: 1,
            buffer,
        }),
        stop: None,
    };
    host.execute(&mut request).map_err(SdBlockIoError::Host)?;

    // step 3: Reject a write command whose immediate R1 contains an error.
    SdR1Response::decode(request.command.response[0])
        .check()
        .map_err(SdBlockIoError::Protocol)?;

    // step 4: Query CMD13 after DAT busy clears and require transfer state plus
    // READY_FOR_DATA before allowing the next block request.
    let mut status = MmcRequest {
        command: crate::device::mmc::MmcCommand::new(
            SdCommand::SendStatus as u8,
            rca.command_argument(),
            MmcResponseType::R1,
        ),
        data: None,
        stop: None,
    };
    host.execute(&mut status).map_err(SdBlockIoError::Host)?;
    let response = SdR1Response::decode(status.command.response[0]);
    response.check().map_err(SdBlockIoError::Protocol)?;
    // step 4.1: Interpret CMD13 through named protocol fields; only
    // READY_FOR_DATA plus TRAN may release the next write.
    if !response.flags.contains(SdR1Flags::READY_FOR_DATA)
        || response.card_state != Some(SdCardState::Transfer)
    {
        return Err(SdBlockIoError::Protocol(SdProtocolError::NotReady(
            status.command.response[0],
        )));
    }
    Ok(())
}

static MAJOR: MonoOnce<MajorNum> = unsafe { MonoOnce::new() };
static MINORS: Lazy<SpinLock<GeneralMinorAllocator>> =
    Lazy::new(|| SpinLock::new(GeneralMinorAllocator::new()));

#[initcall(driver)]
fn init() {
    let driver = Arc::new(SdMemoryBlockDriver {
        kobj_base: KObjectBase::new(KObjIdent::try_from("sd-memory-block").unwrap()),
        drv_base: DriverBase::new(),
    });
    let major = register_block_driver(driver.clone())
        .unwrap_or_else(|error| panic!("failed to register SD Memory block driver: {:?}", error));
    MAJOR.init(|slot| {
        slot.write(major);
    });
    register_driver(driver);
}

#[kunit]
fn block_range_is_checked() {
    assert_eq!(validate_range(8, 7, 512), Ok(1));
    assert_eq!(validate_range(8, 8, 512), Err(SysError::IO));
    assert_eq!(
        validate_range(usize::MAX, usize::MAX, 512),
        Err(SysError::InvalidArgument)
    );
    assert_eq!(validate_range(8, 0, 513), Err(SysError::InvalidArgument));
}

#[kunit]
fn single_block_io_uses_capacity_addressing_and_status() {
    struct FakeHost {
        commands: SpinLock<Vec<crate::device::mmc::MmcCommand>>,
    }

    impl MmcHost for FakeHost {
        fn caps(&self) -> crate::device::mmc::MmcHostCaps {
            unreachable!()
        }

        fn set_ios(
            &self,
            _ios: crate::device::mmc::MmcIos,
        ) -> Result<crate::device::mmc::MmcIos, MmcHostError> {
            unreachable!()
        }

        fn execute(&self, request: &mut MmcRequest<'_>) -> Result<(), MmcHostError> {
            if let Some(MmcData::Read { buffer, .. }) = request.data.as_mut() {
                buffer.fill(0xa5);
            }
            if request.command.opcode == SdCommand::SendStatus as u8 {
                request.command.response[0] =
                    SdCardState::Transfer.response_bits() | SdR1Flags::READY_FOR_DATA.bits();
            }
            self.commands.lock().push(request.command);
            Ok(())
        }

        fn recover_transport(&self) -> Result<(), MmcHostError> {
            Ok(())
        }
    }

    let host = FakeHost {
        commands: SpinLock::new(Vec::new()),
    };
    let mut read_buffer = [0u8; BlockSize::UNIT_BYTES];
    read_one(
        &host,
        crate::device::mmc::SdAddressing::Block,
        7,
        &mut read_buffer,
    )
    .unwrap();
    assert!(read_buffer.iter().all(|byte| *byte == 0xa5));

    let write_buffer = [0x5au8; BlockSize::UNIT_BYTES];
    write_one(
        &host,
        crate::device::mmc::SdAddressing::Byte,
        SdRelativeAddress::from_raw(3),
        7,
        &write_buffer,
    )
    .unwrap();

    let commands = host.commands.lock();
    assert_eq!(commands.len(), 3);
    assert_eq!(commands[0].opcode, SdCommand::ReadSingleBlock as u8);
    assert_eq!(commands[0].argument, 7);
    assert_eq!(commands[1].opcode, SdCommand::WriteBlock as u8);
    assert_eq!(commands[1].argument, 7 * BlockSize::UNIT_BYTES as u32);
    assert_eq!(commands[2].opcode, SdCommand::SendStatus as u8);
    assert_eq!(commands[2].argument, 3 << 16);
}
