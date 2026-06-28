//! OOM killer worker.

use crate::{
    prelude::*,
    task::{
        for_each_thread_group_from, get_thread_group,
        kthread::{KThreadBuilder, KThreadCtx, KThreadHandle},
        sig::{
            SigNo, Signal,
            info::{SiCode, SigInfoFields, SigKill},
        },
    },
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

static OOM_KILLER: SpinLock<Option<KThreadHandle>> = SpinLock::new(None);

#[derive(Debug)]
struct OomVictim {
    tg: Arc<ThreadGroup>,
    tgid: Tid,
    exclusive_pages: usize,
}

#[initcall(late)]
fn init_oom_killer() {
    let worker = KThreadBuilder::new("oom-killer-0")
        .spawn(oom_killer_entry, NilOpaque::new())
        .unwrap_or_else(|err| panic!("failed to spawn OOM killer: {:?}", err));

    let mut slot = OOM_KILLER.lock();
    assert!(slot.is_none(), "OOM killer initialized twice");
    *slot = Some(worker);
}

pub fn wake_oom_killer() {
    if let Some(worker) = OOM_KILLER.lock().as_ref().cloned() {
        worker.wake();
    }
}

fn oom_killer_entry(ctx: KThreadCtx, _: AnyOpaque) -> i32 {
    let mut active_victim = None;

    loop {
        if ctx.should_stop() {
            break;
        }

        ctx.wait_until_woken(|| frame_allocator_stats().exceeds_oom_kill_threshold());

        if ctx.should_stop() {
            break;
        }
        if !frame_allocator_stats().exceeds_oom_kill_threshold() {
            continue;
        }

        run_oom_kill_round(&mut active_victim);
    }

    0
}

fn run_oom_kill_round(active_victim: &mut Option<Tid>) {
    if active_victim_is_pending(*active_victim) {
        yield_now();
        return;
    }
    *active_victim = None;

    let stats = frame_allocator_stats();
    let Some(victim) = select_victim() else {
        knoticeln!(
            "oom killer: no eligible victim while frame usage is {}/{} pages",
            stats.used_pages(),
            stats.total_pages
        );
        yield_now();
        return;
    };

    kalertln!(
        "oom killer: killing tgid {} with {} exclusive physical page(s); frame usage {}/{} pages",
        victim.tgid,
        victim.exclusive_pages,
        stats.used_pages(),
        stats.total_pages
    );

    let sender = get_current_task();
    victim.tg.recv_signal(Signal::new(
        SigNo::SIGKILL,
        SiCode::Kernel,
        SigInfoFields::Kill(SigKill {
            pid: sender.tgid(),
            uid: sender.cred().uid.real,
        }),
    ));
    *active_victim = Some(victim.tgid);
    yield_now();
}

fn active_victim_is_pending(active_victim: Option<Tid>) -> bool {
    let Some(tgid) = active_victim else {
        return false;
    };

    let Some(tg) = get_thread_group(&tgid) else {
        return false;
    };

    !matches!(tg.status().life_cycle(), ThreadGroupLifeCycle::Exited(_))
}

fn select_victim() -> Option<OomVictim> {
    let groups = thread_group_snapshot();
    let mut victim: Option<OomVictim> = None;

    for tg in groups {
        let Some(exclusive_pages) = score_thread_group(&tg) else {
            continue;
        };
        if exclusive_pages == 0 {
            continue;
        }

        let candidate = OomVictim {
            tgid: tg.tgid(),
            tg,
            exclusive_pages,
        };
        if match victim.as_ref() {
            Some(old) => candidate.exclusive_pages > old.exclusive_pages,
            None => true,
        } {
            victim = Some(candidate);
        }
    }

    victim
}

fn thread_group_snapshot() -> Vec<Arc<ThreadGroup>> {
    let mut groups = Vec::new();
    for_each_thread_group_from(|tg| groups.push(tg.clone()), None);
    groups
}

fn score_thread_group(tg: &Arc<ThreadGroup>) -> Option<usize> {
    let tgid = tg.tgid();
    if tg.ty() != ThreadGroupType::User {
        return None;
    }
    if tgid == Tid::IDLE || tgid == Tid::INIT {
        return None;
    }
    if !matches!(tg.status().life_cycle(), ThreadGroupLifeCycle::Alive) {
        return None;
    }

    let leader = tg.leader()?;
    let flags = leader.flags();
    // Victim eligibility is a user-process topology policy. The kernel flag is
    // only a defensive cache check here, not the kthread classifier.
    if flags.is_idle() || flags.is_kernel() {
        return None;
    }

    let uspace = leader.try_clone_uspace_handle()?;
    Some(uspace.exclusive_physical_pages_snapshot())
}
