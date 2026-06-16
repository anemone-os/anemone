//! exit-related system calls and APIs.
//!
//! - https://www.man7.org/linux/man-pages/man2/exit.2.html

use crate::{
    prelude::{user_access::UserWritePtr, *},
    task::{
        futex::exit_robust_list,
        sig::{
            SigNo, Signal,
            info::{SiCode, SigChld, SigInfoFields, SigKill},
        },
        tid::Tid,
    },
};

pub mod exit;
pub mod exit_group;

/// Exit current task through the user-process lifecycle path.
pub fn kernel_exit(code: ExitCode) -> ! {
    {
        let task = get_current_task();
        if task.tid() == Tid::INIT {
            panic!("init task shall not exit");
        }
        kdebugln!("kernel_exit: task={} exit with code {:?}", task.tid(), code);
        if task.kthread().is_some() {
            panic!(
                "kthread task {} entered user-process kernel_exit path",
                task.tid()
            );
        }

        if let Some(addr) = task.get_clear_child_tid() {
            let usp = task.clone_uspace_handle();
            let cleard = {
                let mut guard = usp.lock();
                match UserWritePtr::<Tid>::try_new(addr, &mut guard) {
                    Ok(mut uptr) => {
                        uptr.write(Tid::new(0));
                        true
                    },
                    Err(e) => {
                        knoticeln!(
                            "failed to clear child tid for task {}: {:?} at address {:#x}",
                            task.tid(),
                            e,
                            addr.get()
                        );
                        false
                    },
                }
            };
            if cleard {
                if let Err(e) = futex::wake_at(&task.clone_uspace_handle(), addr, 1) {
                    knoticeln!(
                        "failed to clear child tid for {}: {:?} at address {:#x}",
                        task.tid(),
                        e,
                        addr.get()
                    );
                } else {
                    kdebugln!(
                        "cleared child tid and woke futex for task {} at address {:#x}",
                        task.tid(),
                        addr.get()
                    );
                }
            }
        }

        if !task.flags().is_kernel() {
            if let Err(e) = exit_robust_list() {
                knoticeln!(
                    "failed to exit robust list for task {}: {:?}",
                    task.tid(),
                    e,
                );
            }
        }

        let tg = task.get_thread_group();

        defer_to_dispose(task.clone());

        // Opened-description final-release can run fanotify mark cleanup and
        // wake waiters, so it must happen while the exiting task is still in a
        // sleepable, interrupts-enabled process context. Deferred task disposal
        // and `Drop` are memory cleanup only.
        task.close_all_fds_for_exit();

        task.set_exit_code(code);

        // TODO: this is not very accurate. but good enough for now.
        tg.accumulate_member_usage(&task);

        let old_uspace = task.try_clone_uspace_handle();
        if let Some(old_uspace) = &old_uspace {
            if task.is_last_user_of_uspace(old_uspace) {
                old_uspace.detach_all_sysv_shm_for(task.tgid());
                unsafe {
                    old_uspace.clear();
                }
            }
        }

        let is_last = task.detach_from_topology();

        // if we are the last thread in this thread group, we should do the cleanup
        // work.

        // a longer critical section must be held here to avoid races. TODO: explain
        // why.
        if is_last {
            let mut tg_inner = tg.inner.write();

            let xcode = match tg_inner.status.life_cycle {
                ThreadGroupLifeCycle::Alive => {
                    // no one called exit_group before. all threads call exit... use our exit code.
                    code
                },
                ThreadGroupLifeCycle::Exiting(existing_code) => {
                    // someone already called exit_group before. use their exit code.
                    existing_code
                },
                ThreadGroupLifeCycle::Exited(existing_code) => {
                    panic!("thread group already exited with code {:?}", existing_code);
                },
            };

            // 1. reparent orphan children.
            // following operations are a bit tricky, but it's safe.
            //
            // TODO: but i think we'd better switch to a more reasonable and less
            // error-prone design later.
            drop(tg_inner);
            tg.reparent_orphan_children();
            tg_inner = tg.inner.write();

            // 2. set status to Exited, so that wait4 can reap this thread group.
            tg_inner.status.life_cycle = ThreadGroupLifeCycle::Exited(xcode);

            drop(tg_inner);

            let cpu_usage = tg.cpu_usage_snapshot();

            if let Some(terminate_signal) = tg.terminate_signal() {
                let uid = tg
                    .leader()
                    .map(|leader| leader.cred().uid.real)
                    .unwrap_or_else(|| task.cred().uid.real);
                tg.get_parent().recv_signal(Signal::new(
                    terminate_signal,
                    SiCode::Kernel,
                    SigInfoFields::Chld(SigChld {
                        pid: tg.tgid(),
                        uid,
                        // TODO: this is false. we should look at si_code first.
                        status: match xcode {
                            ExitCode::Exited(xcode) => xcode as i32,
                            ExitCode::Signaled(signo) => signo.as_usize() as i32,
                        },
                        utime: duration_to_ticks(cpu_usage.self_user() + cpu_usage.reaped_user()),
                        stime: duration_to_ticks(
                            cpu_usage.self_kernel() + cpu_usage.reaped_kernel(),
                        ),
                    }),
                ));
            }

            // 3. publish child_exited event.
            tg.get_parent().child_exited.publish(1, false);

            // 4. orphan children reparented to init may contain zombie thread groups. let's
            //    publish that to init as well.
            // this hardcoding is a bit ugly. when we support subreapers, we should publish
            // this to the actual reaper.
            get_init_task()
                .get_thread_group()
                .child_exited
                .publish(1, false);

            task.vfork_done.publish(1, true);
        }

        scheduler_zombie_tail(&task);
    }

    schedule_never_return()
}

