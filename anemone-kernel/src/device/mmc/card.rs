//! Published MMC card identity and boot-only publication transaction.
//!
//! A card is constructed only after protocol discovery has completed. Firmware
//! capabilities select candidates, but never become card identity.

use super::{MmcHostDevice, bus};
use crate::{
    device::kobject::{KObject, KObjectBase, KObjectOps},
    prelude::*,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MmcCardKind {
    SdMemory,
    Mmc,
    Sdio,
    SdioCombo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdAddressing {
    /// Standard-capacity cards use a byte address in memory commands.
    Byte,
    /// High/extended-capacity cards use a 512-byte logical-block address.
    Block,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SdOperatingConditions: u32 {
        const VOLTAGE_2_7_TO_3_6 = 0x00ff_8000;
        const SWITCH_1_8_V_ACCEPTED = 1 << 24;
        const UHS_II_CARD_STATUS = 1 << 29;
        const CAPACITY_STATUS = 1 << 30;
        const POWER_UP_COMPLETE = 1 << 31;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct SdRelativeAddress(u16);

impl SdRelativeAddress {
    pub(crate) const fn from_raw(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u16 {
        self.0
    }

    pub const fn command_argument(self) -> u32 {
        (self.0 as u32) << 16
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct SdCid([u32; 4]);

impl SdCid {
    pub(crate) const fn from_response(words: [u32; 4]) -> Self {
        Self(words)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct SdCsd([u32; 4]);

impl SdCsd {
    pub(crate) const fn from_response(words: [u32; 4]) -> Self {
        Self(words)
    }

    pub const fn words(self) -> [u32; 4] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdCardIdentity {
    pub operating_conditions: SdOperatingConditions,
    pub rca: SdRelativeAddress,
    /// Canonical response order: bits 127:96 through bits 31:0.
    pub cid: SdCid,
    /// Canonical response order: bits 127:96 through bits 31:0.
    pub csd: SdCsd,
    pub addressing: SdAddressing,
    pub capacity_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmcCardIdentity {
    SdMemory(SdCardIdentity),
}

impl MmcCardIdentity {
    pub const fn kind(self) -> MmcCardKind {
        match self {
            Self::SdMemory(_) => MmcCardKind::SdMemory,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MmcCardId(u32);

impl MmcCardId {
    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Debug, KObject, Device)]
pub struct MmcCardDevice {
    #[kobject]
    kobj_base: KObjectBase,
    #[device]
    dev_base: DeviceBase,
    id: MmcCardId,
    identity: MmcCardIdentity,
    /// Command capability only. The device-model parent link remains the
    /// hierarchy truth source, so this weak reference must not drive identity.
    host: Weak<MmcHostDevice>,
}

impl KObjectOps for MmcCardDevice {}

impl DeviceOps for MmcCardDevice {}

impl MmcCardDevice {
    fn new(
        kobj_base: KObjectBase,
        id: MmcCardId,
        identity: MmcCardIdentity,
        host: Weak<MmcHostDevice>,
    ) -> Self {
        Self {
            kobj_base,
            dev_base: DeviceBase::new(None),
            id,
            identity,
            host,
        }
    }

    pub const fn id(&self) -> MmcCardId {
        self.id
    }

    pub const fn kind(&self) -> MmcCardKind {
        self.identity.kind()
    }

    pub const fn identity(&self) -> MmcCardIdentity {
        self.identity
    }

    pub fn host(&self) -> Option<Arc<MmcHostDevice>> {
        self.host.upgrade()
    }
}

static NEXT_CARD_ID: SpinLock<u32> = SpinLock::new(0);

fn allocate_card_id() -> Result<MmcCardId, SysError> {
    let mut next = NEXT_CARD_ID.lock_irqsave();
    let id = MmcCardId(*next);
    *next = next.checked_add(1).ok_or(SysError::ResourceExhausted)?;
    Ok(id)
}

/// Publish one fully identified boot-time card below its host.
///
/// All fallible preparation precedes the parent/bus publication boundary.
/// The current bus registration API is infallible and Stage 2 has no remove or
/// hotplug path, so no parallel presence state or rollback API is introduced.
pub fn register_card(
    host: Arc<MmcHostDevice>,
    identity: MmcCardIdentity,
) -> Result<Arc<MmcCardDevice>, SysError> {
    let id = allocate_card_id()?;
    let name = ident_format!("mmc-card{}", id.get())
        .expect("an MMC card ID must fit in a kernel object name");
    let card = Arc::new(MmcCardDevice::new(
        KObjectBase::new(name),
        id,
        identity,
        Arc::downgrade(&host),
    ));

    card.set_parent(Some(host.clone()));
    host.add_child(card.clone());
    bus::register_device(card.clone());
    Ok(card)
}
