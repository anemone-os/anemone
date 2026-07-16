//! One-shot boot discovery for cards attached before system startup.
//!
//! Stage 2 deliberately exposes no rescan or removal entry point. The sole
//! caller is a successfully probed host controller.

mod sd;

pub(crate) use sd::{
    SdCardState, SdCommand, SdProtocolError, SdR1Flags, SdR1Response, command_argument,
};

use super::{
    MmcCardIdentity, MmcCardKind, MmcCardKinds, MmcHost, MmcHostDevice, MmcHostError, register_card,
};
use crate::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmcDiscoveryError {
    NoCard,
    ProtocolRejected(MmcCardKind),
    ProtocolUnavailable(MmcCardKind),
    UnsupportedCard,
    InitializationTimeout,
    Transport(MmcHostError),
    InvalidIdentity,
    Publication(SysError),
}

/// Execute the only Stage-2 discovery attempt for one boot-published host.
///
/// Failure never tears down the host: a controller remains useful diagnostic
/// state even when no supported card was present during the cold scan.
pub fn discover_cold_card(host: Arc<MmcHostDevice>) {
    let caps = host.caps();
    kinfoln!(
        "mmc host{}: cold scan candidates={:?}",
        host.id().get(),
        caps.allowed_kinds
    );

    let mut unavailable = None;

    let mut sd_failure = None;
    if caps.allowed_kinds.contains(MmcCardKinds::SD_MEMORY) {
        match sd::attach(host.as_ref()) {
            Ok(identity) => {
                let identity = MmcCardIdentity::SdMemory(identity);
                match register_card(host.clone(), identity) {
                    Ok(card) => {
                        let sd = match card.identity() {
                            MmcCardIdentity::SdMemory(sd) => sd,
                        };
                        kinfoln!(
                            "mmc host{}: card{} attached kind={:?} rca={:#x} capacity={}B addressing={:?}",
                            host.id().get(),
                            card.id().get(),
                            card.kind(),
                            sd.rca.get(),
                            sd.capacity_bytes,
                            sd.addressing
                        );
                    },
                    Err(error) => log_failure(host.as_ref(), MmcDiscoveryError::Publication(error)),
                }
                return;
            },
            Err(error @ MmcDiscoveryError::NoCard)
            | Err(error @ MmcDiscoveryError::ProtocolRejected(_)) => {
                kerrln!(
                    "mmc host{}: SD Memory candidate failed: {:?}",
                    host.id().get(),
                    error
                );
                sd_failure = Some(error);
            },
            Err(error) => {
                log_failure(host.as_ref(), error);
                return;
            },
        }
    }

    if caps.allowed_kinds.contains(MmcCardKinds::SDIO) {
        kerrln!(
            "mmc host{}: TODO(stage 3+): SDIO/SDIO-combo attach is currently not supported because a fixed SDIO specification is unavailable",
            host.id().get()
        );
        unavailable = Some(MmcCardKind::Sdio);
    }

    if caps.allowed_kinds.contains(MmcCardKinds::MMC) {
        kerrln!(
            "mmc host{}: TODO(stage 3+): MMC/eMMC discovery is currently not supported because the required JEDEC specification is unavailable",
            host.id().get()
        );
        unavailable = Some(MmcCardKind::Mmc);
    }

    // Unsupported candidates must not overwrite the failure returned by the
    // supported SD Memory path; that error identifies the attempted protocol.
    if let Some(error) = sd_failure {
        log_failure(host.as_ref(), error);
    } else if let Some(kind) = unavailable {
        log_failure(host.as_ref(), MmcDiscoveryError::ProtocolUnavailable(kind));
    } else {
        kinfoln!("mmc host{}: no card detected", host.id().get());
    }
}

fn log_failure(host: &MmcHostDevice, error: MmcDiscoveryError) {
    kerrln!(
        "mmc host{}: cold discovery failed: {:?}",
        host.id().get(),
        error
    );
}