/// Exit current kthread without entering ordinary user-process cleanup.
pub fn kthread_exit(result: i32) -> ! {
    {
        let task = get_current_task();
        let tg = task.get_thread_group();
        assert!(
            tg.ty() == ThreadGroupType::KThread,
            "kthread_exit called by non-kthread task {}",
            task.tid()
        );
        assert!(
            task.tid() != Tid::KTHREADD,
            "kthreadd must not exit in phase-4 kthread core"
        );
        task.complete_kthread_returned_entry(result);

        // Phase 4 keeps kthreads from owning fd tables. This assertion is the
        // temporary task-local resource closeout boundary: before any future
        // consumer is allowed to inherit or open fds in a kthread, this must be
        // replaced by a full kthread-safe fd-table closeout helper that can run
        // opened-description final-release hooks in process context.
        assert!(
            task.opened_fd_numbers_snapshot().is_empty(),
            "kthread {} exited with published fd table entries",
            task.tid()
        );

        task.set_exit_code(ExitCode::Exited(result as i8));
        defer_to_dispose(task.clone());
        tg.unpublish_kthread_topology();
        task.publish_kthread_external_exit(result);
        scheduler_zombie_tail(&task);
    }

    schedule_never_return()
}

fn scheduler_zombie_tail(task: &Task) {
    // ORDER MATTERS.
    // Setting status to Zombie must be the last thing before we drop the task.
    // Otherwise if a preemption occurs after setting status to Zombie but
    // before we, e.g., detach from thread group, we'll end up with a zombie
    // task that still appears in the thread group.
    task.update_sched_state_with(|prev| {
        assert!(
            matches!(prev, TaskSchedState::Runnable | TaskSchedState::Zombie),
            "exiting task should not own an active wait-core state: task={} state={:?}",
            task.tid(),
            prev,
        );
        (TaskSchedState::Zombie, ())
    });
}

fn schedule_never_return() -> ! {
    with_intr_disabled(|| unsafe {
        schedule();
    });

    unreachable!("exited task should never be scheduled again");
}

/// Exit current thread group.
///
/// NOTE: thread who called this function might not be the one who actually
/// performs the exit.
///
/// TODO: we should reserve [TidHandle] of leader thread until the thread group
/// is reaped.
pub fn kernel_exit_group(code: ExitCode) -> ! {
    {
        let task = get_current_task();
        if task.tid() == Tid::INIT {
            panic!("init task shall not exit");
        }
        let tg = task.get_thread_group();
        let is_exiting = tg.update_life_cycle_with(|prev| match prev {
            ThreadGroupLifeCycle::Alive => (ThreadGroupLifeCycle::Exiting(code), false),
            ThreadGroupLifeCycle::Exiting(existing_code) => {
                (ThreadGroupLifeCycle::Exiting(*existing_code), true)
            },
            ThreadGroupLifeCycle::Exited(code) => {
                panic!("thread group already exited with code {:?}", code);
            },
        });

        if is_exiting {
            // someone already started exiting this thread group. we can just exit this
            // thread.
            drop(tg);
            drop(task);

            kernel_exit(code)
        }

        // we are the first thread calling exit_group.

        // TODO: when signal is implemented, we should send SIGKILL to all other
        // threads in this thread group.
        tg.for_each_member(|member| {
            if member.tid() != task.tid() {
                member.recv_signal(Signal::new(
                    SigNo::SIGKILL,
                    SiCode::Kernel,
                    SigInfoFields::Kill(SigKill {
                        pid: task.tgid(),
                        uid: task.cred().uid.real,
                    }),
                ))
            }
        });

        // no need to wait anymore. the last thread that exits will do the
        // cleanup work.
    }
    kernel_exit(code)
}
