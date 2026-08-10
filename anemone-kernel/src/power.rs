//! System Power Subsystem.

use crate::{net as kernel_net, prelude::*};

pub trait PowerOffHandler: Send {
    unsafe fn poweroff(&self);
}

/// **This trait expects a cold reboot implementation**
pub trait RebootHandler: Send {
    unsafe fn reboot(&self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MachineIntent {
    PowerOff,
    Reboot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EpisodePhase {
    Orderly,
    Emergency,
    MachineAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EpisodeSnapshot {
    executor: CpuId,
    intent: MachineIntent,
    phase: EpisodePhase,
}

struct TerminalEpisode {
    encoded: AtomicUsize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EmergencyDisposition {
    Execute,
    Halt,
}

impl TerminalEpisode {
    const INTENT_REBOOT: usize = 1 << 0;
    const PHASE_EMERGENCY: usize = 1 << 1;
    const PHASE_MACHINE_ACTION: usize = 1 << 2;
    const EXECUTOR_SHIFT: usize = 3;

    const fn new() -> Self {
        Self {
            encoded: AtomicUsize::new(0),
        }
    }

    fn encode(snapshot: EpisodeSnapshot) -> usize {
        let executor = snapshot
            .executor
            .logical_id()
            .checked_add(1)
            .and_then(|id| id.checked_shl(Self::EXECUTOR_SHIFT as u32))
            .expect("CPU ID cannot be encoded in terminal episode");
        let intent = match snapshot.intent {
            MachineIntent::PowerOff => 0,
            MachineIntent::Reboot => Self::INTENT_REBOOT,
        };
        let phase = match snapshot.phase {
            EpisodePhase::Orderly => 0,
            EpisodePhase::Emergency => Self::PHASE_EMERGENCY,
            EpisodePhase::MachineAction => Self::PHASE_MACHINE_ACTION,
        };
        executor | intent | phase
    }

    fn decode(encoded: usize) -> Option<EpisodeSnapshot> {
        if encoded == 0 {
            return None;
        }
        let executor = CpuId::new((encoded >> Self::EXECUTOR_SHIFT) - 1);
        let intent = if encoded & Self::INTENT_REBOOT == 0 {
            MachineIntent::PowerOff
        } else {
            MachineIntent::Reboot
        };
        let phase = match encoded & (Self::PHASE_EMERGENCY | Self::PHASE_MACHINE_ACTION) {
            0 => EpisodePhase::Orderly,
            Self::PHASE_EMERGENCY => EpisodePhase::Emergency,
            Self::PHASE_MACHINE_ACTION => EpisodePhase::MachineAction,
            _ => unreachable!("terminal episode has two phases"),
        };
        Some(EpisodeSnapshot {
            executor,
            intent,
            phase,
        })
    }

    /// Atomically publishes the executor, intent, and initial phase. This is
    /// the episode's only election point; a failed caller must never run the
    /// orderly plan or change the already-published intent.
    fn publish_orderly(&self, executor: CpuId, intent: MachineIntent) -> bool {
        let encoded = Self::encode(EpisodeSnapshot {
            executor,
            intent,
            phase: EpisodePhase::Orderly,
        });
        self.encoded
            .compare_exchange(0, encoded, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Enters emergency execution without electing a second executor. The
    /// orderly winner may switch its own episode; every other caller, a panic
    /// during machine action, and a recursive emergency must stop locally.
    fn enter_emergency(&self, executor: CpuId) -> EmergencyDisposition {
        loop {
            let encoded = self.encoded.load(Ordering::Acquire);
            let Some(snapshot) = Self::decode(encoded) else {
                let emergency = Self::encode(EpisodeSnapshot {
                    executor,
                    intent: MachineIntent::PowerOff,
                    phase: EpisodePhase::Emergency,
                });
                match self.encoded.compare_exchange(
                    0,
                    emergency,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => return EmergencyDisposition::Execute,
                    Err(_) => continue,
                }
            };

            if snapshot.executor != executor || snapshot.phase != EpisodePhase::Orderly {
                return EmergencyDisposition::Halt;
            }
            let emergency = Self::encode(EpisodeSnapshot {
                phase: EpisodePhase::Emergency,
                ..snapshot
            });
            match self.encoded.compare_exchange(
                encoded,
                emergency,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return EmergencyDisposition::Execute,
                Err(_) => continue,
            }
        }
    }

    /// Marks the shared final path before invoking a handler. A panic from a
    /// machine handler then takes the recursive-emergency halt path instead of
    /// retrying this or an earlier handler.
    fn enter_machine_action(&self, executor: CpuId) -> Option<MachineIntent> {
        loop {
            let encoded = self.encoded.load(Ordering::Acquire);
            let snapshot = Self::decode(encoded)?;
            if snapshot.executor != executor || snapshot.phase == EpisodePhase::MachineAction {
                return None;
            }
            let machine = Self::encode(EpisodeSnapshot {
                phase: EpisodePhase::MachineAction,
                ..snapshot
            });
            match self.encoded.compare_exchange(
                encoded,
                machine,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(snapshot.intent),
                Err(_) => continue,
            }
        }
    }
}

struct HaltHandler;

impl PowerOffHandler for HaltHandler {
    unsafe fn poweroff(&self) {
        kemergln!("no power off handler succeeded, halting the system");
        halt_current_cpu();
    }
}

impl RebootHandler for HaltHandler {
    unsafe fn reboot(&self) {
        kemergln!("no reboot handler succeeded, halting the system");
        halt_current_cpu();
    }
}

struct PowerOffHandlerList {
    handlers: Vec<Box<dyn PowerOffHandler>>,
    /// Structural fallback rather than a regular registration: ordinary
    /// handlers can only be inserted before this unique, permanent last item.
    halt: HaltHandler,
}

impl PowerOffHandlerList {
    const fn new() -> Self {
        Self {
            handlers: Vec::new(),
            halt: HaltHandler,
        }
    }

    unsafe fn run(&self) -> ! {
        for handler in &self.handlers {
            unsafe {
                handler.poweroff();
            }
        }
        unsafe {
            self.halt.poweroff();
        }
        unreachable!()
    }
}

struct RebootHandlerList {
    handlers: Vec<Box<dyn RebootHandler>>,
    /// See [`PowerOffHandlerList::halt`].
    halt: HaltHandler,
}

impl RebootHandlerList {
    const fn new() -> Self {
        Self {
            handlers: Vec::new(),
            halt: HaltHandler,
        }
    }

    unsafe fn run(&self) -> ! {
        for handler in &self.handlers {
            unsafe {
                handler.reboot();
            }
        }
        unsafe {
            self.halt.reboot();
        }
        unreachable!()
    }
}

struct OrderlyStep {
    name: &'static str,
    run: unsafe fn(),
}

static TERMINAL_EPISODE: TerminalEpisode = TerminalEpisode::new();
static POWER_OFF_HANDLERS: SpinLock<PowerOffHandlerList> =
    SpinLock::new(PowerOffHandlerList::new());
static REBOOT_HANDLERS: SpinLock<RebootHandlerList> = SpinLock::new(RebootHandlerList::new());

// This literal order is the global orderly plan. Participant discovery and
// owner-local traversal stay in the subsystem facades referenced here.
static ORDERLY_PLAN: [OrderlyStep; 3] = [
    OrderlyStep {
        name: "filesystem",
        run: fs::on_shutdown,
    },
    OrderlyStep {
        name: "network",
        run: kernel_net::shutdown,
    },
    OrderlyStep {
        name: "device",
        run: device::shutdown,
    },
];

/// Register a power off handler before the permanent halt fallback.
pub fn register_power_off_handler(handler: Box<dyn PowerOffHandler>) {
    POWER_OFF_HANDLERS.lock_irqsave().handlers.push(handler);
}

/// Register a reboot handler before the permanent halt fallback.
pub fn register_reboot_handler(handler: Box<dyn RebootHandler>) {
    REBOOT_HANDLERS.lock_irqsave().handlers.push(handler);
}

#[inline(never)]
pub(crate) fn halt_current_cpu() -> ! {
    // Losers can arrive from an ordinary requester with interrupts enabled.
    // Mask them here so this terminal fallback cannot resume scheduler or
    // unrelated interrupt work while another CPU owns the episode.
    unsafe {
        IntrArch::local_intr_disable();
    }
    loop {
        core::hint::spin_loop();
    }
}

fn stop_other_cpus(context: &str) {
    if let Err(err) = broadcast_ipi_async(IpiPayload::StopExecution) {
        kemergln!(
            "failed to broadcast StopExecution during {} shutdown: {:?}",
            context,
            err
        );
    }
}

unsafe fn run_machine_action() -> ! {
    let cpu = cur_cpu_id();
    let Some(intent) = TERMINAL_EPISODE.enter_machine_action(cpu) else {
        halt_current_cpu();
    };
    kemergln!(
        "system-power: executor {} entering {:?} machine action",
        cpu,
        intent
    );
    match intent {
        MachineIntent::PowerOff => unsafe { POWER_OFF_HANDLERS.lock_irqsave().run() },
        MachineIntent::Reboot => unsafe { REBOOT_HANDLERS.lock_irqsave().run() },
    }
}

unsafe fn run_orderly(intent: MachineIntent) -> ! {
    let cpu = cur_cpu_id();
    if !TERMINAL_EPISODE.publish_orderly(cpu, intent) {
        kemergln!(
            "system-power: CPU {} lost terminal episode publication, halting",
            cpu
        );
        halt_current_cpu();
    }

    kemergln!(
        "system-power: CPU {} published orderly {:?} episode",
        cpu,
        intent
    );
    stop_other_cpus("orderly");
    for step in &ORDERLY_PLAN {
        kemergln!("system-power: starting {} shutdown step", step.name);
        unsafe {
            (step.run)();
        }
        kemergln!("system-power: completed {} shutdown step", step.name);
    }
    unsafe {
        run_machine_action();
    }
}

/// Publish or switch the current episode to its emergency path.
pub(crate) fn enter_emergency() -> EmergencyDisposition {
    TERMINAL_EPISODE.enter_emergency(cur_cpu_id())
}

/// Enter the same machine-handler list used by orderly shutdown. The panic
/// owner performs diagnostics and CPU stop before invoking this terminal step.
pub(crate) unsafe fn run_emergency_machine_action() -> ! {
    unsafe {
        run_machine_action();
    }
}

/// Power off the system.
pub unsafe fn power_off() -> ! {
    unsafe {
        run_orderly(MachineIntent::PowerOff);
    }
}

/// Reboot the system.
pub unsafe fn reboot() -> ! {
    unsafe {
        run_orderly(MachineIntent::Reboot);
    }
}

mod api {
    use anemone_abi::system::native::power::SHUTDOWN_MAGIC;

    use super::*;

    // Successful shutdown never returns through the generated wrapper, so it
    // has no completed invocation for the syscall profiler.
    #[syscall(SYS_POWER_SHUTDOWN, profile = false)]
    pub fn sys_power_shutdown(magic: u64) -> Result<u64, SysError> {
        if magic != SHUTDOWN_MAGIC {
            return Err(SysError::InvalidArgument);
        }

        unsafe {
            power_off();
        }
    }
}
pub use api::*;

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn first_publication_freezes_executor_and_intent() {
        let episode = TerminalEpisode::new();
        assert!(episode.publish_orderly(CpuId::new(1), MachineIntent::Reboot));
        assert!(!episode.publish_orderly(CpuId::new(2), MachineIntent::PowerOff));
        assert_eq!(
            TerminalEpisode::decode(episode.encoded.load(Ordering::Acquire)),
            Some(EpisodeSnapshot {
                executor: CpuId::new(1),
                intent: MachineIntent::Reboot,
                phase: EpisodePhase::Orderly,
            })
        );
    }

    #[kunit]
    fn orderly_winner_switches_same_episode_to_emergency() {
        let episode = TerminalEpisode::new();
        assert!(episode.publish_orderly(CpuId::new(1), MachineIntent::Reboot));
        assert_eq!(
            episode.enter_emergency(CpuId::new(1)),
            EmergencyDisposition::Execute
        );
        assert_eq!(
            TerminalEpisode::decode(episode.encoded.load(Ordering::Acquire)),
            Some(EpisodeSnapshot {
                executor: CpuId::new(1),
                intent: MachineIntent::Reboot,
                phase: EpisodePhase::Emergency,
            })
        );
        assert_eq!(
            episode.enter_emergency(CpuId::new(2)),
            EmergencyDisposition::Halt
        );
    }

    #[kunit]
    fn emergency_is_single_executor_and_machine_action_is_terminal() {
        let episode = TerminalEpisode::new();
        assert_eq!(
            episode.enter_emergency(CpuId::new(3)),
            EmergencyDisposition::Execute
        );
        assert_eq!(
            episode.enter_emergency(CpuId::new(3)),
            EmergencyDisposition::Halt
        );
        assert_eq!(
            episode.enter_machine_action(CpuId::new(3)),
            Some(MachineIntent::PowerOff)
        );
        assert_eq!(episode.enter_machine_action(CpuId::new(3)), None);
    }
}
